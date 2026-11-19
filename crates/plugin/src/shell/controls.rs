//! The controls sheet (Settings in the notch): the patch's macros (host-automatable) and its
//! params, plus patch settings (hot reload, folder). Paper like the chat; the two share the
//! window's right side.

use nice_plug_iced::iced::{
    Background, Border, Center, Element, Fill, Padding, Theme,
    widget::{checkbox, column, row, scrollable, slider, space, text},
};
use patch::{Control, Patch, Values};

use super::{sheet, theme};

#[derive(Debug, Clone)]
pub enum Message {
    Close,
    /// Macro slot, normalized value.
    Macro(usize, f32),
    MacroReleased(usize),
    /// Free param index, normalized value.
    Param(usize, f32),
    HotReload(bool),
}

pub fn view<'a>(patch: &'a Patch, values: &'a Values, hot_reload: bool) -> Element<'a, Message> {
    let mut list = column![].spacing(10).padding(Padding { top: 14.0, bottom: 14.0, left: 16.0, right: 16.0 });

    list = list.push(sheet::section_title("Macros · automatable in the host"));
    if patch.macros.is_empty() {
        list = list.push(muted("This patch has no macros."));
    }
    for (slot, m) in patch.macros.iter().enumerate() {
        let v = values.macros[slot];
        list = list.push(control(
            &m.name,
            patch.macro_text(slot, v),
            slider(0.0..=1.0, v, move |x| Message::Macro(slot, x))
                .step(0.001)
                .on_release(Message::MacroReleased(slot))
                .style(paper_slider)
                .into(),
        ));
    }

    if !patch.params.is_empty() {
        list = list.push(space::vertical().height(4));
        list = list.push(sheet::section_title("Params · saved with the project"));
    }
    for (i, spec) in patch.params.iter().enumerate() {
        let v = values.params.get(i).copied().unwrap_or(0.0);
        let widget: Element<'a, Message> = match patch.controls[i] {
            Control::Free => slider(0.0..=1.0, v, move |x| Message::Param(i, x)).step(0.001).style(paper_slider).into(),
            Control::Macro { slot, .. } => muted(format!("set by {}", patch.macros[slot].name)),
        };
        list = list.push(control(&spec.name, spec.display(v), widget));
    }

    list = list.push(space::vertical().height(4));
    list = list.push(sheet::section_title("Patch"));
    list = list.push(
        checkbox(hot_reload)
            .label("Reload when the patch files change")
            .on_toggle(Message::HotReload)
            .size(14)
            .text_size(12)
            .font(theme::font::PAPER)
            .style(paper_checkbox),
    );
    list = list.push(text(patch.dir.display().to_string()).size(11).font(theme::font::PAPER_MONO).color(theme::PAPER_MUTED));
    if let Some(a) = &patch.author {
        list = list.push(muted(format!("by {a}")));
    }

    sheet::frame(column![sheet::header("Controls", &patch.name, Message::Close), sheet::divider(), scrollable(list).height(Fill).direction(theme::thin_scrollbar()).style(theme::paper_scrollbar)])
}

fn control<'a>(name: &'a str, value: String, widget: Element<'a, Message>) -> Element<'a, Message> {
    column![
        row![
            text(name).size(12.5).font(theme::font::PAPER_SEMIBOLD).color(theme::PAPER_INK),
            space::horizontal(),
            text(value).size(11.5).font(theme::font::PAPER_MONO).color(theme::PAPER_INK_2),
        ]
        .align_y(Center),
        widget,
    ]
    .spacing(4)
    .into()
}

fn muted<'a>(s: impl text::IntoFragment<'a>) -> Element<'a, Message> {
    text(s).size(11.5).font(theme::font::PAPER).color(theme::PAPER_MUTED).into()
}

fn paper_slider(_: &Theme, status: slider::Status) -> slider::Style {
    let handle = match status {
        slider::Status::Hovered | slider::Status::Dragged => theme::PAPER_INK,
        _ => theme::PAPER_INK_2,
    };
    slider::Style {
        rail: slider::Rail {
            backgrounds: (Background::Color(theme::PAPER_INK), Background::Color(theme::PAPER_LINE)),
            width: 3.0,
            border: Border { radius: theme::radius(2.0), ..Default::default() },
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Circle { radius: 6.0 },
            background: Background::Color(theme::PAPER_RAISED),
            border_width: 2.0,
            border_color: handle,
        },
    }
}

fn paper_checkbox(_: &Theme, status: checkbox::Status) -> checkbox::Style {
    let checked = matches!(
        status,
        checkbox::Status::Active { is_checked: true } | checkbox::Status::Hovered { is_checked: true } | checkbox::Status::Disabled { is_checked: true }
    );
    checkbox::Style {
        background: if checked { theme::PAPER_INK.into() } else { theme::PAPER_RAISED.into() },
        icon_color: theme::PAPER,
        border: Border { radius: theme::radius(4.0), width: 1.0, color: if checked { theme::PAPER_INK } else { theme::PAPER_LINE } },
        text_color: Some(theme::PAPER_INK),
    }
}
