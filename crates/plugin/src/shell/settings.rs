//! The settings sheet (gear in the top bar): the user's and the instance's own settings, not
//! the patch's. Appearance (theme, interface size) is per user and saved in settings.json;
//! passive mode is per instance and not built yet, so its switch is shown disabled.
//! Stateless: the shell owns everything it shows.

use nice_plug_iced::iced::{
    Border, Center, Element, Fill, Padding, Theme,
    alignment::Horizontal,
    widget::{button, column, container, row, pick_list, scrollable, space, text, text_input},
};

use super::{icons, sheet, tabs, theme};
use crate::assistant::Connection;
use crate::captures;
use crate::user_settings::{self, Ai, Provider, ThemeMode};

#[derive(Debug, Clone)]
pub enum Message {
    Close,
    SetTheme(ThemeMode),
    ScaleDown,
    ScaleUp,
    TogglePassive,
    /// Delete every stored capture (audio + analyses).
    ClearCaptures,
    SetProvider(Provider),
    Ai(Field, String),
    /// Enter in a field: check the connection again.
    Probe,
    SaveKey,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Field {
    Model,
    BaseUrl,
    ClaudeDir,
    /// Only held until it's saved to the keychain.
    Key,
}

pub struct Props<'a> {
    pub ai: &'a Ai,
    pub ai_key: &'a str,
    pub ai_connection: Connection,
    pub theme: ThemeMode,
    /// 1.0 = 100 %.
    pub ui_scale: f32,
    pub passive: bool,
    /// False while passive mode isn't implemented.
    pub passive_available: bool,
    /// What the captures folder holds (read when the sheet opens, not per frame).
    pub captures: captures::Usage,
}

pub fn view<'a>(p: Props<'a>) -> Element<'a, Message> {
    // The newest `KEEP` captures are kept anyway; this is for getting the space back now.
    let captures_hint = format!("{} · the audio of each Listen, kept for the last {}", p.captures.label(), captures::KEEP);
    let passive_hint = if p.passive_available {
        "don't process; share this channel's audio with the other ScriptFX instances"
    } else {
        "coming later: don't process; share this channel's audio with the other ScriptFX instances, e.g. on the master"
    };
    let list = column![
        sheet::section_title("Appearance · for you, in every project"),
        setting("Theme", "Auto follows your computer's setting", theme_switch(p.theme)),
        setting("Interface size", "resizes the plugin window", scale(p.ui_scale)),
        space::vertical().height(4),
        ai(&p),
        space::vertical().height(4),
        sheet::section_title("This instance"),
        setting("Passive listener", passive_hint, passive(&p)),
        space::vertical().height(4),
        sheet::section_title("Listen · on this computer"),
        setting("Stored captures", captures_hint, clear_captures(&p)),
    ]
    .spacing(14)
    .padding(Padding { top: 14.0, bottom: 14.0, left: 16.0, right: 16.0 });

    sheet::frame(column![
        sheet::header("Settings", "ScriptFX", Message::Close),
        sheet::divider(),
        scrollable(list).height(Fill).direction(theme::thin_scrollbar()).style(theme::paper_scrollbar),
    ])
}

/// Who answers in the chat, and its connection.
fn ai<'a>(p: &Props<'a>) -> Element<'a, Message> {
    let segments = row(Provider::ALL.into_iter().map(|v| {
        let label = match v {
            Provider::Off => "Off",
            Provider::Claude => "Claude Code",
            Provider::Api => "API",
        };
        button(container(text(label).size(11.5).font(theme::font::PAPER)).center_y(Fill))
            .on_press(Message::SetProvider(v))
            .height(24)
            .padding(Padding { left: 9.0, right: 9.0, ..Padding::ZERO })
            .style(segment_style(v == p.ai.provider))
            .into()
    }))
    .spacing(2);
    let hint = match p.ai.provider {
        Provider::Off => "nothing is sent anywhere",
        Provider::Claude => "uses your installed Claude Code and its login",
        Provider::Api => "any OpenAI-compatible API or local server",
    };
    let mut list = column![sheet::section_title("AI · for you, in every project"), track(segments.into())].spacing(9);
    list = list.push(text(hint).size(11).font(theme::font::PAPER).color(theme::c().paper_muted));
    match p.ai.provider {
        Provider::Off => return list.into(),
        Provider::Claude => {
            list = list
                .push(model("Model (empty: your default)", &p.ai.claude_model, &p.ai_connection))
                .push(field("Config folder (empty: default)", &p.ai.claude_config_dir, Field::ClaudeDir));
        }
        Provider::Api => {
            list = list.push(field("Address", &p.ai.base_url, Field::BaseUrl)).push(model("Model", &p.ai.api_model, &p.ai_connection)).push(
                row![
                    field("Paste a new key", p.ai_key, Field::Key),
                    sheet::small(sheet::label(if p.ai_key.is_empty() { "Remove key" } else { "Save key" }), Some(Message::SaveKey)),
                ]
                .spacing(6)
                .align_y(Center),
            );
        }
    }
    list.push(text(p.ai_connection.status.clone()).size(11).font(theme::font::PAPER).color(theme::c().paper_ink_2))
        .push(
            text("Each message sends the chat, the loaded patch, its values and an attached analysis. Never audio.")
                .size(11)
                .font(theme::font::PAPER)
                .color(theme::c().paper_muted),
        )
        .into()
}

/// An entry of the model list.
#[derive(Clone, PartialEq)]
struct Choice(llm::Model);

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0.name)
    }
}

/// The models the provider has; typed by hand while that isn't known.
fn model<'a>(placeholder: &'a str, value: &'a str, c: &Connection) -> Element<'a, Message> {
    if c.models.is_empty() {
        return field(placeholder, value, Field::Model);
    }
    let mut choices: Vec<Choice> = c.models.iter().cloned().map(Choice).collect();
    let mut picked = choices.iter().find(|m| m.0.id == value).cloned();
    // A saved model the list doesn't have stays picked (and visible).
    if picked.is_none() && !value.is_empty() {
        let saved = Choice(llm::Model { id: value.into(), name: format!("{value} (not listed)") });
        choices.insert(0, saved.clone());
        picked = Some(saved);
    }
    pick_list(choices, picked, |m: Choice| Message::Ai(Field::Model, m.0.id))
        .placeholder("Pick a model")
        .width(Fill)
        .menu_height(200)
        .text_size(12)
        .font(theme::font::PAPER)
        .padding(Padding { top: 6.0, bottom: 6.0, left: 9.0, right: 9.0 })
        .style(theme::paper_pick_list)
        .menu_style(theme::paper_menu)
        .into()
}

fn field<'a>(placeholder: &'a str, value: &'a str, f: Field) -> Element<'a, Message> {
    let c = theme::c();
    text_input(placeholder, value)
        .on_input(move |s| Message::Ai(f, s))
        .on_submit(if f == Field::Key { Message::SaveKey } else { Message::Probe })
        .secure(f == Field::Key)
        .size(12)
        .font(theme::font::PAPER)
        .padding(Padding { top: 6.0, bottom: 6.0, left: 9.0, right: 9.0 })
        .style(move |_: &Theme, status| text_input::Style {
            background: c.paper_raised.into(),
            border: Border {
                radius: theme::radius(theme::RADIUS_BUTTON),
                width: 1.0,
                color: if matches!(status, text_input::Status::Focused { .. }) { c.paper_muted } else { c.paper_line },
            },
            icon: c.paper_muted,
            placeholder: c.paper_muted,
            value: c.paper_ink,
            selection: theme::alpha(c.signal, 0.3),
        })
        .into()
}

/// Name and hint on the left, the control on the right. The hint may be owned (built per view).
fn setting<'a>(
    name: &'a str,
    hint: impl Into<std::borrow::Cow<'a, str>>,
    control: Element<'a, Message>,
) -> Element<'a, Message> {
    let hint = hint.into();
    row![
        column![
            text(name).size(12.5).font(theme::font::PAPER_SEMIBOLD).color(theme::c().paper_ink),
            text(hint).size(11).font(theme::font::PAPER).color(theme::c().paper_muted),
        ]
        .spacing(2)
        .width(Fill),
        control,
    ]
    .spacing(12)
    .align_y(Center)
    .into()
}

// ---- controls on paper ---------------------------------------------------------------------------

/// A row of segments in a paper-raised track.
fn track<'a>(content: Element<'a, Message>) -> Element<'a, Message> {
    container(content)
        .padding(2)
        .style(|_: &Theme| container::Style {
            background: Some(theme::c().paper_raised.into()),
            border: Border { radius: theme::radius(theme::RADIUS_BUTTON), width: 1.0, color: theme::c().paper_line },
            ..Default::default()
        })
        .into()
}

/// A segment inside a track: filled with paper-ink when `on`, a light wash on hover.
fn segment_style(on: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let c = theme::c();
        let hover = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: if on {
                Some(c.paper_ink.into())
            } else if hover {
                Some(theme::alpha(c.paper_ink, 0.06).into())
            } else {
                None
            },
            text_color: if on { c.on_paper_ink } else { c.paper_ink },
            border: Border { radius: theme::radius(theme::RADIUS_BUTTON - 2.0), ..Default::default() },
            ..Default::default()
        }
    }
}

/// Auto | Light | Dark, with the highlight sliding between them.
fn theme_switch<'a>(mode: ThemeMode) -> Element<'a, Message> {
    let segment = |m: ThemeMode, on: bool| -> Element<'a, Message> {
        let (icon, label) = match m {
            ThemeMode::System => (icons::AUTO, "Auto"),
            ThemeMode::Light => (icons::SUN, "Light"),
            ThemeMode::Dark => (icons::MOON, "Dark"),
        };
        let ink = if on { theme::c().on_paper_ink } else { theme::c().paper_ink };
        button(
            container(row![icons::icon(icon, 12.0, 2.0, ink), text(label).size(11.5).font(theme::font::PAPER).color(ink)].spacing(5).align_y(Center))
                .center_y(Fill),
        )
        .on_press(Message::SetTheme(m))
        .height(24)
        .padding(Padding { left: 8.0, right: 9.0, ..Padding::ZERO })
        .style(tabs::style(on, segment_style(on)))
        .into()
    };
    let off = row(ThemeMode::ALL.into_iter().map(|m| segment(m, false))).spacing(2);
    let on = row(ThemeMode::ALL.into_iter().map(|m| segment(m, true))).spacing(2);
    let selected = ThemeMode::ALL.iter().position(|m| *m == mode);
    let pill = tabs::Pill {
        background: theme::c().paper_ink,
        radius: theme::RADIUS_BUTTON - 2.0,
        shadow: Default::default(),
    };
    track(tabs::tabs(off, on, selected, pill).into())
}

/// − 100 % +
fn scale<'a>(ui_scale: f32) -> Element<'a, Message> {
    let step = |icon: &'static str, msg: Message, enabled: bool| -> Element<'a, Message> {
        let ink = if enabled { theme::c().paper_ink } else { theme::alpha(theme::c().paper_muted, 0.6) };
        button(container(icons::icon(icon, 12.0, 2.2, ink)).center(Fill))
            .on_press_maybe(enabled.then_some(msg))
            .width(24)
            .height(24)
            .padding(0)
            .style(segment_style(false))
            .into()
    };
    let value = text(format!("{:.0} %", ui_scale * 100.0))
        .size(11.5)
        .font(theme::font::PAPER_MONO)
        .color(theme::c().paper_ink)
        .width(46)
        .align_x(Horizontal::Center);
    track(
        row![
            step(icons::MINUS, Message::ScaleDown, ui_scale > user_settings::UI_SCALE_MIN),
            value,
            step(icons::PLUS, Message::ScaleUp, ui_scale < user_settings::UI_SCALE_MAX),
        ]
        .align_y(Center)
        .into(),
    )
}

/// Deletes every stored capture. Disabled (and silent) when there is nothing to delete, so it
/// can't be pressed hopefully; the hint next to it says how much there is.
fn clear_captures<'a>(p: &Props) -> Element<'a, Message> {
    let empty = p.captures.captures == 0;
    let c = theme::c();
    let label = text("Clear").size(12).font(theme::font::PAPER_SEMIBOLD).color(if empty { c.paper_muted } else { c.paper_ink });
    button(container(label).padding(Padding { top: 5.0, bottom: 5.0, left: 11.0, right: 11.0 }))
        .on_press_maybe((!empty).then_some(Message::ClearCaptures))
        .padding(0)
        .style(move |_: &Theme, status: button::Status| button::Style {
            background: Some(match status {
                button::Status::Hovered if !empty => c.paper_line.into(),
                _ => c.paper_raised.into(),
            }),
            border: Border { radius: theme::radius(theme::RADIUS_BUTTON), width: 1.0, color: c.paper_line },
            ..Default::default()
        })
        .into()
}

/// A pill switch; disabled (dimmed knob, no press) while passive mode isn't built.
fn passive<'a>(p: &Props<'_>) -> Element<'a, Message> {
    let on = p.passive && p.passive_available;
    let c = theme::c();
    let knob_color = if !p.passive_available { theme::alpha(c.paper_muted, 0.6) } else if on { c.on_paper_ink } else { c.paper_ink };
    let track_color = if on { c.led_ok } else { c.paper_raised };
    let knob = container(space::horizontal().width(12).height(12)).style(move |_: &Theme| container::Style {
        background: Some(knob_color.into()),
        border: Border { radius: theme::radius(6.0), ..Default::default() },
        ..Default::default()
    });
    let switch = container(knob)
        .width(30)
        .height(18)
        .padding(2)
        .align_x(if on { Horizontal::Right } else { Horizontal::Left })
        .center_y(18)
        .style(move |_: &Theme| container::Style {
            background: Some(track_color.into()),
            border: Border { radius: theme::radius(9.0), width: 1.0, color: c.paper_line },
            ..Default::default()
        });
    button(switch)
        .on_press_maybe(p.passive_available.then_some(Message::TogglePassive))
        .padding(0)
        .style(|_: &Theme, _| button::Style::default())
        .into()
}
