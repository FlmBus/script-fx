//! Inline stroke icons (24-unit grid, round caps), tinted with the ink of their surface.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{LazyLock, Mutex};

use iced_wgpu::frame_stats::phase;
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
/// The library: two stacked cards.
pub const LIBRARY: &str = r#"<rect x="4" y="4" width="16" height="6.5" rx="1.5"/><rect x="4" y="13.5" width="16" height="6.5" rx="1.5"/>"#;
pub const CHEVRON_RIGHT: &str = r#"<path d="M9 5l7 7-7 7"/>"#;
pub const CHEVRON_DOWN: &str = r#"<path d="M6 9l6 6 6-6"/>"#;
pub const SAVE: &str = r#"<path d="M5 3h11l3 3v15H5z"/><path d="M8 3v6h8V3M8 21v-7h8v7"/>"#;
pub const FOLDER: &str = r#"<path d="M3 6h6l2 2h10v11H3z"/>"#;
pub const RELOAD: &str = r#"<path d="M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7"/>"#;
pub const CHAT: &str = r#"<path d="M4 5h16v11H9l-5 4z"/>"#;
/// Settings (the shell's own). Gear outline from Lucide "settings" (ISC licence).
pub const GEAR: &str = r#"<path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/>"#;
/// The patch's controls (macros, params): two sliders.
pub const SLIDERS: &str = r#"<path d="M4 7h9M17 7h3M4 17h3M11 17h9"/><circle cx="15" cy="7" r="2"/><circle cx="9" cy="17" r="2"/>"#;
/// The surfaces (Surface: a faceplate with two knobs; Chain: two blocks joined; Matrix: a table).
pub const VIEW_SURFACE: &str = r#"<rect x="3" y="4" width="18" height="16" rx="2"/><circle cx="8.5" cy="13" r="2.5"/><circle cx="15.5" cy="13" r="2.5"/>"#;
pub const VIEW_CHAIN: &str = r#"<rect x="2" y="9" width="6" height="6" rx="1.5"/><rect x="16" y="9" width="6" height="6" rx="1.5"/><path d="M8 12h8"/>"#;
pub const VIEW_MATRIX: &str = r#"<path d="M3 6h18M3 12h18M3 18h18M9 3v18"/>"#;
/// Bypass (Lucide "power", ISC).
pub const POWER: &str = r#"<path d="M12 3v9"/><path d="M18.4 6.6a9 9 0 1 1-12.8 0"/>"#;
pub const CLOSE: &str =r#"<path d="M6 6l12 12M18 6L6 18"/>"#;
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
    // `SCRIPTFX_DIAG_NO_ICONS`: nothing in the texture atlas but the patch's layers (`diag.rs`).
    // One shared handle, so this doesn't itself add an atlas entry per frame.
    if crate::diag::switches().no_icons {
        static BLANK: LazyLock<image::Handle> = LazyLock::new(|| image::Handle::from_rgba(1, 1, vec![0; 4]));
        return BLANK.clone();
    }
    let px = (size * f32::from_bits(SCALE.load(Ordering::Relaxed))).round().max(1.0) as u32;
    let rgba = color.into_rgba8();
    let key = Key { body: body.to_string(), px, stroke: stroke.to_bits(), rgba, degrees: degrees.to_bits() };
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache.entry(key).or_insert_with(|| render(body, px, stroke, rgba, degrees)).clone()
}

fn render(body: &str, px: u32, stroke: f32, [r, g, b, a]: [u8; 4], degrees: f32) -> image::Handle {
    // Counted and timed: in a steady state no icon should be rasterised at all, so a frame line
    // that keeps reporting icon rasters means the cache key changes (scale, colour) every frame.
    crate::diag::note_icon_raster();
    let was = phase::enter(phase::ICON_RASTER);
    let handle = render_inner(body, px, stroke, [r, g, b, a], degrees);
    phase::enter(was);
    handle
}

fn render_inner(body: &str, px: u32, stroke: f32, [r, g, b, a]: [u8; 4], degrees: f32) -> image::Handle {
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
