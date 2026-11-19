//! A patch's branding in the shell: the library card's title font and logo (`theme.library.font`
//! / `.logo`, else `theme.fonts.title` / `theme.images.logo`), files inside the patch folder
//! (paths validated by `Patch::load`).
//!
//! Isolation: iced keeps fonts for the whole process and finds them by family name, so a patch
//! font named "Inter" would restyle the shell (in every ScriptFX instance in the host). Every
//! patch font is therefore loaded under a private family name (`SFX <hash> <family>`, the hash
//! from the patch folder), written into a copy of the font's `name` table. Patches can't reach
//! the shell's fonts or each other's.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use nice_plug_iced::iced::{
    Color, Font,
    font::{Family, Stretch, Style, Weight},
    widget::{image, svg},
};

#[derive(Clone)]
pub enum Logo {
    Svg(svg::Handle),
    Image(image::Handle),
}

#[derive(Clone)]
pub struct Brand {
    /// A TTF/OTF title font (text rendered by iced).
    pub font: Option<Font>,
    /// The title rendered in an SVG or BMFont font: image + logical size.
    pub title: Option<(image::Handle, f32, f32)>,
    pub logo: Option<(Logo, f32)>,
}

/// Title size on the library card (logical px): SVG font em size, BMFont line height.
const TITLE_EM: f32 = 15.0;
const TITLE_LINE: f32 = 20.0;
/// Rendered at 2× so it stays crisp on HiDPI.
const OVERSAMPLE: f32 = 2.0;

/// Fonts already handed to iced, by private family name (iced keeps fonts for the process;
/// names are leaked once per font version because `Font` wants `&'static str`).
static FONTS: Mutex<Option<HashMap<String, Font>>> = Mutex::new(None);

/// Loads the patch's font and logo. TTF/OTF files iced hasn't seen yet are pushed to
/// `new_fonts` (the shell loads them into iced with a `Task`); SVG/BMFont fonts render the
/// title (`name`) into an image, SVG glyphs in `accent`.
pub fn load(dir: &Path, theme: &patch::Theme, name: &str, accent: Color, new_fonts: &mut Vec<Vec<u8>>) -> Brand {
    let resolve = |rel: Option<&str>| rel.and_then(|rel| patch::resolve_inside(dir, rel).ok());
    let mut brand = Brand { font: None, title: None, logo: resolve(theme.logo()).and_then(|p| logo(&p)) };
    if let Some(path) = resolve(theme.title_font()) {
        match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
            Some("svg" | "fnt") => brand.title = glyph_title(&path, dir, name, accent),
            _ => brand.font = font(&path, new_fonts),
        }
    }
    brand
}

fn glyph_title(path: &Path, dir: &Path, name: &str, accent: Color) -> Option<(image::Handle, f32, f32)> {
    let font = ui_components::glyph_font::GlyphFont::load(path, dir).map_err(|e| nice_plug::nice_log!("{e}")).ok()?;
    let size = if path.extension().is_some_and(|e| e == "fnt") { TITLE_LINE } else { TITLE_EM };
    let color = resvg::tiny_skia::Color::from_rgba(accent.r, accent.g, accent.b, accent.a)?;
    let pm = font.render(name, size * OVERSAMPLE, color)?;
    let (w, h) = (pm.width() as f32 / OVERSAMPLE, pm.height() as f32 / OVERSAMPLE);
    Some((image::Handle::from_rgba(pm.width(), pm.height(), super::stage::unpremultiply(pm.data())), w, h))
}

fn font(path: &Path, new_fonts: &mut Vec<Vec<u8>>) -> Option<Font> {
    let bytes = std::fs::read(path).ok()?;
    let face = ttf_parser::Face::parse(&bytes, 0).ok()?;
    let name = |id| face.names().into_iter().find(|n| n.name_id == id && n.is_unicode()).and_then(|n| n.to_string());
    let original = name(ttf_parser::name_id::TYPOGRAPHIC_FAMILY).or_else(|| name(ttf_parser::name_id::FAMILY))?;
    // Private per patch folder and per file version (an edited font gets a new name).
    let folder = path.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    let key = fnv(folder.as_bytes()) ^ fnv(&bytes).rotate_left(17);
    let family = format!("SFX {:08x} {original}", key as u32);
    let mut guard = FONTS.lock().unwrap();
    let cache = guard.get_or_insert_with(HashMap::new);
    if let Some(f) = cache.get(&family) {
        return Some(*f);
    }
    let weight = match face.weight().to_number() {
        0..=149 => Weight::Thin,
        150..=249 => Weight::ExtraLight,
        250..=349 => Weight::Light,
        350..=449 => Weight::Normal,
        450..=549 => Weight::Medium,
        550..=649 => Weight::Semibold,
        650..=749 => Weight::Bold,
        750..=849 => Weight::ExtraBold,
        _ => Weight::Black,
    };
    let style = if face.is_italic() { Style::Italic } else { Style::Normal };
    let renamed = private_family(&bytes, &family)?;
    let f = Font { family: Family::Name(Box::leak(family.clone().into_boxed_str())), weight, stretch: Stretch::Normal, style };
    cache.insert(family, f);
    new_fonts.push(renamed);
    Some(f)
}

fn fnv(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ *b as u64).wrapping_mul(0x0100_0000_01b3))
}

/// A copy of the font (TrueType/OpenType, not a collection) whose family names (name IDs 1, 4,
/// 6, 16) are replaced by `family`. All other tables are copied unchanged.
fn private_family(font: &[u8], family: &str) -> Option<Vec<u8>> {
    let u16_at = |b: &[u8], o: usize| Some(u16::from_be_bytes(b.get(o..o + 2)?.try_into().ok()?));
    let u32_at = |b: &[u8], o: usize| Some(u32::from_be_bytes(b.get(o..o + 4)?.try_into().ok()?));
    let version = u32_at(font, 0)?;
    if version != 0x0001_0000 && &font[0..4] != b"OTTO" && &font[0..4] != b"true" {
        return None; // collections and unknown containers
    }
    let num_tables = u16_at(font, 4)? as usize;
    let mut tables = Vec::with_capacity(num_tables);
    for i in 0..num_tables {
        let rec = 12 + i * 16;
        let tag: [u8; 4] = font.get(rec..rec + 4)?.try_into().ok()?;
        let (off, len) = (u32_at(font, rec + 8)? as usize, u32_at(font, rec + 12)? as usize);
        tables.push((tag, font.get(off..off + len)?.to_vec()));
    }

    // New `name` table: the original records minus the family ones, plus ours (Windows, UTF-16BE).
    let (_, name) = tables.iter_mut().find(|(t, _)| t == b"name")?;
    let count = u16_at(name, 2)? as usize;
    let storage = u16_at(name, 4)? as usize;
    let replaced = [1u16, 4, 6, 16];
    let mut records: Vec<([u16; 4], Vec<u8>)> = Vec::new();
    for i in 0..count {
        let r = 6 + i * 12;
        let ids = [u16_at(name, r)?, u16_at(name, r + 2)?, u16_at(name, r + 4)?, u16_at(name, r + 6)?];
        if replaced.contains(&ids[3]) {
            continue;
        }
        let (len, off) = (u16_at(name, r + 8)? as usize, u16_at(name, r + 10)? as usize);
        records.push((ids, name.get(storage + off..storage + off + len)?.to_vec()));
    }
    let utf16 = |s: &str| s.encode_utf16().flat_map(u16::to_be_bytes).collect::<Vec<u8>>();
    let postscript: String = family.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    for (id, value) in [(1u16, family.to_string()), (4, family.to_string()), (6, postscript), (16, family.to_string())] {
        records.push(([3, 1, 0x0409, id], utf16(&value)));
    }
    records.sort_by_key(|(ids, _)| *ids);
    let mut out: Vec<u8> = Vec::new();
    out.extend(0u16.to_be_bytes());
    out.extend((records.len() as u16).to_be_bytes());
    out.extend((6 + records.len() as u16 * 12).to_be_bytes());
    let mut strings: Vec<u8> = Vec::new();
    for (ids, data) in &records {
        for v in ids {
            out.extend(v.to_be_bytes());
        }
        out.extend((data.len() as u16).to_be_bytes());
        out.extend((strings.len() as u16).to_be_bytes());
        strings.extend(data);
    }
    out.extend(strings);
    *name = out;

    // Reassemble: table directory, then the tables (4-byte aligned) with fresh checksums.
    let checksum = |d: &[u8]| {
        d.chunks(4).fold(0u32, |s, c| {
            let mut w = [0u8; 4];
            w[..c.len()].copy_from_slice(c);
            s.wrapping_add(u32::from_be_bytes(w))
        })
    };
    let mut file = font[0..12].to_vec();
    let mut offset = 12 + tables.len() * 16;
    let mut body: Vec<u8> = Vec::new();
    for (tag, data) in &tables {
        file.extend(tag);
        file.extend(checksum(data).to_be_bytes());
        file.extend((offset as u32).to_be_bytes());
        file.extend((data.len() as u32).to_be_bytes());
        body.extend(data);
        while body.len() % 4 != 0 {
            body.push(0);
        }
        offset = 12 + tables.len() * 16 + body.len();
    }
    file.extend(body);
    Some(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_fonts_get_a_private_family_name() {
        let src = include_bytes!("../../../../patches/neon-saturator/fonts/Orbitron-ExtraBold.ttf");
        let out = private_family(src, "SFX 1234abcd Orbitron").unwrap();
        let face = ttf_parser::Face::parse(&out, 0).unwrap();
        let families: Vec<String> = face
            .names()
            .into_iter()
            .filter(|n| n.name_id == ttf_parser::name_id::FAMILY || n.name_id == ttf_parser::name_id::TYPOGRAPHIC_FAMILY)
            .filter_map(|n| n.to_string())
            .collect();
        assert!(!families.is_empty() && families.iter().all(|f| f == "SFX 1234abcd Orbitron"), "{families:?}");
        // Glyphs and metrics untouched.
        let orig = ttf_parser::Face::parse(src, 0).unwrap();
        assert_eq!(face.number_of_glyphs(), orig.number_of_glyphs());
        assert_eq!(face.glyph_index('N'), orig.glyph_index('N'));
        // A font that claims to be a shell font can't take its name.
        let mut db = resvg::usvg::fontdb::Database::new();
        db.load_font_data(out);
        assert!(db.faces().all(|f| f.families.iter().all(|(n, _)| n == "SFX 1234abcd Orbitron")));
    }
}

/// The logo and its aspect ratio (width / height).
fn logo(path: &Path) -> Option<(Logo, f32)> {
    let bytes = std::fs::read(path).ok()?;
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "svg" => {
            let tree = resvg::usvg::Tree::from_data(&bytes, &resvg::usvg::Options::default()).ok()?;
            let size = tree.size();
            Some((Logo::Svg(svg::Handle::from_memory(bytes)), size.width() / size.height()))
        }
        "png" => {
            let pm = resvg::tiny_skia::Pixmap::decode_png(&bytes).ok()?;
            let aspect = pm.width() as f32 / pm.height() as f32;
            let rgba = super::stage::unpremultiply(pm.data());
            Some((Logo::Image(image::Handle::from_rgba(pm.width(), pm.height(), rgba)), aspect))
        }
        _ => None,
    }
}
