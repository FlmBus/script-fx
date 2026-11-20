//! The meta knob's menu (right-click a meta knob): one modulation slot, as in Serum's matrix row.
//! A small paper card next to the click, like the value entry:
//!
//! - **Amount**: −100…+100 % of the knob's travel (the meta knob's drag, finer here);
//! - **Polarity**: uni (0…amount) | bi (−amount…+amount around the source's centre);
//! - **Curve**: tension on the source, −1 (fast start) … 0 (linear) … +1 (slow start);
//! - **Invert**: the range runs the other way (same amount);
//! - **Bypass**: mute the slot, keep its settings;
//! - **Reset** to what the patch defines. "Open in Matrix" comes with the Matrix.
//!
//! Changes apply live and are saved with the project (`Session::set_mod`). A press elsewhere or
//! Escape closes it.

use patch::ModSettings;

use super::theme;
use nice_plug_iced::iced::{
    Border, Center, Element, Point, Shadow, Theme, Vector,
    mouse,
    widget::{button, checkbox, column, container, mouse_area, row, slider, space, text},
};

/// The card's width (logical px); its height is about `HEIGHT`, for placing it.
pub const WIDTH: f32 = 220.0;
pub const HEIGHT: f32 = 228.0;

/// The open menu: param `param`'s `k`-th modulation, opened at `at` (window point).
pub struct MetaMenu {
    pub param: usize,
    pub k: usize,
    pub at: Point,
}

#[derive(Debug, Clone)]
pub enum Message {
    Set(ModSettings),
}

impl MetaMenu {
    /// `title`: "Heat → Amount"; `patch`: what Reset goes back to.
    pub fn view<'a>(&self, title: String, s: ModSettings, patch: ModSettings) -> Element<'a, Message> {
        let c = theme::c();
        let label = |t: &'static str| text(t).size(11.5).font(theme::font::PAPER).color(c.paper_muted).width(64);
        let value = |t: String| text(t).size(11.5).font(theme::font::PAPER_MONO).color(c.paper_ink).width(40).align_x(alignment_right());

        let amount = row![
            label("Amount"),
            slider(-1.0..=1.0, s.amount, move |a| Message::Set(ModSettings { amount: a, ..s })).step(0.005).style(theme::paper_slider),
            value(format!("{:+.0}%", s.amount * 100.0).replace('-', "−")),
        ]
        .spacing(8)
        .align_y(Center);

        let polarity = row![
            label("Polarity"),
            pills([("uni", !s.bipolar, ModSettings { bipolar: false, ..s }), ("bi", s.bipolar, ModSettings { bipolar: true, ..s })]),
        ]
        .spacing(8)
        .align_y(Center);

        let curve = row![
            label("Curve"),
            slider(-1.0..=1.0, s.curve, move |v| Message::Set(ModSettings { curve: if v.abs() < 0.03 { 0.0 } else { v }, ..s }))
                .step(0.01)
                .style(theme::paper_slider),
            value(if s.curve == 0.0 { "lin".into() } else { format!("{:+.2}", s.curve).replace('-', "−") }),
        ]
        .spacing(8)
        .align_y(Center);

        let invert = row![label("Invert"), checkbox(s.invert).on_toggle(move |i| Message::Set(ModSettings { invert: i, ..s })).size(14)]
            .spacing(8)
            .align_y(Center);

        let bypass = row![label("Bypass"), checkbox(s.bypass).on_toggle(move |b| Message::Set(ModSettings { bypass: b, ..s })).size(14)]
            .spacing(8)
            .align_y(Center);

        let footer = row![
            button(text("Reset").size(11.5).font(theme::font::PAPER)).on_press(Message::Set(patch)).padding([3, 8]).style(theme::paper_small),
            space::horizontal(),
            text("Matrix: later").size(11).font(theme::font::PAPER).color(c.paper_muted),
        ]
        .align_y(Center);

        let card = container(
            column![
                text(title).size(12.5).font(theme::font::PAPER_SEMIBOLD).color(c.paper_ink),
                amount,
                polarity,
                curve,
                invert,
                bypass,
                footer,
            ]
            .spacing(9),
        )
        .width(WIDTH)
        .padding(10)
        .style(|_: &Theme| container::Style {
            background: Some(theme::c().paper.into()),
            border: Border { radius: theme::radius(theme::RADIUS_FIELD), width: 1.0, color: theme::c().paper_edge },
            shadow: Shadow { color: theme::shadow(0.45), offset: Vector::new(0.0, 10.0), blur_radius: 28.0 },
            ..Default::default()
        });
        // Occupies the pointer like a sheet: a press on the card isn't a press on the page.
        mouse_area(card).interaction(mouse::Interaction::Idle).into()
    }
}

/// A row of pills; the one that's `on` is filled. Each sets its settings.
fn pills<'a, const N: usize>(options: [(&'static str, bool, ModSettings); N]) -> Element<'a, Message> {
    let c = theme::c();
    let pill = |(name, on, settings): (&'static str, bool, ModSettings)| -> Element<'a, Message> {
        button(container(text(name).size(11.5).font(theme::font::PAPER)).center_x(36))
            .on_press(Message::Set(settings))
            .padding([2, 0])
            .style(move |_: &Theme, status| button::Style {
                background: if on {
                    Some(c.paper_ink.into())
                } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                    Some(theme::alpha(c.paper_ink, 0.06).into())
                } else {
                    None
                },
                text_color: if on { c.on_paper_ink } else { c.paper_ink },
                border: Border { radius: theme::radius(theme::RADIUS_BUTTON - 2.0), ..Default::default() },
                ..Default::default()
            })
            .into()
    };
    container(row(options.map(pill)).spacing(2))
        .padding(2)
        .style(|_: &Theme| container::Style {
            background: Some(theme::alpha(theme::c().paper_ink, 0.05).into()),
            border: Border { radius: theme::radius(theme::RADIUS_BUTTON), ..Default::default() },
            ..Default::default()
        })
        .into()
}

fn alignment_right() -> nice_plug_iced::iced::alignment::Horizontal {
    nice_plug_iced::iced::alignment::Horizontal::Right
}
