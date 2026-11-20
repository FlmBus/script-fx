//! The controls sheet (Settings in the notch): the patch's macros (host-automatable) and its
//! params, plus patch settings (hot reload, folder). Paper like the chat; the two share the
//! window's right side.

use nice_plug_iced::iced::{
    Border, Center, Element, Fill, Padding, Theme,
    widget::{checkbox, column, row, scrollable, slider, space, text},
};
use patch::{Patch, Values};

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
                .style(theme::paper_slider)
                .into(),
            None,
        ));
    }

    if !patch.params.is_empty() {
        list = list.push(space::vertical().height(4));
        list = list.push(sheet::section_title("Params · saved with the project"));
    }
    for (i, spec) in patch.params.iter().enumerate() {
        // The slider is the knob's base value; macros add their offsets around it.
        let v = values.base(i);
        let widget: Element<'a, Message> = slider(0.0..=1.0, v, move |x| Message::Param(i, x)).step(0.001).style(theme::paper_slider).into();
        // "tape · Amount": params are block knobs, the name alone is ambiguous.
        let label = match spec.id.split_once('.') {
            Some((block, _)) => format!("{block} · {}", spec.name),
            None => spec.name.clone(),
        };
        let mods = &patch.modulations[i];
        let note = (!mods.is_empty()).then(|| {
            let names: Vec<&str> = mods.iter().map(|m| patch.macros[m.slot].name.as_str()).collect();
            format!("modulated by {}", names.join(" + "))
        });
        list = list.push(control(label, spec.display(v), widget, note));
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
    list = list.push(text(patch.dir.display().to_string()).size(11).font(theme::font::PAPER_MONO).color(theme::c().paper_muted));
    if let Some(a) = &patch.author {
        list = list.push(muted(format!("by {a}")));
    }

    sheet::frame(column![sheet::header("Controls", &patch.name, Message::Close), sheet::divider(), scrollable(list).height(Fill).direction(theme::thin_scrollbar()).style(theme::paper_scrollbar)])
}

fn control<'a>(name: impl text::IntoFragment<'a>, value: String, widget: Element<'a, Message>, note: Option<String>) -> Element<'a, Message> {
    let mut col = column![
        row![
            text(name).size(12.5).font(theme::font::PAPER_SEMIBOLD).color(theme::c().paper_ink),
            space::horizontal(),
            text(value).size(11.5).font(theme::font::PAPER_MONO).color(theme::c().paper_ink_2),
        ]
        .align_y(Center),
        widget,
    ]
    .spacing(4);
    if let Some(note) = note {
        col = col.push(muted(note));
    }
    col.into()
}

fn muted<'a>(s: impl text::IntoFragment<'a>) -> Element<'a, Message> {
    text(s).size(11.5).font(theme::font::PAPER).color(theme::c().paper_muted).into()
}

fn paper_checkbox(_: &Theme, status: checkbox::Status) -> checkbox::Style {
    let checked = matches!(
        status,
        checkbox::Status::Active { is_checked: true } | checkbox::Status::Hovered { is_checked: true } | checkbox::Status::Disabled { is_checked: true }
    );
    checkbox::Style {
        background: if checked { theme::c().paper_ink.into() } else { theme::c().paper_raised.into() },
        icon_color: theme::c().paper,
        border: Border { radius: theme::radius(4.0), width: 1.0, color: if checked { theme::c().paper_ink } else { theme::c().paper_line } },
        text_color: Some(theme::c().paper_ink),
    }
}
