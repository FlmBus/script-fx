//! Patch panel renderer built on resvg/usvg.
//!
//! resvg renders static SVG only (no scripts, no SMIL/CSS animation, no network).
//! Animation is done by "patch & reparse": the bindings compute per-element
//! patches (transform/style/text), which are spliced into the original SVG
//! source at byte offsets found once at load time. The patched source is then
//! re-parsed by usvg and rendered by resvg. Nothing is rendered when no bound
//! value changed.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use patch::{Axis, Binding, ParamSpec, Pivot, Patch, Source, Theme, Values};
use resvg::usvg::{self, fontdb, roxmltree};
use resvg::{tiny_skia, usvg::ImageHrefResolver};

pub use tiny_skia::Pixmap;

#[derive(Debug)]
pub enum PanelError {
    Io(std::io::Error),
    Svg(String),
    NoControlPanel,
}

impl std::error::Error for PanelError {}

impl std::fmt::Display for PanelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PanelError::Io(e) => write!(f, "io: {e}"),
            PanelError::Svg(e) => write!(f, "svg: {e}"),
            PanelError::NoControlPanel => write!(f, "patch has no control_panel.svg"),
        }
    }
}

/// Bundled fonts only. Scanning system fonts took 7.5s cold on a dev machine
/// (unacceptable inside a DAW) and would make patches render differently on
/// every machine. Patches can ship extra fonts in `<patch>/fonts/`.
fn base_fonts() -> &'static fontdb::Database {
    static DB: OnceLock<fontdb::Database> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_font_data(include_bytes!("../assets/NotoSans-Regular.ttf").to_vec());
        db.load_font_data(include_bytes!("../assets/NotoSans-Bold.ttf").to_vec());
        for generic in ["sans-serif", "serif", "monospace", "cursive", "fantasy"] {
            match generic {
                "sans-serif" => db.set_sans_serif_family("Noto Sans"),
                "serif" => db.set_serif_family("Noto Sans"),
                "monospace" => db.set_monospace_family("Noto Sans"),
                "cursive" => db.set_cursive_family("Noto Sans"),
                _ => db.set_fantasy_family("Noto Sans"),
            }
        }
        db
    })
}

fn fonts_for(dir: &Path) -> Arc<fontdb::Database> {
    let mut db = base_fonts().clone();
    let fonts_dir = dir.join("fonts");
    if fonts_dir.is_dir() {
        db.load_fonts_dir(fonts_dir);
    }
    Arc::new(db)
}

/// Theme keys become CSS classes: `.fill-accent`, `.stroke-accent`, `.stop-accent`.
fn theme_css(theme: &Theme) -> String {
    let mut css = String::new();
    for (k, v) in &theme.0 {
        css.push_str(&format!(
            ".fill-{k}{{fill:{v}}} .stroke-{k}{{stroke:{v}}} .stop-{k}{{stop-color:{v}}}\n"
        ));
    }
    css
}

/// `<image href>` may only load files from inside the patch folder.
fn sandboxed_image_resolver(dir: PathBuf) -> ImageHrefResolver<'static> {
    ImageHrefResolver {
        resolve_data: ImageHrefResolver::default_data_resolver(),
        resolve_string: Box::new(move |href: &str, opts: &usvg::Options| {
            let path = patch::resolve_inside(&dir, href).ok()?;
            let data = Arc::new(std::fs::read(&path).ok()?);
            match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
                "png" => Some(usvg::ImageKind::PNG(data)),
                "jpg" | "jpeg" => Some(usvg::ImageKind::JPEG(data)),
                "gif" => Some(usvg::ImageKind::GIF(data)),
                "webp" => Some(usvg::ImageKind::WEBP(data)),
                "svg" => usvg::Tree::from_data(&data, opts).ok().map(usvg::ImageKind::SVG),
                _ => None,
            }
        }),
    }
}

enum Op {
    Rotate { from: f32, to: f32, pivot: (f32, f32) },
    Translate { from: [f32; 2], to: [f32; 2] },
    Scale { from: f32, to: f32, axis: Axis, pivot: (f32, f32) },
    Opacity { from: f32, to: f32 },
    Fill { from: [f32; 3], to: [f32; 3] },
    TextParam(ParamSpec),
    TextPercent,
    TextFormat(String),
}

struct BoundOp {
    element: usize,
    source: Source,
    op: Op,
}

/// Byte offsets inside the SVG source for one bound element.
struct ElementSlots {
    /// Right after `<tagname` — where new attributes get inserted.
    insert_at: usize,
    transform: Option<Range<usize>>,
    style: Option<Range<usize>>,
    /// First text node below this element (for text bindings).
    text: Option<Range<usize>>,
}

#[derive(Default)]
struct ElementPatch {
    transform: String,
    style: String,
    text: Option<String>,
}

pub struct DragTarget {
    pub id: String,
    /// Bounding box in SVG user units, refreshed after every re-render so
    /// hit areas follow moving elements (e.g. a fader cap).
    pub rect: usvg::Rect,
    /// `Source::Macro(slot)` or `Source::Param(index)`.
    pub target: Source,
    pub range: f32,
}

#[derive(Default, Clone, Copy)]
pub struct Stats {
    pub patch_us: f32,
    pub parse_us: f32,
    pub render_us: f32,
    pub renders: u64,
}

pub struct SvgControlPanel {
    source: String,
    elements: Vec<ElementSlots>,
    ops: Vec<BoundOp>,
    pub drags: Vec<DragTarget>,
    options: usvg::Options<'static>,
    size: usvg::Size,
    pixmap: Option<Pixmap>,
    last_inputs: Vec<f32>,
    last_px: (u32, u32),
    pub stats: Stats,
    /// Non-fatal problems (e.g. a binding refers to a missing element).
    pub warnings: Vec<String>,
}

impl SvgControlPanel {
    pub fn load(patch: &Patch) -> Result<Self, PanelError> {
        let ui = patch.control_panel.as_ref().ok_or(PanelError::NoControlPanel)?;
        let source = std::fs::read_to_string(&ui.svg).map_err(PanelError::Io)?;
        Self::from_source(source, &patch.dir, patch, &ui.bindings)
    }

    fn from_source(
        source: String,
        dir: &Path,
        patch: &Patch,
        bindings: &[Binding],
    ) -> Result<Self, PanelError> {
        let mut options = usvg::Options {
            resources_dir: Some(dir.to_path_buf()),
            fontdb: fonts_for(dir),
            style_sheet: Some(theme_css(&patch.theme)),
            image_href_resolver: sandboxed_image_resolver(dir.to_path_buf()),
            ..Default::default()
        };
        // usvg would otherwise silently fall back; keep defaults explicit.
        options.default_size = usvg::Size::from_wh(100.0, 100.0).unwrap();

        // Initial (unpatched) tree: gives us the canvas size and the geometry
        // used for pivots and drag hit areas.
        let tree =
            usvg::Tree::from_str(&source, &options).map_err(|e| PanelError::Svg(e.to_string()))?;
        let doc = roxmltree::Document::parse_with_options(
            &source,
            roxmltree::ParsingOptions {
                allow_dtd: false,
                ..Default::default()
            },
        )
        .map_err(|e| PanelError::Svg(e.to_string()))?;

        let mut warnings = Vec::new();
        let mut element_ids: Vec<String> = Vec::new();
        let mut elements = Vec::new();
        let mut ops = Vec::new();
        let mut drags = Vec::new();

        let bbox_center = |id: &str| -> Option<(f32, f32)> {
            let r = tree.node_by_id(id)?.abs_bounding_box()?;
            Some((r.x() + r.width() / 2.0, r.y() + r.height() / 2.0))
        };

        for b in bindings {
            let id = b.element_id();
            let Some(node) = doc.descendants().find(|n| n.attribute("id") == Some(id)) else {
                warnings.push(format!("binding target '#{id}' not found in SVG"));
                continue;
            };

            if let Binding::Drag { target, range, .. } = b {
                match tree.node_by_id(id).and_then(|n| n.abs_bounding_box()) {
                    Some(rect) => drags.push(DragTarget {
                        id: id.to_string(),
                        rect,
                        target: patch.resolve_source(target).expect("validated by patch"),
                        range: *range,
                    }),
                    None => warnings.push(format!("drag target '#{id}' has no visible geometry")),
                }
                continue;
            }

            let element = match element_ids.iter().position(|e| e == id) {
                Some(i) => i,
                None => {
                    element_ids.push(id.to_string());
                    elements.push(slots_for(&source, node));
                    elements.len() - 1
                }
            };

            let mut source_ = patch
                .resolve_source(b.source().unwrap())
                .expect("validated by patch");
            let mut pivot_warnings = Vec::new();
            let mut pivot = |p: &Option<Pivot>| -> (f32, f32) {
                match p {
                    Some(Pivot::Point([x, y])) => (*x, *y),
                    Some(Pivot::Element(pid)) => bbox_center(pid).unwrap_or_else(|| {
                        pivot_warnings.push(format!("pivot element '#{pid}' not found"));
                        (0.0, 0.0)
                    }),
                    None => bbox_center(id).unwrap_or((0.0, 0.0)),
                }
            };
            let op = match b {
                Binding::Rotate { from, to, pivot: p, .. } => Op::Rotate {
                    from: *from,
                    to: *to,
                    pivot: pivot(p),
                },
                Binding::Translate { from, to, .. } => Op::Translate { from: *from, to: *to },
                Binding::Scale { from, to, axis, pivot: p, .. } => Op::Scale {
                    from: *from,
                    to: *to,
                    axis: *axis,
                    pivot: pivot(p),
                },
                Binding::Opacity { from, to, .. } => Op::Opacity { from: *from, to: *to },
                Binding::Fill { from, to, .. } => Op::Fill {
                    from: parse_color(patch.theme.resolve(from)),
                    to: parse_color(patch.theme.resolve(to)),
                },
                Binding::Text { format, .. } => {
                    if elements[element].text.is_none() {
                        warnings.push(format!("text target '#{id}' has no text content to replace"));
                    }
                    match (source_.clone(), format) {
                        (Source::Param(i), None) => Op::TextParam(patch.params[i].clone()),
                        // A macro driving a single param shows that param's value.
                        (Source::Macro(m), None) => match patch.macros[m].targets.as_slice() {
                            [t] => {
                                let i = patch.param_index(&t.param).expect("validated");
                                source_ = Source::Param(i);
                                Op::TextParam(patch.params[i].clone())
                            }
                            _ => Op::TextPercent,
                        },
                        (_, f) => Op::TextFormat(f.clone().unwrap_or_else(|| "%.2f".into())),
                    }
                }
                Binding::Drag { .. } => unreachable!(),
            };
            warnings.extend(pivot_warnings);
            ops.push(BoundOp {
                element,
                source: source_,
                op,
            });
        }

        let size = tree.size();
        Ok(SvgControlPanel {
            source,
            elements,
            ops,
            drags,
            options,
            size,
            pixmap: None,
            last_inputs: Vec::new(),
            last_px: (0, 0),
            stats: Stats::default(),
            warnings,
        })
    }

    /// Canvas size in SVG user units.
    pub fn size(&self) -> (f32, f32) {
        (self.size.width(), self.size.height())
    }

    pub fn pixmap(&self) -> Option<&Pixmap> {
        self.pixmap.as_ref()
    }

    pub fn drag_target_at(&self, x: f32, y: f32) -> Option<&DragTarget> {
        self.drags.iter().rev().find(|d| {
            let r = d.rect;
            x >= r.left() && x <= r.right() && y >= r.top() && y <= r.bottom()
        })
    }

    /// Forces the next [`update`](Self::update) to render (e.g. after the
    /// texture holding the last render was lost).
    pub fn invalidate(&mut self) {
        self.last_inputs.clear();
        self.last_px = (0, 0);
    }

    /// Re-renders if any bound input or the target pixel size changed.
    /// Returns true when the pixmap was updated.
    pub fn update(&mut self, values: &Values, px_w: u32, px_h: u32) -> bool {
        let inputs: Vec<f32> = self.ops.iter().map(|o| values.get(&o.source)).collect();
        if self.pixmap.is_some() && inputs == self.last_inputs && (px_w, px_h) == self.last_px {
            return false;
        }

        let t0 = Instant::now();
        let patched = self.patched_source(&inputs);
        let t1 = Instant::now();
        let tree = match usvg::Tree::from_str(&patched, &self.options) {
            Ok(t) => t,
            Err(e) => {
                self.warnings.push(format!("patched SVG failed to parse: {e}"));
                self.ops.clear();
                return false;
            }
        };
        let t2 = Instant::now();
        for d in &mut self.drags {
            if let Some(r) = tree.node_by_id(&d.id).and_then(|n| n.abs_bounding_box()) {
                d.rect = r;
            }
        }

        let scale = (px_w as f32 / self.size.width()).min(px_h as f32 / self.size.height());
        let w = ((self.size.width() * scale).round() as u32).max(1);
        let h = ((self.size.height() * scale).round() as u32).max(1);
        let pixmap = match &mut self.pixmap {
            Some(p) if p.width() == w && p.height() == h => {
                p.fill(tiny_skia::Color::TRANSPARENT);
                p
            }
            slot => slot.insert(Pixmap::new(w, h).unwrap()),
        };
        resvg::render(
            &tree,
            tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
        let t3 = Instant::now();

        let ema = |old: f32, new: std::time::Duration| {
            let new = new.as_secs_f32() * 1e6;
            if old == 0.0 { new } else { old * 0.9 + new * 0.1 }
        };
        self.stats.patch_us = ema(self.stats.patch_us, t1 - t0);
        self.stats.parse_us = ema(self.stats.parse_us, t2 - t1);
        self.stats.render_us = ema(self.stats.render_us, t3 - t2);
        self.stats.renders += 1;
        self.last_inputs = inputs;
        self.last_px = (px_w, px_h);
        true
    }

    fn patched_source(&self, inputs: &[f32]) -> String {
        let mut patches: Vec<ElementPatch> =
            self.elements.iter().map(|_| ElementPatch::default()).collect();

        for (o, &v) in self.ops.iter().zip(inputs) {
            let p = &mut patches[o.element];
            let lerp = |a: f32, b: f32| a + (b - a) * v;
            match &o.op {
                Op::Rotate { from, to, pivot } => {
                    p.transform.push_str(&format!(
                        "rotate({} {} {}) ",
                        lerp(*from, *to),
                        pivot.0,
                        pivot.1
                    ));
                }
                Op::Translate { from, to } => {
                    p.transform.push_str(&format!(
                        "translate({} {}) ",
                        lerp(from[0], to[0]),
                        lerp(from[1], to[1])
                    ));
                }
                Op::Scale { from, to, axis, pivot } => {
                    let s = lerp(*from, *to).max(1e-4);
                    let (sx, sy) = match axis {
                        Axis::Both => (s, s),
                        Axis::X => (s, 1.0),
                        Axis::Y => (1.0, s),
                    };
                    p.transform.push_str(&format!(
                        "translate({px} {py}) scale({sx} {sy}) translate({} {}) ",
                        -pivot.0,
                        -pivot.1,
                        px = pivot.0,
                        py = pivot.1
                    ));
                }
                Op::Opacity { from, to } => {
                    p.style.push_str(&format!(";opacity:{}", lerp(*from, *to).clamp(0.0, 1.0)));
                }
                Op::Fill { from, to } => {
                    let c = |i: usize| (lerp(from[i], to[i]).clamp(0.0, 255.0)) as u8;
                    p.style
                        .push_str(&format!(";fill:#{:02x}{:02x}{:02x}", c(0), c(1), c(2)));
                }
                Op::TextParam(spec) => p.text = Some(escape(&spec.display(v))),
                Op::TextPercent => p.text = Some(format!("{:.0}%", v * 100.0)),
                Op::TextFormat(f) => p.text = Some(escape(&patch::format_value(f, v))),
            }
        }

        // Collect edits as (range, replacement) and splice in source order.
        let mut edits: Vec<(Range<usize>, String)> = Vec::new();
        for (slots, p) in self.elements.iter().zip(patches) {
            if !p.transform.is_empty() {
                match &slots.transform {
                    Some(r) => edits.push((
                        r.clone(),
                        format!("{}{}", p.transform, &self.source[r.clone()]),
                    )),
                    None => edits.push((
                        slots.insert_at..slots.insert_at,
                        format!(" transform=\"{}\"", p.transform.trim_end()),
                    )),
                }
            }
            if !p.style.is_empty() {
                match &slots.style {
                    Some(r) => {
                        edits.push((r.clone(), format!("{}{}", &self.source[r.clone()], p.style)))
                    }
                    None => edits.push((
                        slots.insert_at..slots.insert_at,
                        format!(" style=\"{}\"", &p.style[1..]),
                    )),
                }
            }
            if let (Some(t), Some(r)) = (p.text, &slots.text) {
                edits.push((r.clone(), t));
            }
        }
        edits.sort_by_key(|(r, _)| (r.start, r.end));

        let mut out = String::with_capacity(self.source.len() + 256);
        let mut pos = 0;
        for (r, s) in edits {
            out.push_str(&self.source[pos..r.start]);
            out.push_str(&s);
            pos = r.end;
        }
        out.push_str(&self.source[pos..]);
        out
    }
}

fn slots_for(source: &str, node: roxmltree::Node) -> ElementSlots {
    let start = node.range().start;
    // `<` + qualified tag name.
    let name_len = source[start + 1..]
        .find(|c: char| c.is_whitespace() || c == '/' || c == '>')
        .unwrap_or(0);
    let attr = |name: &str| {
        node.attributes()
            .find(|a| a.name() == name && a.namespace().is_none())
            .map(|a| a.range_value())
    };
    ElementSlots {
        insert_at: start + 1 + name_len,
        transform: attr("transform"),
        style: attr("style"),
        text: node
            .descendants()
            .find(|n| n.is_text() && !n.text().unwrap_or("").trim().is_empty())
            .map(|n| n.range()),
    }
}

fn parse_color(s: &str) -> [f32; 3] {
    let hex = s.trim_start_matches('#');
    let hex: String = if hex.len() == 3 {
        hex.chars().flat_map(|c| [c, c]).collect()
    } else {
        hex.to_string()
    };
    let ch = |i: usize| u8::from_str_radix(hex.get(i..i + 2).unwrap_or("ff"), 16).unwrap_or(255) as f32;
    [ch(0), ch(2), ch(4)]
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Useful for debugging: what the theme CSS for a patch looks like.
pub fn debug_theme_css(theme: &Theme) -> String {
    theme_css(theme)
}

#[allow(dead_code)]
fn _assert_send_sync() {
    fn is<T: Send + Sync>() {}
    is::<SvgControlPanel>();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patches() -> PathBuf {
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../patches"))
    }

    fn values_for(patch: &Patch) -> Values {
        let mut v = Values::default();
        for i in 0..patch.macros.len() {
            v.macros[i] = patch.macro_default(i);
        }
        let macros = v.macros;
        patch.compute_params(&macros, &patch.default_param_values(), &mut v.params);
        v
    }

    #[test]
    fn drag_area_follows_translated_element() {
        let p = Patch::load(patches().join("clean-gain")).unwrap();
        let mut panel = SvgControlPanel::load(&p).unwrap();
        assert!(panel.warnings.is_empty(), "{:?}", panel.warnings);
        let mut v = values_for(&p);
        v.macros[0] = 1.0; // fader at top: translate(0 -100)
        let macros = v.macros;
        p.compute_params(&macros, &p.default_param_values(), &mut v.params);
        assert!(panel.update(&v, 400, 300));
        let top = panel.drags[0].rect.top();
        assert!((top - 37.0).abs() < 2.0, "cap top at {top}");
        assert!(panel.drag_target_at(150.0, 50.0).is_some());
        assert!(panel.drag_target_at(150.0, 150.0).is_none());
        // unchanged inputs → no re-render
        assert!(!panel.update(&v, 400, 300));
    }

    #[test]
    fn composes_with_existing_transform_and_style() {
        let dir = std::env::temp_dir().join(format!("ui-svg-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("patch.lua"),
            r#"return { name = "T", params = { { id = "a", name = "A" } }, control_panel = { svg = "p.svg", bindings = {
                rotate("n", { source = "param.a", from = 0, to = 90, pivot = { 50, 50 } }),
                opacity("n", { source = "param.a" }),
                text("t", { source = "param.a" }),
                opacity("missing", { source = "param.a" }),
            } } }"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("p.svg"),
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
                 <rect id="n" x="40" y="10" width="20" height="40" transform="translate(1 1)" style="fill:red"/>
                 <text id="t" x="10" y="90">x</text></svg>"#,
        )
        .unwrap();
        let p = Patch::load(&dir).unwrap();
        let mut panel = SvgControlPanel::load(&p).unwrap();
        assert_eq!(panel.warnings.len(), 1, "{:?}", panel.warnings);
        let v = Values { params: vec![0.5], ..Default::default() };
        let src = panel.patched_source(&[0.5, 0.5, 0.5]);
        assert!(src.contains(r#"transform="rotate(45 50 50) translate(1 1)""#), "{src}");
        assert!(src.contains(r#"style="fill:red;opacity:0.5""#), "{src}");
        assert!(src.contains(">0.50</text>"), "{src}");
        assert!(panel.update(&v, 100, 100));
        std::fs::remove_dir_all(&dir).ok();
    }
}
