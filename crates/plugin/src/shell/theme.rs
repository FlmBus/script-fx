//! Design tokens of the "ScriptFX Shell" design system (tokens.json) and the shared widget
//! styles built from them. Sections never hard-code a colour; they use these.

use std::sync::atomic::{AtomicBool, Ordering};

use nice_plug_iced::iced::{
    Background, Border, Color, Shadow, Theme, Vector, border, color,
    widget::{button, container, overlay, pick_list, scrollable, slider, text_editor, text_input},
};

// ---- colour -------------------------------------------------------------------------------------
//
// Two palettes with the same tokens (the design system's "dark" and "light" themes); sections
// read the current one through `c()`, so a mode switch restyles everything on the next frame.
// The sheets ("paper": chat, controls) follow the mode like everything else.
// Patch-owned colours (library cards, ControlPanels) don't change with the mode.

pub struct Palette {
    pub frame: Color,
    pub frame_raised: Color,
    pub frame_node: Color,
    pub frame_control: Color,
    pub frame_field: Color,
    pub frame_hairline: Color,
    pub frame_hairline_strong: Color,
    pub frame_wire: Color,
    pub frame_ink: Color,
    pub frame_ink_2: Color,
    pub frame_ink_3: Color,
    pub frame_muted: Color,
    pub frame_subtle: Color,
    /// Hover wash on transparent frame buttons.
    pub frame_hover: Color,
    pub frame_invert: Color,
    pub frame_invert_hover: Color,
    pub on_frame_invert: Color,
    /// Focus ring and text selection in frame inputs.
    pub frame_focus: Color,
    pub frame_selection: Color,
    /// The empty-state backdrop (top → bottom).
    pub frame_backdrop: (Color, Color),

    pub paper: Color,
    pub paper_raised: Color,
    pub paper_hover: Color,
    pub paper_line: Color,
    pub paper_divider: Color,
    pub paper_ink: Color,
    pub paper_ink_2: Color,
    pub paper_ink_3: Color,
    pub paper_muted: Color,
    pub on_paper_ink: Color,
    /// The sheet's outer edge.
    pub paper_edge: Color,

    pub signal: Color,
    pub signal_hover: Color,
    pub signal_pressed: Color,
    pub on_signal: Color,
    pub signal_ink: Color,

    pub led_ok: Color,
    pub led_reduce: Color,
    pub led_clip: Color,

    /// Shadow colour and strength (multiplies each shadow's alpha): softer on the light frame.
    pub shadow: Color,
    pub shadow_strength: f32,
}

pub const DARK: Palette = Palette {
    frame: color!(0x0b0b0c),
    frame_raised: color!(0x141416),
    frame_node: color!(0x161618),
    frame_control: Color::from_rgba(1.0, 1.0, 1.0, 0.08),
    frame_field: Color::from_rgba(1.0, 1.0, 1.0, 0.05),
    frame_hairline: Color::from_rgba(1.0, 1.0, 1.0, 0.06),
    frame_hairline_strong: Color::from_rgba(1.0, 1.0, 1.0, 0.09),
    frame_wire: Color::from_rgba(1.0, 1.0, 1.0, 0.18),
    frame_ink: color!(0xfafafa),
    frame_ink_2: color!(0xe4e4e7),
    frame_ink_3: color!(0xd4d4d8),
    frame_muted: color!(0xa1a1aa),
    frame_subtle: color!(0x8b8b92),
    frame_hover: Color::from_rgba(1.0, 1.0, 1.0, 0.06),
    frame_invert: color!(0xf4f4f5),
    frame_invert_hover: color!(0xffffff),
    on_frame_invert: color!(0x0b0b0c),
    frame_focus: Color::from_rgba(1.0, 1.0, 1.0, 0.22),
    frame_selection: Color::from_rgba(1.0, 1.0, 1.0, 0.25),
    frame_backdrop: (color!(0x1a1a1d), color!(0x0f0f11)),

    paper: color!(0x1c1c1f),
    paper_raised: color!(0x26262a),
    paper_hover: color!(0x2e2e33),
    paper_line: color!(0x3a3a40),
    paper_divider: color!(0x2e2e33),
    paper_ink: color!(0xf4f4f5),
    paper_ink_2: color!(0xe4e4e7),
    paper_ink_3: color!(0xc4c4cb),
    paper_muted: color!(0xa1a1aa),
    on_paper_ink: color!(0x1c1c1f),
    paper_edge: Color::from_rgba(1.0, 1.0, 1.0, 0.08),

    signal: color!(0xff5a1f),
    signal_hover: color!(0xff6d38),
    signal_pressed: color!(0xf04d12),
    on_signal: color!(0x1a1a1a),
    signal_ink: color!(0xfb923c),

    led_ok: color!(0x4ade80),
    led_reduce: color!(0xfbbf24),
    led_clip: color!(0xf87171),

    shadow: Color::BLACK,
    shadow_strength: 1.0,
};

pub const LIGHT: Palette = Palette {
    frame: color!(0xe7e7ea),
    frame_raised: color!(0xf4f4f6),
    frame_node: color!(0xf7f7f9),
    frame_control: Color::from_rgba(0.0, 0.0, 0.0, 0.06),
    frame_field: Color::from_rgba(0.0, 0.0, 0.0, 0.04),
    frame_hairline: Color::from_rgba(0.0, 0.0, 0.0, 0.08),
    frame_hairline_strong: Color::from_rgba(0.0, 0.0, 0.0, 0.13),
    frame_wire: Color::from_rgba(0.0, 0.0, 0.0, 0.22),
    frame_ink: color!(0x18181b),
    frame_ink_2: color!(0x27272a),
    frame_ink_3: color!(0x3f3f46),
    frame_muted: color!(0x52525b),
    frame_subtle: color!(0x5f5f67),
    frame_hover: Color::from_rgba(0.0, 0.0, 0.0, 0.06),
    frame_invert: color!(0x18181b),
    frame_invert_hover: color!(0x000000),
    on_frame_invert: color!(0xfafafa),
    frame_focus: Color::from_rgba(0.0, 0.0, 0.0, 0.3),
    frame_selection: Color::from_rgba(0.0, 0.0, 0.0, 0.18),
    frame_backdrop: (color!(0xf2f2f4), color!(0xe0e0e4)),

    paper: color!(0xf7f6f2),
    paper_raised: color!(0xffffff),
    paper_hover: color!(0xfbfaf7),
    paper_line: color!(0xd6d3cb),
    paper_divider: color!(0xe3e0d9),
    paper_ink: color!(0x1a1a1a),
    paper_ink_2: color!(0x2b2a27),
    paper_ink_3: color!(0x47443e),
    paper_muted: color!(0x625e56),
    on_paper_ink: color!(0xf7f6f2),
    paper_edge: Color::from_rgba(0.0, 0.0, 0.0, 0.08),

    signal: color!(0xff5a1f),
    signal_hover: color!(0xff6d38),
    signal_pressed: color!(0xf04d12),
    on_signal: color!(0x1a1a1a),
    signal_ink: color!(0xc2410c),

    led_ok: color!(0x15803d),
    led_reduce: color!(0xb45309),
    led_clip: color!(0xdc2626),

    shadow: color!(0x1a1a24),
    shadow_strength: 0.4,
};

static LIGHT_MODE: AtomicBool = AtomicBool::new(false);

/// A shadow colour at `a` (the dark-mode alpha), scaled for the current mode.
pub fn shadow(a: f32) -> Color {
    alpha(c().shadow, a * c().shadow_strength)
}

/// The current palette.
pub fn c() -> &'static Palette {
    if LIGHT_MODE.load(Ordering::Relaxed) { &LIGHT } else { &DARK }
}

/// Switches the palette for the whole process (all ScriptFX windows share one look).
pub fn set_light(light: bool) {
    LIGHT_MODE.store(light, Ordering::Relaxed);
}

/// The OS's dark/light setting, for the "System" mode. Always read on a helper thread, every few
/// seconds (the D-Bus portal call on Linux can block for a long time when no portal answers, so
/// never on the UI thread); dark until the first answer. A change in the OS shows up while open.
pub fn system_is_light() -> bool {
    static SYSTEM_LIGHT: AtomicBool = AtomicBool::new(false);
    static WATCH: std::sync::Once = std::sync::Once::new();
    WATCH.call_once(|| {
        let _ = std::thread::Builder::new().name("scriptfx-theme-watch".into()).spawn(|| {
            loop {
                let light = matches!(dark_light::detect(), Ok(dark_light::Mode::Light));
                SYSTEM_LIGHT.store(light, Ordering::Relaxed);
                std::thread::sleep(std::time::Duration::from_secs(3));
            }
        });
    });
    SYSTEM_LIGHT.load(Ordering::Relaxed)
}

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

pub const SIZE_TOP_BAR: f32 = 34.0;
/// The library sheet (left).
pub const SIZE_LIBRARY: f32 = 248.0;
pub const SIZE_SHEET: f32 = 340.0;
pub const SIZE_CONTROL: f32 = 30.0;
/// The mod dock below the stage: macros and modulators in one strip.
pub const SIZE_DOCK: f32 = 108.0;

/// The stage (the patch window) at rest = the ControlPanel's area: the editor minus top bar,
/// gutters and the dock. The sizes are chosen so this is exactly the ControlPanel's 3:2
/// (816×544): no letterbox bars around the panel (`stage_fits_the_panel` checks it). The notch
/// hangs into it; the dock has a row of its own below it.
pub fn stage_size() -> (f32, f32) {
    let (w, h) = crate::WINDOW_SIZE;
    (w - 2.0 * SPACE_GUTTER, h - SIZE_TOP_BAR - 2.0 * SPACE_GUTTER - SPACE_2 - SIZE_DOCK)
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
    container::Style { background: Some(c().frame.into()), text_color: Some(c().frame_ink_3), ..Default::default() }
}

pub fn paper_sheet(_: &Theme) -> container::Style {
    container::Style {
        background: Some(c().paper.into()),
        text_color: Some(c().paper_ink),
        border: Border { radius: radius(RADIUS_WINDOW), width: 1.0, color: c().paper_edge },
        // shadow-sheet: 0 24px 60px rgba(0,0,0,0.55)
        shadow: Shadow { color: shadow(0.55), offset: Vector::new(0.0, 24.0), blur_radius: 60.0 },
        snap: true,
    }
}

pub fn paper_raised(_: &Theme) -> container::Style {
    container::Style {
        background: Some(c().paper_raised.into()),
        border: Border { radius: radius(RADIUS_FIELD), width: 1.0, color: c().paper_line },
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
                background: filled.then(|| c().frame_field.into()),
                text_color: alpha(c().frame_ink_2, 0.35),
                border: Border { radius: radius(RADIUS_CONTROL), ..Default::default() },
                ..Default::default()
            };
        }
        let base = if filled { c().frame_control } else { Color::TRANSPARENT };
        let bg = match status {
            button::Status::Hovered | button::Status::Pressed if !filled => c().frame_hover,
            _ => hovered(base, status, 0.05),
        };
        button::Style {
            background: Some(bg.into()),
            text_color: c().frame_ink_2,
            border: Border { radius: radius(RADIUS_CONTROL), ..Default::default() },
            ..Default::default()
        }
    }
}

/// Frame primary: the one strong action per view (Build patch, Chat toggle while open).
pub fn frame_primary(_: &Theme, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => c().frame_invert_hover,
        button::Status::Disabled => alpha(c().frame_invert, 0.25),
        _ => c().frame_invert,
    };
    button::Style {
        background: Some(bg.into()),
        text_color: if status == button::Status::Disabled { alpha(c().frame_ink, 0.45) } else { c().on_frame_invert },
        border: Border { radius: radius(RADIUS_CONTROL), ..Default::default() },
        ..Default::default()
    }
}

/// A recessed track on the frame that holds segments (the notch's view switch, the drawer's tabs).
pub fn frame_track(_: &Theme) -> container::Style {
    container::Style {
        background: Some(c().frame_field.into()),
        border: Border { radius: radius(RADIUS_SECTION), ..Default::default() },
        ..Default::default()
    }
}

/// A segment of the notch's view switch: inverted when `on` (the strong state on the frame),
/// a light wash on hover, dimmed when disabled.
pub fn frame_switch(on: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let c = c();
        // `on` first: the current view has no action, so iced reports it as disabled.
        let (bg, ink) = match status {
            _ if on => (Some(c.frame_invert), c.on_frame_invert),
            button::Status::Disabled => (None, alpha(c.frame_ink_2, 0.35)),
            button::Status::Hovered | button::Status::Pressed => (Some(c.frame_hover), c.frame_ink),
            _ => (None, c.frame_ink_2),
        };
        button::Style {
            background: bg.map(Into::into),
            text_color: ink,
            border: Border { radius: radius(RADIUS_BUTTON), ..Default::default() },
            ..Default::default()
        }
    }
}

/// A tab in the drawer's track: raised when `on` (a lighter face and a small shadow, not
/// inverted: the tabs are many and equal), a light wash on hover.
pub fn frame_tab(on: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let c = c();
        let hover = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: if on {
                Some(alpha(c.frame_ink, 0.12).into())
            } else if hover {
                Some(c.frame_hover.into())
            } else {
                None
            },
            text_color: if on { c.frame_ink } else { c.frame_ink_2 },
            border: Border { radius: radius(RADIUS_BUTTON), ..Default::default() },
            shadow: if on {
                Shadow { color: shadow(0.4), offset: Vector::new(0.0, 1.0), blur_radius: 3.0 }
            } else {
                Shadow::default()
            },
            ..Default::default()
        }
    }
}

pub fn paper_slider(_: &Theme, status: slider::Status) -> slider::Style {
    let handle = match status {
        slider::Status::Hovered | slider::Status::Dragged => c().paper_ink,
        _ => c().paper_ink_2,
    };
    slider::Style {
        rail: slider::Rail {
            backgrounds: (Background::Color(c().paper_ink), Background::Color(c().paper_line)),
            width: 3.0,
            border: Border { radius: radius(2.0), ..Default::default() },
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Circle { radius: 6.0 },
            background: Background::Color(c().paper_raised),
            border_width: 2.0,
            border_color: handle,
        },
    }
}

/// Paper small: History, Undo, Save as snapshot, Listen.
pub fn paper_small(_: &Theme, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => c().paper_hover,
        button::Status::Pressed => c().paper_divider,
        _ => c().paper_raised,
    };
    button::Style {
        background: Some(bg.into()),
        text_color: if status == button::Status::Disabled { c().paper_muted } else { c().paper_ink },
        border: Border { radius: radius(RADIUS_BUTTON), width: 1.0, color: c().paper_line },
        ..Default::default()
    }
}

/// Signal: Send only. The single orange in the shell.
pub fn signal(_: &Theme, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => c().signal_hover,
        button::Status::Pressed => c().signal_pressed,
        _ => c().signal,
    };
    button::Style {
        background: Some(bg.into()),
        text_color: c().on_signal,
        border: Border { radius: radius(RADIUS_FIELD), ..Default::default() },
        ..Default::default()
    }
}

/// Borderless icon button on paper (close).
pub fn paper_bare(_: &Theme, status: button::Status) -> button::Style {
    button::Style {
        background: match status {
            button::Status::Hovered | button::Status::Pressed => Some(alpha(c().paper_ink, 0.06).into()),
            _ => None,
        },
        text_color: c().paper_ink,
        border: Border { radius: radius(RADIUS_BUTTON), ..Default::default() },
        ..Default::default()
    }
}

// ---- inputs -------------------------------------------------------------------------------------

pub fn frame_input(_: &Theme, status: text_input::Status) -> text_input::Style {
    let focused = matches!(status, text_input::Status::Focused { .. });
    text_input::Style {
        background: Background::Color(c().frame_field),
        border: Border {
            radius: radius(RADIUS_CONTROL),
            width: 1.0,
            color: if focused { c().frame_focus } else { c().frame_hairline_strong },
        },
        icon: c().frame_muted,
        placeholder: c().frame_subtle,
        value: c().frame_ink,
        selection: c().frame_selection,
    }
}

/// A pick list on the frame (the Matrix's add row): a field like `frame_input`.
pub fn frame_pick_list(_: &Theme, status: pick_list::Status) -> pick_list::Style {
    let open = matches!(status, pick_list::Status::Opened { .. });
    let hover = matches!(status, pick_list::Status::Hovered);
    pick_list::Style {
        text_color: c().frame_ink,
        placeholder_color: c().frame_subtle,
        handle_color: c().frame_muted,
        background: Background::Color(c().frame_field),
        border: Border {
            radius: radius(RADIUS_CONTROL),
            width: 1.0,
            color: if open { c().frame_focus } else if hover { c().frame_ink_3 } else { c().frame_hairline_strong },
        },
    }
}

/// A pick list on a sheet.
pub fn paper_pick_list(_: &Theme, status: pick_list::Status) -> pick_list::Style {
    let active = matches!(status, pick_list::Status::Opened { .. } | pick_list::Status::Hovered);
    pick_list::Style {
        text_color: c().paper_ink,
        placeholder_color: c().paper_muted,
        handle_color: c().paper_muted,
        background: Background::Color(c().paper_raised),
        border: Border { radius: radius(RADIUS_BUTTON), width: 1.0, color: if active { c().paper_muted } else { c().paper_line } },
    }
}

pub fn paper_menu(_: &Theme) -> overlay::menu::Style {
    overlay::menu::Style {
        background: Background::Color(c().paper),
        border: Border { radius: radius(RADIUS_BUTTON), width: 1.0, color: c().paper_edge },
        text_color: c().paper_ink_2,
        selected_text_color: c().paper_ink,
        selected_background: Background::Color(alpha(c().paper_ink, 0.1)),
        shadow: Shadow { color: shadow(0.4), offset: Vector::new(0.0, 6.0), blur_radius: 18.0 },
    }
}

/// The open pick list's menu.
pub fn frame_menu(_: &Theme) -> overlay::menu::Style {
    overlay::menu::Style {
        background: Background::Color(c().frame_raised),
        border: Border { radius: radius(RADIUS_CONTROL), width: 1.0, color: c().frame_hairline_strong },
        text_color: c().frame_ink_2,
        selected_text_color: c().frame_ink,
        selected_background: Background::Color(alpha(c().frame_ink, 0.1)),
        shadow: Shadow { color: shadow(0.4), offset: Vector::new(0.0, 6.0), blur_radius: 18.0 },
    }
}

pub fn paper_editor(_: &Theme, status: text_editor::Status) -> text_editor::Style {
    let focused = matches!(status, text_editor::Status::Focused { .. });
    text_editor::Style {
        background: Background::Color(c().paper_raised),
        border: Border {
            radius: radius(RADIUS_FIELD),
            width: 1.0,
            color: if focused { c().paper_muted } else { c().paper_line },
        },
        placeholder: c().paper_muted,
        value: c().paper_ink,
        selection: alpha(c().signal, 0.3),
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
                background: Background::Color(c().paper_raised),
                border: Border { radius: radius(RADIUS_PILL), width: 1.0, color: c().paper_line },
                shadow: Shadow::default(),
                icon: c().paper_ink,
            },
        }
    }
}

/// A horizontal scrollbar on the frame (the chain view): faint, a bit stronger on hover.
pub fn frame_hscrollbar(t: &Theme, s: scrollable::Status) -> scrollable::Style {
    let (disabled, hovered) = match s {
        scrollable::Status::Active { is_horizontal_scrollbar_disabled, .. } => (is_horizontal_scrollbar_disabled, false),
        scrollable::Status::Hovered { is_horizontal_scrollbar_disabled, is_horizontal_scrollbar_hovered, .. } => {
            (is_horizontal_scrollbar_disabled, is_horizontal_scrollbar_hovered)
        }
        scrollable::Status::Dragged { is_horizontal_scrollbar_disabled, .. } => (is_horizontal_scrollbar_disabled, true),
    };
    let a = if disabled { 0.0 } else if hovered { 0.3 } else { 0.1 };
    let mut style = scrollbar(c().frame_ink, 0.0, 0.0)(t, s);
    style.horizontal_rail.scroller.background = alpha(c().frame_ink, a).into();
    style
}

/// On paper (the sheets).
pub fn paper_scrollbar(t: &Theme, s: scrollable::Status) -> scrollable::Style {
    scrollbar(c().paper_ink, 0.18, 0.45)(t, s)
}


#[cfg(test)]
mod tests {
    #[test]
    fn stage_fits_the_panel() {
        let (w, h) = super::stage_size();
        assert_eq!(w * ui_components::HEIGHT, h * ui_components::WIDTH, "stage {w}×{h} isn't the panel's aspect ratio");
    }
}
