//! Reactive UI components: Lua `render(props)` → SVG → one cached image per component.
//!
//! The UI is the `panel` of the patch: `patch.luau` requires its UI files and
//! passes `Patch.new(..., { panel = Panel.new({ ... }) })`, wiring macros, params
//! and metered blocks into the components as props. Every ControlPanel has the
//! same logical size, [`WIDTH`] × [`HEIGHT`]; the shell scales it to the window.
//! Every frame the shell calls [`Ui::update`] with the current inputs (bound
//! parameter values, meters, hover/drag, time). Each component instance is
//! re-rendered only when its *inputs* changed: its props (as passed by its
//! parent), the value of its `bind`, the meter of its `source`, hover/drag (for
//! interactive kinds), its local `state`, and `time` (only if `animated`).
//! Rasterisation happens only when the produced SVG actually changed (or the
//! scale did). Children are separate layers, so a parent never re-renders
//! because a child's value changed.
//!
//! Lua runs in the patch sandbox (no I/O, memory cap, instruction budget per
//! call). A failing component shows an error box; the rest keeps working.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use mlua::{Function, Lua, Table, Value};
use resvg::usvg::{ImageHrefResolver, fontdb};
use resvg::{tiny_skia, usvg};

pub use tiny_skia::Pixmap;


/// Logical size of every ControlPanel (fixed, so all patches look alike in the shell).
pub const WIDTH: f32 = 840.0;
pub const HEIGHT: f32 = 560.0;

/// Bundled fonts only. Scanning system fonts took 7.5s cold on a dev machine
/// (unacceptable inside a DAW) and would make patches render differently on
/// every machine. Patches can ship extra fonts in `<patch>/fonts/`.
fn base_fonts() -> &'static fontdb::Database {
    static DB: OnceLock<fontdb::Database> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_font_data(include_bytes!("../assets/NotoSans-Regular.ttf").to_vec());
        db.load_font_data(include_bytes!("../assets/NotoSans-Bold.ttf").to_vec());
        db.set_sans_serif_family("Noto Sans");
        db.set_serif_family("Noto Sans");
        db.set_monospace_family("Noto Sans");
        db.set_cursive_family("Noto Sans");
        db.set_fantasy_family("Noto Sans");
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
const MAX_TREE_DEPTH: usize = 24;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

/// Value of a bound parameter/macro, provided by the shell.
#[derive(Debug, Clone, PartialEq)]
pub struct Bound {
    /// Normalized 0..1.
    pub value: f32,
    pub display: String,
    /// Driven by a macro (not directly editable).
    pub locked: bool,
}

pub struct Inputs<'a> {
    pub time: f32,
    pub hover: Option<u64>,
    pub active: Option<u64>,
    pub bound: &'a dyn Fn(&str) -> Option<Bound>,
    pub meter: &'a dyn Fn(&str) -> Option<f32>,
    /// Arbitrary numeric series (e.g. a spectrum), by name.
    pub series: &'a dyn Fn(&str) -> Option<Vec<f32>>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct FrameStats {
    pub nodes: usize,
    pub renders: usize,
    pub rasters: usize,
    pub raster_px: usize,
    pub lua_us: f32,
    pub parse_us: f32,
    pub raster_us: f32,
    pub total_us: f32,
}

pub struct Layer<'a> {
    pub id: u64,
    /// Where to draw `pixmap` (logical units): the component's content area,
    /// which may be smaller than the component itself.
    pub rect: Rect,
    pub pixmap: Option<&'a Pixmap>,
    /// The pixmap was (re)rendered during the last update.
    pub changed: bool,
    pub kind: Option<&'a str>,
    pub error: Option<&'a str>,
    /// Component name and duration of its last rasterisation (profiling).
    pub name: String,
    pub raster_us: f32,
}

/// What the shell needs to know about an interactive component.
#[derive(Debug, Clone)]
pub struct Hit {
    pub id: u64,
    pub kind: String,
    pub bind: Option<String>,
    pub rect: Rect,
}

struct Node {
    id: u64,
    key: String,
    def: Table,
    props: Table,
    props_fp: u64,
    state: Table,
    kind: Option<String>,
    bind: Option<String>,
    source: Option<String>,
    series: Option<String>,
    animated: bool,
    local: Rect,
    abs: Rect,
    input_fp: Option<u64>,
    svg: String,
    svg_dirty: bool,
    pixmap: Option<Pixmap>,
    raster_scale: f32,
    changed: bool,
    error: Option<String>,
    children: Vec<Node>,
    raster_us: f32,
    /// Area of the component that actually has content (logical, relative to
    /// the component). Only this area is rasterised.
    content: Rect,
}

pub struct Ui {
    rt: patch::runtime::Runtime,
    lua: Lua,
    options: usvg::Options<'static>,
    root: Option<Node>,
    next_id: u64,
    pub stats: FrameStats,
}

fn fingerprint_into(v: &Value, h: &mut DefaultHasher, depth: usize) {
    match v {
        Value::Nil => 0u8.hash(h),
        Value::Boolean(b) => (1u8, *b).hash(h),
        Value::Integer(i) => (2u8, *i).hash(h),
        Value::Number(n) => (3u8, n.to_bits()).hash(h),
        Value::String(s) => (4u8, s.as_bytes().as_ref()).hash(h),
        Value::Table(t) if depth < 8 => {
            5u8.hash(h);
            let mut entries: Vec<(String, Value)> = t
                .pairs::<Value, Value>()
                .filter_map(Result::ok)
                .map(|(k, v)| (format!("{k:?}"), v))
                .collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            for (k, v) in entries {
                k.hash(h);
                fingerprint_into(&v, h, depth + 1);
            }
        }
        other => (6u8, other.to_pointer() as usize).hash(h),
    }
}

fn fingerprint(v: &Value) -> u64 {
    let mut h = DefaultHasher::new();
    fingerprint_into(v, &mut h, 0);
    h.finish()
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn error_svg(w: f32, h: f32, msg: &str) -> String {
    let short: String = msg.lines().next().unwrap_or("").chars().take(90).collect();
    format!(
        r##"<rect x="1" y="1" width="{}" height="{}" fill="#3a1416" stroke="#e5534b" stroke-width="2" rx="4"/><text x="8" y="18" font-size="11" fill="#ffb3ad" font-family="Noto Sans">{}</text>"##,
        (w - 2.0).max(1.0),
        (h - 2.0).max(1.0),
        esc(&short)
    )
}

impl Ui {
    /// Loads `<dir>/ui.lua`.
    /// Loads the patch in `dir` and builds its `panel`.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let rt = patch::runtime::Runtime::new(dir).map_err(|e| e.to_string())?;
        let lua = rt.lua.clone();
        let options = usvg::Options {
            resources_dir: Some(dir.to_path_buf()),
            fontdb: fonts_for(dir),
            image_href_resolver: sandboxed_image_resolver(dir.to_path_buf()),
            font_family: "Noto Sans".into(),
            ..Default::default()
        };
        let mut ui = Ui {
            rt,
            lua,
            options,
            root: None,
            next_id: 1,
            stats: FrameStats::default(),
        };
        ui.reload()?;
        Ok(ui)
    }

    /// Re-evaluates `ui.lua`. Instances are matched by key, so local state
    /// (e.g. the selected tab) survives; everything re-renders once.
    pub fn reload(&mut self) -> Result<(), String> {
        let _c = patch::locale::CNumeric::enter();
        let patch = self.rt.eval_patch().map_err(|e| e.to_string())?;
        let root: Value = patch.get("panel").map_err(|e| e.to_string())?;
        if root.is_nil() {
            return Err("the patch has no panel: pass `panel = <Root>.new({ ... })` to Patch.new".into());
        }
        let (def, props) =
            descriptor(&root).ok_or("`panel` must be a component instance (<Root>.new({ ... }))")?;
        let frame = Rect { x: 0.0, y: 0.0, w: WIDTH, h: HEIGHT };
        let root = match self.root.take() {
            Some(mut old) => {
                old.def = def;
                old.input_fp = None;
                self.set_props(&mut old, props, frame);
                old
            }
            None => self.new_node("root".into(), def, props, frame),
        };
        self.root = Some(root);
        Ok(())
    }

    fn new_node(&mut self, key: String, def: Table, props: Table, parent: Rect) -> Node {
        let id = self.next_id;
        self.next_id += 1;
        let mut n = Node {
            id,
            key,
            def,
            props: props.clone(),
            props_fp: 0,
            state: self.lua.create_table().expect("lua table"),
            kind: None,
            bind: None,
            source: None,
            series: None,
            animated: false,
            local: Rect::default(),
            abs: Rect::default(),
            input_fp: None,
            svg: String::new(),
            svg_dirty: false,
            pixmap: None,
            raster_scale: 0.0,
            changed: false,
            error: None,
            children: Vec::new(),
            raster_us: 0.0,
            content: Rect::default(),
        };
        self.set_props(&mut n, props, parent);
        n
    }

    fn set_props(&self, n: &mut Node, props: Table, parent: Rect) {
        n.props_fp = fingerprint(&Value::Table(props.clone()));
        let num = |k: &str| props.get::<Option<f32>>(k).ok().flatten();
        let size: Option<Table> = n.def.get("size").ok();
        let size_at = |i: usize| size.as_ref().and_then(|t| t.get::<Option<f32>>(i).ok().flatten());
        n.local = Rect {
            x: num("x").unwrap_or(0.0),
            y: num("y").unwrap_or(0.0),
            w: num("w").or(size_at(1)).unwrap_or(parent.w),
            h: num("h").or(size_at(2)).unwrap_or(parent.h),
        };
        n.kind = n.def.get::<Option<String>>("kind").ok().flatten();
        n.bind = reference(&props, "bind");
        n.source = reference(&props, "source");
        n.series = props.get::<Option<String>>("series").ok().flatten();
        n.animated = n.def.get::<Option<bool>>("animated").ok().flatten().unwrap_or(false)
            || props.get::<Option<bool>>("animated").ok().flatten().unwrap_or(false);
        n.props = props;
    }

    /// Renders/rasterises what changed. `scale` = physical pixels per logical unit.
    pub fn update(&mut self, inputs: &Inputs, scale: f32) {
        // All Lua (render functions) runs inside; see patch::locale.
        let _c = patch::locale::CNumeric::enter();
        let t0 = Instant::now();
        self.stats = FrameStats::default();
        if let Some(mut root) = self.root.take() {
            let frame = Rect { x: 0.0, y: 0.0, w: WIDTH, h: HEIGHT };
            self.update_node(&mut root, frame, Value::Nil, inputs, scale, 0);
            self.root = Some(root);
        }
        self.stats.total_us = t0.elapsed().as_secs_f32() * 1e6;
    }

    fn update_node(&mut self, n: &mut Node, parent: Rect, theme: Value, inputs: &Inputs, scale: f32, depth: usize) {
        self.stats.nodes += 1;
        n.changed = false;
        n.abs = Rect { x: parent.x + n.local.x, y: parent.y + n.local.y, w: n.local.w, h: n.local.h };

        let bound = n.bind.as_deref().and_then(|b| (inputs.bound)(b));
        let interactive = n.kind.is_some();
        let hover = interactive && inputs.hover == Some(n.id);
        let active = interactive && inputs.active == Some(n.id);
        let level = n.source.as_deref().and_then(|s| (inputs.meter)(s));
        let series = n.series.as_deref().and_then(|s| (inputs.series)(s));
        let time = n.animated.then_some(inputs.time);

        let mut h = DefaultHasher::new();
        n.props_fp.hash(&mut h);
        if let Some(b) = &bound {
            (b.value.to_bits(), &b.display, b.locked).hash(&mut h);
        }
        (hover, active).hash(&mut h);
        level.map(|l| (l * 2000.0).round() as i32).hash(&mut h);
        if let Some(s) = &series {
            for v in s {
                ((v * 2000.0).round() as i32).hash(&mut h);
            }
        }
        time.map(f32::to_bits).hash(&mut h);
        fingerprint(&Value::Table(n.state.clone())).hash(&mut h);
        fingerprint(&theme).hash(&mut h);
        let fp = h.finish();

        if n.input_fp != Some(fp) {
            n.input_fp = Some(fp);
            self.render_node(n, bound, hover, active, level, series, time, &theme, depth);
        }
        if n.svg_dirty || (n.pixmap.is_some() && n.raster_scale != scale) {
            self.rasterize(n, scale);
        }

        let child_theme = match n.props.get::<Value>("theme") {
            Ok(Value::Nil) | Err(_) => theme,
            Ok(t) => t,
        };
        let abs = n.abs;
        for c in n.children.iter_mut() {
            self.update_node(c, abs, child_theme.clone(), inputs, scale, depth + 1);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_node(
        &mut self,
        n: &mut Node,
        bound: Option<Bound>,
        hover: bool,
        active: bool,
        level: Option<f32>,
        series: Option<Vec<f32>>,
        time: Option<f32>,
        theme: &Value,
        depth: usize,
    ) {
        let result = (|| -> mlua::Result<(String, Vec<(Table, Table)>)> {
            let p = self.lua.create_table()?;
            for pair in n.props.pairs::<Value, Value>() {
                let (k, v) = pair?;
                p.raw_set(k, v)?;
            }
            p.set("width", n.local.w)?;
            p.set("height", n.local.h)?;
            if let Some(b) = &bound {
                p.set("value", b.value)?;
                p.set("display", b.display.as_str())?;
                p.set("locked", b.locked)?;
            }
            p.set("hover", hover)?;
            p.set("dragging", active)?;
            if let Some(l) = level {
                p.set("level", l)?;
            }
            if let Some(s) = series {
                p.set("series", self.lua.create_sequence_from(s)?)?;
            }
            if let Some(t) = time {
                p.set("time", t)?;
            }
            p.set("state", n.state.clone())?;
            if p.get::<Value>("theme")?.is_nil() {
                p.set("theme", theme.clone())?;
            }
            let render: Function = n.def.get("render")?;
            self.rt.begin_call();
            let t = Instant::now();
            let out: Value = render.call(p)?;
            self.stats.lua_us += t.elapsed().as_secs_f32() * 1e6;
            let mut svg = String::new();
            let mut children = Vec::new();
            collect(&out, &mut svg, &mut children, 0)?;
            Ok((svg, children))
        })();
        self.stats.renders += 1;

        match result {
            Ok((svg, descs)) if depth < MAX_TREE_DEPTH => {
                n.error = None;
                if svg != n.svg {
                    n.svg = svg;
                    n.svg_dirty = true;
                }
                self.reconcile(n, descs);
            }
            Ok(_) => self.fail(n, "component tree too deep".into()),
            Err(e) => self.fail(n, e.to_string()),
        }
    }

    fn fail(&mut self, n: &mut Node, msg: String) {
        // A component that hit the memory cap leaves its garbage behind; collect it
        // now so the next component doesn't fail too.
        if msg.contains("memory") {
            let _ = self.lua.gc_collect();
            let _ = self.lua.gc_collect();
        }
        let first = msg.lines().next().unwrap_or("").to_string();
        n.svg = error_svg(n.local.w, n.local.h, &first);
        n.svg_dirty = true;
        n.error = Some(first);
        n.children.clear();
    }

    fn reconcile(&mut self, n: &mut Node, descs: Vec<(Table, Table)>) {
        let mut old: HashMap<String, Node> = n.children.drain(..).map(|c| (c.key.clone(), c)).collect();
        let mut counters: HashMap<String, usize> = HashMap::new();
        let mut next = Vec::with_capacity(descs.len());
        for (def, props) in descs {
            let name = def.get::<Option<String>>("name").ok().flatten().unwrap_or_else(|| "c".into());
            let key = match props.get::<Value>("key") {
                Ok(Value::String(s)) => s.to_string_lossy(),
                Ok(Value::Integer(i)) => i.to_string(),
                _ => {
                    let c = counters.entry(name.clone()).or_insert(0);
                    *c += 1;
                    format!("{name}#{c}")
                }
            };
            let full = format!("{}/{}", n.key, key);
            let child = match old.remove(&full) {
                Some(mut c) => {
                    if c.def.to_pointer() != def.to_pointer() {
                        c.def = def;
                        c.input_fp = None;
                    }
                    self.set_props(&mut c, props, n.local);
                    c
                }
                None => self.new_node(full, def, props, n.local),
            };
            next.push(child);
        }
        n.children = next;
    }

    fn rasterize(&mut self, n: &mut Node, scale: f32) {
        n.svg_dirty = false;
        if n.svg.is_empty() {
            n.pixmap = None;
            n.changed = true;
            return;
        }
        let (w, h) = (n.local.w.max(1.0), n.local.h.max(1.0));
        let doc = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">{}</svg>"#,
            n.svg
        );
        let t0 = Instant::now();
        let tree = match usvg::Tree::from_str(&doc, &self.options) {
            Ok(t) => t,
            Err(e) => {
                if n.error.is_none() {
                    self.fail(n, format!("invalid SVG: {e}"));
                    return self.rasterize(n, scale);
                }
                return;
            }
        };
        let t1 = Instant::now();
        // Rasterise only the area that has content (incl. strokes/filters), snapped
        // to whole device pixels and clipped to the component.
        let bbox = tree.root().abs_layer_bounding_box();
        let (cx0, cy0, cx1, cy1) = match bbox {
            Some(b) => (
                ((b.left().max(0.0) * scale).floor() / scale),
                ((b.top().max(0.0) * scale).floor() / scale),
                ((b.right().min(w) * scale).ceil() / scale),
                ((b.bottom().min(h) * scale).ceil() / scale),
            ),
            None => (0.0, 0.0, 0.0, 0.0),
        };
        if cx1 <= cx0 || cy1 <= cy0 {
            n.pixmap = None;
            n.changed = true;
            return;
        }
        n.content = Rect { x: cx0, y: cy0, w: cx1 - cx0, h: cy1 - cy0 };
        let pw = ((cx1 - cx0) * scale).round().max(1.0) as u32;
        let ph = ((cy1 - cy0) * scale).round().max(1.0) as u32;
        let mut pm = match &mut n.pixmap {
            Some(p) if p.width() == pw && p.height() == ph => {
                let mut p = p.clone();
                p.fill(tiny_skia::Color::TRANSPARENT);
                p
            }
            _ => Pixmap::new(pw, ph).expect("pixmap"),
        };
        resvg::render(
            &tree,
            tiny_skia::Transform::from_row(scale, 0.0, 0.0, scale, -cx0 * scale, -cy0 * scale),
            &mut pm.as_mut(),
        );
        let t2 = Instant::now();
        self.stats.parse_us += (t1 - t0).as_secs_f32() * 1e6;
        self.stats.raster_us += (t2 - t1).as_secs_f32() * 1e6;
        self.stats.rasters += 1;
        self.stats.raster_px += (pw * ph) as usize;
        n.raster_us = (t2 - t0).as_secs_f32() * 1e6;
        n.pixmap = Some(pm);
        n.raster_scale = scale;
        n.changed = true;
    }

    /// All layers in draw order (parents before their children).
    pub fn layers(&self) -> Vec<Layer<'_>> {
        fn walk<'a>(n: &'a Node, out: &mut Vec<Layer<'a>>) {
            out.push(Layer {
                id: n.id,
                rect: Rect { x: n.abs.x + n.content.x, y: n.abs.y + n.content.y, w: n.content.w, h: n.content.h },
                pixmap: n.pixmap.as_ref(),
                changed: n.changed,
                kind: n.kind.as_deref(),
                error: n.error.as_deref(),
                name: n.def.get::<Option<String>>("name").ok().flatten().unwrap_or_default(),
                raster_us: n.raster_us,
            });
            for c in &n.children {
                walk(c, out);
            }
        }
        let mut out = Vec::new();
        if let Some(r) = &self.root {
            walk(r, &mut out);
        }
        out
    }

    /// Topmost interactive component at (x, y) in logical units.
    pub fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        fn find(n: &Node, x: f32, y: f32) -> Option<Hit> {
            for c in n.children.iter().rev() {
                if let Some(h) = find(c, x, y) {
                    return Some(h);
                }
            }
            match &n.kind {
                Some(k) if n.abs.contains(x, y) => {
                    Some(Hit { id: n.id, kind: k.clone(), bind: n.bind.clone(), rect: n.abs })
                }
                _ => None,
            }
        }
        self.root.as_ref().and_then(|r| find(r, x, y))
    }

    /// Calls the component's `on_click(state, props)` (e.g. tabs, switches with local state).
    pub fn click(&mut self, id: u64) -> Result<(), String> {
        fn find(n: &mut Node, id: u64) -> Option<&mut Node> {
            if n.id == id {
                return Some(n);
            }
            n.children.iter_mut().find_map(|c| find(c, id))
        }
        let Some(root) = self.root.as_mut() else { return Ok(()) };
        let Some(n) = find(root, id) else { return Ok(()) };
        let handler: Option<Function> = n.def.get("on_click").ok().flatten();
        if let Some(f) = handler {
            let _c = patch::locale::CNumeric::enter();
            self.rt.begin_call();
            if let Err(e) = f.call::<()>((n.state.clone(), n.props.clone())) {
                let msg = e.to_string();
                n.error = Some(msg.lines().next().unwrap_or("").to_string());
                return Err(msg);
            }
        }
        Ok(())
    }

    /// The SVG each component produced last (debugging, tests).
    pub fn layer_svgs(&self) -> Vec<(String, String)> {
        fn walk(n: &Node, out: &mut Vec<(String, String)>) {
            out.push((n.key.clone(), n.svg.clone()));
            for c in &n.children {
                walk(c, out);
            }
        }
        let mut out = Vec::new();
        if let Some(r) = &self.root {
            walk(r, &mut out);
        }
        out
    }

    /// Errors of all components (for the shell's error display).
    pub fn errors(&self) -> Vec<String> {
        self.layers().iter().filter_map(|l| l.error.map(String::from)).collect()
    }
}

/// What a `bind` / `source` prop points at, as a key like "macro.tape": the
/// `__ref` of a Param/Macro/metered block/`Meters.*` (checked by `X.new`).
fn reference(props: &Table, field: &str) -> Option<String> {
    match props.get::<Value>(field).ok()? {
        Value::Table(t) => t.get::<Option<String>>("__ref").ok().flatten(),
        _ => None,
    }
}

fn descriptor(v: &Value) -> Option<(Table, Table)> {
    let Value::Table(t) = v else { return None };
    let def: Table = t.get::<Option<Table>>("__component").ok().flatten()?;
    let props: Table = t.get::<Option<Table>>("props").ok().flatten()?;
    Some((def, props))
}

fn collect(v: &Value, svg: &mut String, children: &mut Vec<(Table, Table)>, depth: usize) -> mlua::Result<()> {
    if depth > 64 {
        return Err(mlua::Error::runtime("render output nested too deeply"));
    }
    match v {
        Value::String(s) => svg.push_str(&s.to_str()?),
        Value::Table(_) => {
            if let Some(d) = descriptor(v) {
                children.push(d);
            } else if let Value::Table(t) = v {
                for item in t.sequence_values::<Value>() {
                    collect(&item?, svg, children, depth + 1)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}
