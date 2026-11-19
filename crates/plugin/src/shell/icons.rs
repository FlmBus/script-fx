//! Inline stroke icons (24-unit grid, round caps), tinted with the ink of their surface.

use nice_plug_iced::iced::{
    Color, Element, Theme,
    widget::svg::{self, Svg},
};

pub const PLUS: &str = r#"<path d="M12 5v14M5 12h14"/>"#;
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
    let doc = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="{stroke}" stroke-linecap="round" stroke-linejoin="round">{body}</svg>"#
    );
    Svg::new(svg::Handle::from_memory(doc.into_bytes()))
        .width(size)
        .height(size)
        .style(move |_: &Theme, _| svg::Style { color: Some(color) })
        .into()
}

/// Same, rotated (the library's open/closed chevron).
pub fn icon_rotated<'a, M: 'a>(body: &str, size: f32, stroke: f32, color: Color, degrees: f32) -> Element<'a, M> {
    let doc = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="{stroke}" stroke-linecap="round" stroke-linejoin="round"><g transform="rotate({degrees} 12 12)">{body}</g></svg>"#
    );
    Svg::new(svg::Handle::from_memory(doc.into_bytes()))
        .width(size)
        .height(size)
        .style(move |_: &Theme, _| svg::Style { color: Some(color) })
        .into()
}
