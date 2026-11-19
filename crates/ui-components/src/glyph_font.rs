//! Fonts made of pictures: SVG fonts (SVG 1.1 `<font>`, glyphs are paths or any SVG shapes) and
//! BMFont image fonts (AngelCode text `.fnt` + PNG atlas). For display lettering (titles, labels,
//! values): one line of text, advances + kerning, no shaping. Rendered with resvg / tiny-skia.
//! See docs/research/patch-fonts.md.
//!
//! Sandbox: files only from inside the patch folder, size and glyph limits, SVG glyph content
//! goes through usvg like every other SVG of a patch (no scripts, no external references).

use std::collections::HashMap;
use std::path::Path;

use resvg::{tiny_skia, usvg};

const MAX_FILE: u64 = 4 << 20;
const MAX_GLYPHS: usize = 2048;
const MAX_TEXT: usize = 200;

pub struct GlyphFont {
    kind: Kind,
}

enum Kind {
    Svg(SvgFont),
    Bitmap(BmFont),
}

struct SvgFont {
    units_per_em: f32,
    ascent: f32,
    descent: f32,
    default_advance: f32,
    glyphs: HashMap<char, SvgGlyph>,
    missing: Option<SvgGlyph>,
    kerning: HashMap<(char, char), f32>,
}

#[derive(Clone)]
struct SvgGlyph {
    advance: Option<f32>,
    /// SVG markup in font units, y up (as in the font file).
    content: String,
}

struct BmFont {
    line_height: f32,
    atlas: tiny_skia::Pixmap,
    chars: HashMap<char, BmChar>,
    kerning: HashMap<(char, char), f32>,
}

#[derive(Clone, Copy)]
struct BmChar {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    xoffset: f32,
    yoffset: f32,
    advance: f32,
}

impl GlyphFont {
    /// `.svg` (SVG font) or `.fnt` (BMFont text format). `dir`: the patch folder.
    pub fn load(path: &Path, dir: &Path) -> Result<Self, String> {
        let read = |p: &Path| -> Result<Vec<u8>, String> {
            let len = std::fs::metadata(p).map_err(|e| format!("{}: {e}", p.display()))?.len();
            if len > MAX_FILE {
                return Err(format!("{}: larger than {} MB", p.display(), MAX_FILE >> 20));
            }
            std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))
        };
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let kind = match ext.as_str() {
            "svg" => Kind::Svg(parse_svg_font(&String::from_utf8_lossy(&read(path)?))?),
            "fnt" => {
                let text = String::from_utf8_lossy(&read(path)?).into_owned();
                let font_dir = path.parent().unwrap_or(dir);
                Kind::Bitmap(parse_bmfont(&text, |file| {
                    let rel = font_dir.strip_prefix(dir).unwrap_or(Path::new("")).join(file);
                    let page = patch::resolve_inside(dir, &rel.to_string_lossy()).map_err(|e| format!("page '{file}': {e}"))?;
                    tiny_skia::Pixmap::decode_png(&read(&page)?).map_err(|e| format!("page '{file}': {e}"))
                })?)
            }
            _ => return Err(format!("{}: not an SVG or BMFont (.fnt) font", path.display())),
        };
        Ok(Self { kind })
    }

    /// Renders one line of `text`. `size`: em size (SVG font) or line height (BMFont) in pixels.
    /// `color` tints SVG glyphs drawn in `currentColor`; BMFont images keep their colours.
    pub fn render(&self, text: &str, size: f32, color: tiny_skia::Color) -> Option<tiny_skia::Pixmap> {
        let text: String = text.chars().take(MAX_TEXT).collect();
        match &self.kind {
            Kind::Svg(f) => f.render(&text, size, color),
            Kind::Bitmap(f) => f.render(&text, size),
        }
    }
}

// ---- SVG fonts --------------------------------------------------------------------------------

fn parse_svg_font(src: &str) -> Result<SvgFont, String> {
    let doc = roxmltree::Document::parse(src).map_err(|e| format!("SVG font: {e}"))?;
    let font = doc.descendants().find(|n| n.has_tag_name("font")).ok_or("SVG font: no <font> element")?;
    let num = |n: roxmltree::Node, a: &str| n.attribute(a).and_then(|v| v.trim().parse::<f32>().ok());
    let face = font.children().find(|n| n.has_tag_name("font-face"));
    let units_per_em = face.and_then(|f| num(f, "units-per-em")).unwrap_or(1000.0).max(1.0);
    let ascent = face.and_then(|f| num(f, "ascent")).unwrap_or(units_per_em * 0.8);
    let descent = face.and_then(|f| num(f, "descent")).map(f32::abs).unwrap_or(units_per_em * 0.2);

    // A glyph is its `d` (filled in currentColor) or its child elements, copied as markup.
    let glyph = |n: roxmltree::Node| -> SvgGlyph {
        let mut content = String::new();
        if let Some(d) = n.attribute("d") {
            content.push_str(&format!(r#"<path d="{}"/>"#, xml_escape(d)));
        }
        for c in n.children().filter(|c| c.is_element()) {
            content.push_str(&src[c.range()]);
        }
        SvgGlyph { advance: num(n, "horiz-adv-x"), content }
    };
    let mut glyphs = HashMap::new();
    for n in font.children().filter(|n| n.has_tag_name("glyph")) {
        // Single characters only (no ligatures).
        let mut chars = n.attribute("unicode").unwrap_or("").chars();
        if let (Some(c), None) = (chars.next(), chars.next()) {
            glyphs.insert(c, glyph(n));
        }
        if glyphs.len() > MAX_GLYPHS {
            return Err(format!("SVG font: more than {MAX_GLYPHS} glyphs"));
        }
    }
    let mut kerning = HashMap::new();
    for n in font.children().filter(|n| n.has_tag_name("hkern")) {
        let (Some(u1), Some(u2), Some(k)) = (n.attribute("u1"), n.attribute("u2"), num(n, "k")) else { continue };
        for a in u1.split(',').filter_map(|s| single_char(s.trim())) {
            for b in u2.split(',').filter_map(|s| single_char(s.trim())) {
                kerning.insert((a, b), k);
            }
        }
    }
    Ok(SvgFont {
        units_per_em,
        ascent,
        descent,
        default_advance: num(font, "horiz-adv-x").unwrap_or(units_per_em * 0.5),
        missing: font.children().find(|n| n.has_tag_name("missing-glyph")).map(glyph),
        glyphs,
        kerning,
    })
}

fn single_char(s: &str) -> Option<char> {
    let mut c = s.chars();
    match (c.next(), c.next()) {
        (Some(ch), None) => Some(ch),
        _ => None,
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;")
}

impl SvgFont {
    fn render(&self, text: &str, size: f32, color: tiny_skia::Color) -> Option<tiny_skia::Pixmap> {
        let s = size / self.units_per_em;
        let mut x = 0.0;
        let mut body = String::new();
        let mut prev: Option<char> = None;
        for ch in text.chars() {
            let g = self.glyphs.get(&ch).or(self.missing.as_ref())?;
            if let Some(p) = prev {
                x -= self.kerning.get(&(p, ch)).copied().unwrap_or(0.0);
            }
            if !g.content.is_empty() {
                body.push_str(&format!(r#"<g transform="translate({:.3} {:.3}) scale({s:.5} {:.5})">{}</g>"#, x * s, self.ascent * s, -s, g.content));
            }
            x += g.advance.unwrap_or(self.default_advance);
            prev = Some(ch);
        }
        // Strokes can reach past the advance box: a little padding on all sides.
        let pad = (size * 0.1).ceil();
        let (w, h) = ((x * s + 2.0 * pad).ceil().max(1.0), ((self.ascent + self.descent) * s + 2.0 * pad).ceil());
        let c = color.to_color_u8();
        let doc = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}"><g transform="translate({pad} {pad})" color="rgb({},{},{})" fill="currentColor" opacity="{:.3}">{body}</g></svg>"#,
            c.red(),
            c.green(),
            c.blue(),
            c.alpha() as f32 / 255.0
        );
        let tree = usvg::Tree::from_str(&doc, &usvg::Options::default()).ok()?;
        let mut pm = tiny_skia::Pixmap::new(w as u32, h as u32)?;
        resvg::render(&tree, tiny_skia::Transform::default(), &mut pm.as_mut());
        Some(pm)
    }
}

// ---- BMFont -------------------------------------------------------------------------------------

/// The text format: lines of `tag key=value key="quoted value" …`.
fn parse_bmfont(src: &str, load_page: impl Fn(&str) -> Result<tiny_skia::Pixmap, String>) -> Result<BmFont, String> {
    let mut line_height = None;
    let mut page = None;
    let mut chars = HashMap::new();
    let mut kerning = HashMap::new();
    for line in src.lines() {
        let mut parts = split_fields(line);
        let Some(tag) = parts.next() else { continue };
        let kv: HashMap<String, String> = parts.filter_map(|p| p.split_once('=').map(|(k, v)| (k.to_string(), v.trim_matches('"').to_string()))).collect();
        let n = |k: &str| kv.get(k).and_then(|v| v.parse::<f32>().ok());
        match tag.as_str() {
            "common" => {
                line_height = n("lineHeight");
                if n("pages").unwrap_or(1.0) != 1.0 {
                    return Err("BMFont: only single-page fonts are supported".into());
                }
            }
            "page" if n("id") == Some(0.0) => page = kv.get("file").cloned(),
            "char" => {
                let (Some(id), Some(x), Some(y), Some(w), Some(h)) = (n("id"), n("x"), n("y"), n("width"), n("height")) else { continue };
                let Some(c) = char::from_u32(id as u32) else { continue };
                chars.insert(c, BmChar {
                    x: x as u32,
                    y: y as u32,
                    w: w as u32,
                    h: h as u32,
                    xoffset: n("xoffset").unwrap_or(0.0),
                    yoffset: n("yoffset").unwrap_or(0.0),
                    advance: n("xadvance").unwrap_or(w),
                });
                if chars.len() > MAX_GLYPHS {
                    return Err(format!("BMFont: more than {MAX_GLYPHS} chars"));
                }
            }
            "kerning" => {
                if let (Some(a), Some(b), Some(k)) = (n("first"), n("second"), n("amount")) {
                    if let (Some(a), Some(b)) = (char::from_u32(a as u32), char::from_u32(b as u32)) {
                        kerning.insert((a, b), k);
                    }
                }
            }
            _ => {}
        }
    }
    let line_height = line_height.filter(|h| *h > 0.0).ok_or("BMFont: no 'common lineHeight'")?;
    let atlas = load_page(&page.ok_or("BMFont: no page 0")?)?;
    // Every glyph rectangle must lie inside the atlas.
    if let Some((c, _)) = chars.iter().find(|(_, g)| g.x + g.w > atlas.width() || g.y + g.h > atlas.height()) {
        return Err(format!("BMFont: glyph '{c}' lies outside the atlas"));
    }
    Ok(BmFont { line_height, atlas, chars, kerning })
}

/// Splits on spaces outside quotes.
fn split_fields(line: &str) -> impl Iterator<Item = String> {
    let mut out = Vec::new();
    let (mut cur, mut quoted) = (String::new(), false);
    for c in line.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                cur.push(c);
            }
            ' ' | '\t' if !quoted => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out.into_iter()
}

impl BmFont {
    fn render(&self, text: &str, size: f32) -> Option<tiny_skia::Pixmap> {
        let s = size / self.line_height;
        // Layout first (width), then draw.
        let mut placed = Vec::new();
        let (mut x, mut right, mut prev) = (0.0f32, 0.0f32, None::<char>);
        for ch in text.chars() {
            let Some(g) = self.chars.get(&ch).or_else(|| self.chars.get(&'?')) else { continue };
            if let Some(p) = prev {
                x += self.kerning.get(&(p, ch)).copied().unwrap_or(0.0);
            }
            placed.push((*g, x));
            right = right.max(x + g.xoffset + g.w as f32);
            x += g.advance;
            prev = Some(ch);
        }
        let mut out = tiny_skia::Pixmap::new((right.max(x) * s).ceil().max(1.0) as u32, (self.line_height * s).ceil() as u32)?;
        for (g, x) in placed {
            if g.w == 0 || g.h == 0 {
                continue;
            }
            let region = self.atlas.clone_rect(tiny_skia::IntRect::from_xywh(g.x as i32, g.y as i32, g.w, g.h)?)?;
            let t = tiny_skia::Transform::from_scale(s, s).pre_translate(x + g.xoffset, g.yoffset);
            out.draw_pixmap(0, 0, region.as_ref(), &tiny_skia::PixmapPaint { quality: tiny_skia::FilterQuality::Bicubic, ..Default::default() }, t, None);
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    #[test]
    fn svg_font_renders_glyphs_with_kerning_and_colour() {
        let dir = repo().join("patches/clean-gain");
        let f = GlyphFont::load(&dir.join("fonts/clean-line.svg"), &dir).unwrap();
        let pm = f.render("Clean Gain", 40.0, tiny_skia::Color::from_rgba8(255, 0, 0, 255)).unwrap();
        assert!(pm.width() > 150 && pm.height() >= 40, "{}x{}", pm.width(), pm.height());
        // Something was drawn, in the requested colour.
        let red = pm.pixels().iter().filter(|p| p.alpha() == 255 && p.red() == 255 && p.green() == 0).count();
        assert!(red > 100, "{red} red pixels");
        // Kerning: "AV" is narrower than "A" + "V".
        let w = |t: &str| f.render(t, 40.0, tiny_skia::Color::BLACK).unwrap().width();
        assert!(w("AV") < w("AX"));
    }

    #[test]
    fn bmfont_renders_from_the_atlas() {
        let dir = repo().join("patches/junkyard-amp");
        let f = GlyphFont::load(&dir.join("fonts/label-tape.fnt"), &dir).unwrap();
        let pm = f.render("Junkyard Amp", 36.0, tiny_skia::Color::BLACK).unwrap();
        assert_eq!(pm.height(), 36);
        assert!(pm.width() > 200);
        assert!(pm.pixels().iter().filter(|p| p.alpha() > 200).count() > pm.width() as usize * 10);
    }

    #[test]
    fn fonts_stay_inside_the_patch_folder() {
        let dir = std::env::temp_dir().join(format!("glyph-font-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("fonts")).unwrap();
        std::fs::write(dir.join("fonts/x.fnt"), "common lineHeight=10 pages=1\npage id=0 file=\"../../outside.png\"\n").unwrap();
        let err = GlyphFont::load(&dir.join("fonts/x.fnt"), &dir).err().unwrap();
        assert!(err.contains("page"), "{err}");
        std::fs::write(dir.join("fonts/x.svg"), "<svg><text>no font</text></svg>").unwrap();
        assert!(GlyphFont::load(&dir.join("fonts/x.svg"), &dir).err().unwrap().contains("no <font>"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
