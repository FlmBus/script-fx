//! Design tokens of the "ScriptFX Shell" design system (tokens.json) and the shared widget
//! styles built from them. Sections never hard-code a colour; they use these.

use nice_plug_iced::iced::{
    Background, Border, Color, Shadow, Theme, Vector, border, color,
    widget::{button, container, scrollable, text_editor, text_input},
};

// ---- colour -------------------------------------------------------------------------------------

pub const FRAME: Color = color!(0x0b0b0c);
pub const FRAME_RAISED: Color = color!(0x141416);
pub const FRAME_NODE: Color = color!(0x161618);
pub const FRAME_CONTROL: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.08);
pub const FRAME_FIELD: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.05);
pub const FRAME_HAIRLINE: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.06);
pub const FRAME_HAIRLINE_STRONG: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.09);
pub const FRAME_WIRE: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.18);
pub const FRAME_INK: Color = color!(0xfafafa);
pub const FRAME_INK_2: Color = color!(0xe4e4e7);
pub const FRAME_INK_3: Color = color!(0xd4d4d8);
pub const FRAME_MUTED: Color = color!(0xa1a1aa);
pub const FRAME_SUBTLE: Color = color!(0x8b8b92);
pub const FRAME_INVERT: Color = color!(0xf4f4f5);
pub const ON_FRAME_INVERT: Color = FRAME;

pub const PAPER: Color = color!(0xf7f6f2);
pub const PAPER_RAISED: Color = color!(0xffffff);
pub const PAPER_LINE: Color = color!(0xd6d3cb);
pub const PAPER_DIVIDER: Color = color!(0xe3e0d9);
pub const PAPER_INK: Color = color!(0x1a1a1a);
pub const PAPER_INK_2: Color = color!(0x2b2a27);
pub const PAPER_MUTED: Color = color!(0x625e56);
pub const ON_PAPER_INK: Color = PAPER;

pub const SIGNAL: Color = color!(0xff5a1f);
pub const SIGNAL_INK: Color = color!(0xc2410c);

pub const LED_OK: Color = color!(0x4ade80);
pub const LED_REDUCE: Color = color!(0xfbbf24);
pub const LED_CLIP: Color = color!(0xf87171);

// ---- spacing, radius, size ----------------------------------------------------------------------

pub const SPACE_HAIR: f32 = 2.0;
#[allow(dead_code)]
pub const SPACE_1: f32 = 4.0;
pub const SPACE_2: f32 = 8.0;
pub const SPACE_GUTTER: f32 = 10.0;
#[allow(dead_code)]
pub const SPACE_3: f32 = 12.0;
#[allow(dead_code)]
pub const SPACE_4: f32 = 16.0;

pub const RADIUS_WINDOW: f32 = 14.0;
pub const RADIUS_NOTCH: f32 = 16.0;
pub const RADIUS_CONTROL: f32 = 11.0;
pub const RADIUS_SECTION: f32 = 10.0;
pub const RADIUS_FIELD: f32 = 12.0;
pub const RADIUS_BUTTON: f32 = 8.0;
pub const RADIUS_PILL: f32 = 999.0;

pub const SIZE_SIDEBAR: f32 = 214.0;
pub const SIZE_SHEET: f32 = 340.0;
pub const SIZE_CONTROL: f32 = 30.0;
pub const SIZE_NODE: f32 = 22.0;
pub const SIZE_SIGNAL_LINE: f32 = 52.0;

/// The patch window's size: the editor minus sidebar, gutters and signal line (fixed window).
pub fn stage_size() -> (f32, f32) {
    let (w, h) = crate::WINDOW_SIZE;
    (w - SIZE_SIDEBAR - SPACE_GUTTER, h - SPACE_GUTTER - SIZE_SIGNAL_LINE)
}

// ---- type ---------------------------------------------------------------------------------------

pub mod font {
    use nice_plug_iced::iced::{Font, font::Family, font::Style, font::Weight};

    const fn face(name: &'static str, weight: Weight) -> Font {
        Font { family: Family::Name(name), weight, stretch: nice_plug_iced::iced::font::Stretch::Normal, style: Style::Normal }
    }

    /// Frame text (Inter).
    pub const SANS: Font = face("Inter", Weight::Normal);
    pub const SANS_MEDIUM: Font = face("Inter", Weight::Medium);
    pub const SANS_SEMIBOLD: Font = face("Inter", Weight::Semibold);
    /// Paper text (DM Sans): the chat sheet reads as a different material.
    pub const PAPER: Font = face("DM Sans 9pt", Weight::Normal);
    pub const PAPER_SEMIBOLD: Font = face("DM Sans 9pt", Weight::Semibold);
    /// Readouts on frame / parameter changes on paper.
    pub const MONO: Font = face("JetBrains Mono", Weight::Normal);
    pub const PAPER_MONO: Font = face("DM Mono", Weight::Normal);

    pub const FILES: &[&[u8]] = &[
        include_bytes!("../../fonts/inter-400.ttf"),
        include_bytes!("../../fonts/inter-500.ttf"),
        include_bytes!("../../fonts/inter-600.ttf"),
        include_bytes!("../../fonts/inter-700.ttf"),
        include_bytes!("../../fonts/dm-sans-400.ttf"),
        include_bytes!("../../fonts/dm-sans-600.ttf"),
        include_bytes!("../../fonts/jetbrains-mono-400.ttf"),
        include_bytes!("../../fonts/dm-mono-400.ttf"),
    ];
}

// ---- helpers ------------------------------------------------------------------------------------

pub fn alpha(c: Color, a: f32) -> Color {
    Color { a, ..c }
}

/// Linear blend `a` → `b` by `t` (0..1).
pub fn mix(a: Color, b: Color, t: f32) -> Color {
    Color::from_rgba(a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t, a.b + (b.b - a.b) * t, a.a + (b.a - a.a) * t)
}

pub fn radius(r: f32) -> border::Radius {
    r.into()
}

// ---- surfaces -----------------------------------------------------------------------------------

pub fn frame(_: &Theme) -> container::Style {
    container::Style { background: Some(FRAME.into()), text_color: Some(FRAME_INK_3), ..Default::default() }
}

pub fn paper_sheet(_: &Theme) -> container::Style {
    container::Style {
        background: Some(PAPER.into()),
        text_color: Some(PAPER_INK),
        border: Border { radius: radius(RADIUS_WINDOW), width: 1.0, color: alpha(Color::WHITE, 0.1) },
        // shadow-sheet: 0 24px 60px rgba(0,0,0,0.55)
        shadow: Shadow { color: alpha(Color::BLACK, 0.55), offset: Vector::new(0.0, 24.0), blur_radius: 60.0 },
        snap: true,
    }
}

pub fn paper_raised(_: &Theme) -> container::Style {
    container::Style {
        background: Some(PAPER_RAISED.into()),
        border: Border { radius: radius(RADIUS_FIELD), width: 1.0, color: PAPER_LINE },
        ..Default::default()
    }
}

pub fn rule(c: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style { background: Some(c.into()), ..Default::default() }
}

// ---- buttons (README "Buttons": four kinds, each tied to a surface) -------------------------------

fn hovered(base: Color, status: button::Status, by: f32) -> Color {
    match status {
        button::Status::Hovered => Color { a: (base.a + by).min(1.0), ..base },
        button::Status::Pressed => Color { a: (base.a + by * 2.0).min(1.0), ..base },
        _ => base,
    }
}

/// Frame ghost: secondary actions on the frame. `filled` = with frame-control fill
/// (notch buttons at rest are transparent).
pub fn frame_ghost(filled: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        if status == button::Status::Disabled {
            return button::Style {
                background: filled.then(|| FRAME_FIELD.into()),
                text_color: alpha(FRAME_INK_2, 0.35),
                border: Border { radius: radius(RADIUS_CONTROL), ..Default::default() },
                ..Default::default()
            };
        }
        let base = if filled { FRAME_CONTROL } else { Color::TRANSPARENT };
        let bg = match status {
            button::Status::Hovered | button::Status::Pressed if !filled => alpha(Color::WHITE, 0.06),
            _ => hovered(base, status, 0.05),
        };
        button::Style {
            background: Some(bg.into()),
            text_color: FRAME_INK_2,
            border: Border { radius: radius(RADIUS_CONTROL), ..Default::default() },
            ..Default::default()
        }
    }
}

/// Frame primary: the one strong action per view (Build patch, Chat toggle while open).
pub fn frame_primary(_: &Theme, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => color!(0xffffff),
        button::Status::Disabled => alpha(FRAME_INVERT, 0.25),
        _ => FRAME_INVERT,
    };
    button::Style {
        background: Some(bg.into()),
        text_color: if status == button::Status::Disabled { alpha(FRAME_INK, 0.45) } else { ON_FRAME_INVERT },
        border: Border { radius: radius(RADIUS_CONTROL), ..Default::default() },
        ..Default::default()
    }
}

/// Paper small: History, Undo, Save as snapshot, Listen.
pub fn paper_small(_: &Theme, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => color!(0xfbfaf7),
        button::Status::Pressed => PAPER_DIVIDER,
        _ => PAPER_RAISED,
    };
    button::Style {
        background: Some(bg.into()),
        text_color: if status == button::Status::Disabled { PAPER_MUTED } else { PAPER_INK },
        border: Border { radius: radius(RADIUS_BUTTON), width: 1.0, color: PAPER_LINE },
        ..Default::default()
    }
}

/// Signal: Send only. The single orange in the shell.
pub fn signal(_: &Theme, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => color!(0xff6d38),
        button::Status::Pressed => color!(0xf04d12),
        _ => SIGNAL,
    };
    button::Style {
        background: Some(bg.into()),
        text_color: PAPER_INK,
        border: Border { radius: radius(RADIUS_FIELD), ..Default::default() },
        ..Default::default()
    }
}

/// Borderless icon button on paper (close).
pub fn paper_bare(_: &Theme, status: button::Status) -> button::Style {
    button::Style {
        background: match status {
            button::Status::Hovered | button::Status::Pressed => Some(alpha(PAPER_INK, 0.06).into()),
            _ => None,
        },
        text_color: PAPER_INK,
        border: Border { radius: radius(RADIUS_BUTTON), ..Default::default() },
        ..Default::default()
    }
}

// ---- inputs -------------------------------------------------------------------------------------

pub fn frame_input(_: &Theme, status: text_input::Status) -> text_input::Style {
    let focused = matches!(status, text_input::Status::Focused { .. });
    text_input::Style {
        background: Background::Color(FRAME_FIELD),
        border: Border {
            radius: radius(RADIUS_CONTROL),
            width: 1.0,
            color: if focused { alpha(Color::WHITE, 0.22) } else { FRAME_HAIRLINE_STRONG },
        },
        icon: FRAME_MUTED,
        placeholder: FRAME_SUBTLE,
        value: FRAME_INK,
        selection: alpha(Color::WHITE, 0.25),
    }
}

pub fn paper_editor(_: &Theme, status: text_editor::Status) -> text_editor::Style {
    let focused = matches!(status, text_editor::Status::Focused { .. });
    text_editor::Style {
        background: Background::Color(PAPER_RAISED),
        border: Border {
            radius: radius(RADIUS_FIELD),
            width: 1.0,
            color: if focused { PAPER_MUTED } else { PAPER_LINE },
        },
        placeholder: PAPER_MUTED,
        value: PAPER_INK,
        selection: alpha(SIGNAL, 0.3),
    }
}

// ---- scrollbars -----------------------------------------------------------------------------------

/// Thin overlay scrollbar (no rail), shown only when the content overflows.
pub fn thin_scrollbar() -> scrollable::Direction {
    scrollable::Direction::Vertical(scrollable::Scrollbar::new().width(4).scroller_width(4).margin(3))
}

fn scrollbar(ink: Color, idle: f32, hover: f32) -> impl Fn(&Theme, scrollable::Status) -> scrollable::Style {
    move |_, status| {
        let a = match status {
            scrollable::Status::Active { is_vertical_scrollbar_disabled: true, .. }
            | scrollable::Status::Hovered { is_vertical_scrollbar_disabled: true, .. }
            | scrollable::Status::Dragged { is_vertical_scrollbar_disabled: true, .. } => 0.0,
            scrollable::Status::Active { .. } => idle,
            scrollable::Status::Hovered { is_vertical_scrollbar_hovered, .. } => {
                if is_vertical_scrollbar_hovered { hover } else { (idle + hover) / 2.0 }
            }
            scrollable::Status::Dragged { .. } => hover,
        };
        let rail = scrollable::Rail {
            background: None,
            border: Border::default(),
            scroller: scrollable::Scroller { background: alpha(ink, a).into(), border: Border { radius: radius(2.0), ..Default::default() } },
        };
        scrollable::Style {
            container: container::Style::default(),
            vertical_rail: rail,
            horizontal_rail: rail,
            gap: None,
            auto_scroll: scrollable::AutoScroll {
                background: Background::Color(PAPER_RAISED),
                border: Border { radius: radius(RADIUS_PILL), width: 1.0, color: PAPER_LINE },
                shadow: Shadow::default(),
                icon: PAPER_INK,
            },
        }
    }
}

/// On paper (the sheets).
pub fn paper_scrollbar(t: &Theme, s: scrollable::Status) -> scrollable::Style {
    scrollbar(PAPER_INK, 0.18, 0.45)(t, s)
}

/// On the frame (the library).
pub fn frame_scrollbar(t: &Theme, s: scrollable::Status) -> scrollable::Style {
    scrollbar(Color::WHITE, 0.12, 0.3)(t, s)
}
