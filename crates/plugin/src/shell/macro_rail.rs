//! The macro rail: the patch's macros in a sidebar on the left, so they can be tweaked in the
//! panel view as well as in the chain view. (They used to live in the drawer; the drawer keeps
//! the modulators.)
//!
//! Six macros per page in a 2×3 grid, like the drawer had. Changing pages slides the grid
//! sideways (220 ms, ease-out): forward, the old page leaves left and the new one enters from
//! the right; backward, the other way around. The page dots and arrows are below the grid.
//! Stateless apart from the page: the shell owns whether the rail is open and slides it in.

use std::time::Instant;

use super::{icons, knob, theme};
use nice_plug_iced::iced::{
    Border, Center, Element, Fill, Shadow, Theme, Vector,
    mouse,
    widget::{button, column, container, mouse_area, row, space, stack, text},
};

/// The rail's content width (the shell adds its margin).
pub const WIDTH: f32 = 2.0 * knob::MACRO_SIZE.width + GRID_GAP_X + 2.0 * PAD;

const PAD: f32 = 12.0;
/// Macros per page: 2 columns × 3 rows.
const PAGE: usize = 6;
const GRID_GAP_X: f32 = 6.0;
const GRID_GAP_Y: f32 = 4.0;
const PAGE_MS: u64 = 220;

pub struct Rail {
    page: usize,
    /// The page sliding out, and when the slide started.
    slide: Option<(usize, Instant)>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Page(usize),
    /// Input on a macro knob (slot).
    Macro(usize, knob::KnobEvent),
    Close,
}

impl From<(usize, knob::KnobEvent)> for Message {
    fn from((slot, e): (usize, knob::KnobEvent)) -> Self {
        Message::Macro(slot, e)
    }
}

/// What the shell does for the rail.
pub enum Event {
    Macro(usize, knob::KnobEvent),
    Close,
}

impl Rail {
    pub fn new() -> Self {
        Self { page: 0, slide: None }
    }

    pub fn update(&mut self, message: Message, now: Instant) -> Option<Event> {
        match message {
            Message::Page(p) => {
                if p != self.page {
                    self.slide = Some((self.page, now));
                    self.page = p;
                }
            }
            Message::Macro(slot, e) => return Some(Event::Macro(slot, e)),
            Message::Close => return Some(Event::Close),
        }
        None
    }

    /// 0..1, ease-out, since the last page change.
    fn page_t(&self, now: Instant) -> f32 {
        let Some((_, start)) = self.slide else { return 1.0 };
        let t = now.saturating_duration_since(start).as_secs_f32() / (PAGE_MS as f32 / 1000.0);
        let t = t.clamp(0.0, 1.0);
        1.0 - (1.0 - t).powi(3)
    }

    pub fn view<'a>(&'a self, now: Instant, macros: Vec<knob::Macro<'a>>) -> Element<'a, Message> {
        let title = row![
            text("Macros").size(12.5).font(theme::font::SANS_SEMIBOLD).color(theme::c().frame_ink),
            space::horizontal(),
            button(container(icons::icon(icons::CLOSE, 10.0, 2.2, theme::c().frame_ink_2)).center(Fill))
                .on_press(Message::Close)
                .width(22)
                .height(22)
                .padding(0)
                .style(theme::frame_ghost(false)),
        ]
        .align_y(Center);

        let body: Element<'a, Message> = if macros.is_empty() {
            container(text("This patch has no macros.").size(11).color(theme::c().frame_muted)).center_x(Fill).padding(16).into()
        } else {
            let pages = macros.len().div_ceil(PAGE);
            let page = self.page.min(pages - 1);
            let grid = self.grid(now, &macros, page);
            column![grid, pager(page, pages)].spacing(8).align_x(Center).into()
        };

        // As tall as its content: a card under the top bar, not a full-height panel. `Idle` marks
        // it as occupied, so a click on its blank space doesn't reach the page (which would close it).
        let card = container(column![title, body].spacing(10))
            .width(WIDTH)
            .padding(PAD)
            .style(|_: &Theme| container::Style {
                background: Some(theme::c().frame_raised.into()),
                border: Border { radius: theme::radius(theme::RADIUS_WINDOW), width: 1.0, color: theme::c().frame_hairline_strong },
                shadow: Shadow { color: theme::shadow(0.4), offset: Vector::new(0.0, 8.0), blur_radius: 24.0 },
                ..Default::default()
            });
        mouse_area(card).interaction(mouse::Interaction::Idle).into()
    }

    /// The current page's 2×3 grid; while a page change runs, the old and new grids slide.
    fn grid<'a>(&'a self, now: Instant, macros: &[knob::Macro<'a>], page: usize) -> Element<'a, Message> {
        let height = 3.0 * knob::MACRO_SIZE.height + 2.0 * GRID_GAP_Y;
        let t = self.page_t(now);
        let Some((from, _)) = self.slide else {
            return self.page_grid(macros, page).into();
        };
        if t >= 1.0 {
            return self.page_grid(macros, page).into();
        }
        let dx = t * (WIDTH - 2.0 * PAD);
        // Forward (page > from): old leaves left, new enters from the right.
        let forward = page > from;
        let (old_x, new_x) = if forward { (-dx, WIDTH - 2.0 * PAD - dx) } else { (dx, dx - (WIDTH - 2.0 * PAD)) };
        let old = self.page_grid(macros, from);
        let new = self.page_grid(macros, page);
        container(stack![
            nice_plug_iced::iced::widget::pin(old).x(old_x),
            nice_plug_iced::iced::widget::pin(new).x(new_x),
        ])
        .width(WIDTH - 2.0 * PAD)
        .height(height)
        .clip(true)
        .into()
    }

    /// One page of up to six macro cells, padded with empty cells.
    fn page_grid<'a>(&'a self, macros: &[knob::Macro<'a>], page: usize) -> Element<'a, Message> {
        let mut cells: Vec<Element<'a, Message>> = macros
            .iter()
            .skip(page * PAGE)
            .take(PAGE)
            .map(|m| {
                knob::macro_cell(knob::Macro {
                    slot: m.slot,
                    label: m.label,
                    value_text: m.value_text.clone(),
                    value: m.value,
                    default: m.default,
                })
            })
            .collect();
        while cells.len() < PAGE {
            cells.push(empty_cell());
        }
        let mut cells = cells.into_iter();
        let mut grid = column![].spacing(GRID_GAP_Y);
        for _ in 0..PAGE / 2 {
            grid = grid.push(row![cells.next().unwrap(), cells.next().unwrap()].spacing(GRID_GAP_X));
        }
        grid.into()
    }
}

/// A grid position without a macro.
fn empty_cell<'a>() -> Element<'a, Message> {
    container(space::horizontal().width(knob::MACRO_SIZE.width).height(knob::MACRO_SIZE.height))
        .style(|_: &Theme| container::Style {
            background: Some(theme::alpha(theme::c().frame_ink, 0.015).into()),
            border: Border { radius: theme::radius(8.0), ..Default::default() },
            ..Default::default()
        })
        .into()
}

/// ‹ · ● · › : the current page as a wide pill, the others as dots (clickable).
fn pager<'a>(page: usize, pages: usize) -> Element<'a, Message> {
    let ink = theme::c().frame_ink;
    let arrow = |degrees: f32, to: Option<usize>| -> Element<'a, Message> {
        let colour = if to.is_some() { theme::c().frame_ink_2 } else { theme::alpha(theme::c().frame_ink_2, 0.3) };
        button(container(icons::icon_rotated(icons::CHEVRON_RIGHT, 10.0, 2.5, colour, degrees)).center(Fill))
            .on_press_maybe(to.map(Message::Page))
            .width(16)
            .height(16)
            .padding(0)
            .style(theme::frame_ghost(false))
            .into()
    };
    let dot = |i: usize| -> Element<'a, Message> {
        let on = i == page;
        let pill = container(space::horizontal().width(if on { 12 } else { 5 }).height(5)).style(move |_: &Theme| container::Style {
            background: Some(theme::alpha(ink, if on { 1.0 } else { 0.22 }).into()),
            border: Border { radius: theme::radius(3.0), ..Default::default() },
            ..Default::default()
        });
        button(container(pill).center(Fill)).on_press(Message::Page(i)).width(if on { 16 } else { 9 }).height(16).padding(0).style(|_: &Theme, _| button::Style::default()).into()
    };
    row![
        arrow(180.0, page.checked_sub(1)),
        row((0..pages).map(dot)).align_y(Center),
        arrow(0.0, (page + 1 < pages).then_some(page + 1)),
    ]
    .spacing(4)
    .align_y(Center)
    .height(16)
    .into()
}
