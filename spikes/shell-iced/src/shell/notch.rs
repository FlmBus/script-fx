//! The control notch: a tab of frame material hanging from the window's top edge, joined to
//! it by concave flares. Stateless: the shell owns everything it shows.

use super::{icons, mock, theme};
use nice_plug_iced::iced::{
    Border, Center, Element, Fill, Padding, Point, Radians, Rectangle, Renderer, Shadow, Theme, Top, Vector,
    mouse,
    widget::{button, canvas, column, container, row, space, text},
};
use std::f32::consts::{FRAC_PI_2, PI};

#[derive(Debug, Clone)]
pub enum Message {
    NextSnapshot,
    Save,
    Load,
    Reload,
    ToggleChat,
    Settings,
}

pub fn view<'a>(patch: &'static mock::Patch, snapshot: usize, chat_open: bool) -> Element<'a, Message> {
    let ink = theme::FRAME_INK_2;

    let selector = button(
        container(
            row![
                text(patch.snapshots[snapshot]).size(12).font(theme::font::SANS_MEDIUM).color(theme::FRAME_INK),
                space::horizontal(),
                icons::icon(icons::CHEVRON_DOWN, 10.0, 2.5, theme::FRAME_MUTED),
            ]
            .align_y(Center),
        )
        .center_y(Fill),
    )
    .on_press(Message::NextSnapshot)
    .width(206)
    .height(theme::SIZE_CONTROL)
    .padding(Padding { left: 12.0, right: 10.0, ..Padding::ZERO })
    .style(theme::frame_ghost(true));

    let row1 = row![
        selector,
        square(icons::SAVE, Message::Save),
        square(icons::FOLDER, Message::Load),
    ]
    .spacing(theme::SPACE_HAIR)
    .align_y(Center);

    let sync_led = container(space::horizontal().width(5).height(5)).style(|_: &Theme| container::Style {
        background: Some(theme::LED_OK.into()),
        border: Border { radius: theme::radius(3.0), ..Default::default() },
        ..Default::default()
    });
    let reload = wide(row![icons::icon(icons::RELOAD, 12.0, 2.2, ink), sync_led].spacing(6).align_y(Center), Message::Reload, false);
    let chat_ink = if chat_open { theme::ON_FRAME_INVERT } else { ink };
    let chat_font = if chat_open { theme::font::SANS_SEMIBOLD } else { theme::font::SANS };
    let chat = wide(
        row![icons::icon(icons::CHAT, 12.0, 2.2, chat_ink), text("Chat").size(12).font(chat_font)].spacing(6).align_y(Center),
        Message::ToggleChat,
        chat_open,
    );
    let settings = wide(icons::icon(icons::SETTINGS, 12.0, 2.2, ink), Message::Settings, false);

    let row2 = row![reload, chat, settings].spacing(theme::SPACE_HAIR).align_y(Center);

    let body = container(column![row1, row2].spacing(3).align_x(Center))
        .padding(Padding { top: 5.0, right: 8.0, bottom: 8.0, left: 8.0 })
        .style(|_: &Theme| container::Style {
            background: Some(theme::FRAME.into()),
            border: Border {
                radius: theme::radius(0.0).bottom(theme::RADIUS_NOTCH),
                ..Default::default()
            },
            // shadow-notch: 0 10px 24px -6px rgba(0,0,0,0.45)
            shadow: Shadow { color: theme::alpha(theme::FRAME, 0.45), offset: Vector::new(0.0, 10.0), blur_radius: 24.0 },
            ..Default::default()
        });

    row![flare(Side::Left), body, flare(Side::Right)].align_y(Top).into()
}

fn square<'a>(icon: &str, msg: Message) -> Element<'a, Message> {
    button(container(icons::icon(icon, 13.0, 2.0, theme::FRAME_INK_2)).center(Fill))
        .on_press(msg)
        .width(theme::SIZE_CONTROL)
        .height(theme::SIZE_CONTROL)
        .padding(0)
        .style(theme::frame_ghost(false))
        .into()
}

fn wide<'a>(content: impl Into<Element<'a, Message>>, msg: Message, primary: bool) -> Element<'a, Message> {
    let b = button(container(content).center_y(Fill)).on_press(msg).height(28).padding(Padding { left: 11.0, right: 11.0, ..Padding::ZERO });
    if primary { b.style(theme::frame_primary) } else { b.style(theme::frame_ghost(false)) }.into()
}

// ---- concave flares -----------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Side {
    Left,
    Right,
}

fn flare<'a>(side: Side) -> Element<'a, Message> {
    canvas(Flare(side)).width(theme::RADIUS_WINDOW).height(theme::RADIUS_WINDOW).into()
}

struct Flare(Side);

impl canvas::Program<Message> for Flare {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<canvas::Geometry> {
        let r = bounds.width;
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        // A square of frame colour minus a quarter circle: the frame curves into the window edge.
        let path = canvas::Path::new(|p| match self.0 {
            Side::Left => {
                p.move_to(Point::new(0.0, 0.0));
                p.line_to(Point::new(r, 0.0));
                p.line_to(Point::new(r, r));
                p.arc(canvas::path::Arc { center: Point::new(0.0, r), radius: r, start_angle: Radians(0.0), end_angle: Radians(-FRAC_PI_2) });
                p.close();
            }
            Side::Right => {
                p.move_to(Point::new(r, 0.0));
                p.line_to(Point::new(0.0, 0.0));
                p.line_to(Point::new(0.0, r));
                p.arc(canvas::path::Arc { center: Point::new(r, r), radius: r, start_angle: Radians(PI), end_angle: Radians(PI + FRAC_PI_2) });
                p.close();
            }
        });
        frame.fill(&path, theme::FRAME);
        vec![frame.into_geometry()]
    }
}
