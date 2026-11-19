//! Inline stroke icons (24-unit grid, round caps), tinted with the ink of their surface.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{LazyLock, Mutex};

use nice_plug_iced::iced::{
    Color, Element,
    widget::image,
};

pub const PLUS: &str = r#"<path d="M12 5v14M5 12h14"/>"#;
pub const MINUS: &str = r#"<path d="M5 12h14"/>"#;
pub const SUN: &str = r#"<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M2 12h2M20 12h2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/>"#;
pub const MOON: &str = r#"<path d="M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5z"/>"#;
/// Half-filled circle: "follow the system".
pub const AUTO: &str = r#"<circle cx="12" cy="12" r="8"/><path d="M12 4a8 8 0 0 1 0 16z" fill="black"/>"#;
pub const CHEVRON_RIGHT: &str = r#"<path d="M9 5l7 7-7 7"/>"#;
pub const CHEVRON_DOWN: &str = r#"<path d="M6 9l6 6 6-6"/>"#;
pub const SAVE: &str = r#"<path d="M5 3h11l3 3v15H5z"/><path d="M8 3v6h8V3M8 21v-7h8v7"/>"#;
pub const FOLDER: &str = r#"<path d="M3 6h6l2 2h10v11H3z"/>"#;
pub const RELOAD: &str = r#"<path d="M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7"/>"#;
pub const CHAT: &str = r#"<path d="M4 5h16v11H9l-5 4z"/>"#;
pub const SETTINGS: &str = r#"<circle cx="12" cy="12" r="3"/><path d="M12 2v3M12 19v3M2 12h3M19 12h3M4.9 4.9l2.1 2.1M17 17l2.1 2.1M4.9 19.1L7 17M17 7l2.1-2.1"/>"#;
pub const CLOSE: &str = r#"<path d="M6 6l12 12M18 6L6 18"/>"#;
pub const HEADPHONES: &str = r#"<path d="M4 15v-3a8 8 0 0 1 16 0v3"/><rect x="3" y="14" width="4" height="6" rx="1"/><rect x="17" y="14" width="4" height="6" rx="1"/>"#;

/// `body` is one of the constants above; `stroke` the stroke width on the 24-unit grid.
pub fn icon<'a, M: 'a>(body: &str, size: f32, stroke: f32, color: Color) -> Element<'a, M> {
    icon_rotated(body, size, stroke, color, 0.0)
}

/// Same, rotated (the library's open/closed chevron).
pub fn icon_rotated<'a, M: 'a>(body: &str, size: f32, stroke: f32, color: Color, degrees: f32) -> Element<'a, M> {
    image(raster(body, size, stroke, color, degrees)).width(size).height(size).into()
}

// ---- rasterising --------------------------------------------------------------------------------
//
// Icons are rasterised here at the exact physical size and shown as images, which iced snaps to
// the pixel grid. iced's SVG widget draws at fractional positions instead: at 125 %/150 % thin
// strokes blurred across two pixel rows or almost vanished (the library's +).

/// The window's total scale (system × UI scale), set by the shell every frame.
static SCALE: AtomicU32 = AtomicU32::new(0x3f80_0000); // 1.0

pub fn set_scale(scale: f32) {
    SCALE.store(scale.to_bits(), Ordering::Relaxed);
}

#[derive(Hash, PartialEq, Eq)]
struct Key {
    body: String,
    px: u32,
    stroke: u32,
    rgba: [u8; 4],
    degrees: u32,
}

fn raster(body: &str, size: f32, stroke: f32, color: Color, degrees: f32) -> image::Handle {
    static CACHE: LazyLock<Mutex<HashMap<Key, image::Handle>>> = LazyLock::new(Default::default);
    let px = (size * f32::from_bits(SCALE.load(Ordering::Relaxed))).round().max(1.0) as u32;
    let rgba = color.into_rgba8();
    let key = Key { body: body.to_string(), px, stroke: stroke.to_bits(), rgba, degrees: degrees.to_bits() };
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache.entry(key).or_insert_with(|| render(body, px, stroke, rgba, degrees)).clone()
}

fn render(body: &str, px: u32, stroke: f32, [r, g, b, a]: [u8; 4], degrees: f32) -> image::Handle {
    // `black` in a body (AUTO's filled half) means "the icon colour", like the stroke.
    let body = body.replace("\"black\"", "\"currentColor\"");
    let doc = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" color="#{r:02x}{g:02x}{b:02x}" opacity="{op}" fill="none" stroke="currentColor" stroke-width="{stroke}" stroke-linecap="round" stroke-linejoin="round"><g transform="rotate({degrees} 12 12)">{body}</g></svg>"##,
        op = a as f32 / 255.0,
    );
    let rendered = (|| {
        let tree = resvg::usvg::Tree::from_str(&doc, &resvg::usvg::Options::default()).ok()?;
        let mut pm = resvg::tiny_skia::Pixmap::new(px, px)?;
        let s = px as f32 / 24.0;
        resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(s, s), &mut pm.as_mut());
        Some(image::Handle::from_rgba(px, px, super::stage::unpremultiply(pm.data())))
    })();
    rendered.unwrap_or_else(|| image::Handle::from_rgba(1, 1, vec![0; 4]))
}
