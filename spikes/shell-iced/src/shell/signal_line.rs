//! The signal line under the window: IN, nodes joined by a 1px wire, parallel lanes for
//! splits, OUT. View only; the patch provides nodes, labels and LED states.

use super::{mock, theme};
use nice_plug_iced::iced::{
    Border, Center, Color, Element, Fill, FillPortion, Length, Padding, Theme,
    widget::{Column, Row, column, container, row, space, text},
};

const LANE_H: f32 = 20.0;
const LANE_GAP: f32 = 2.0;

pub fn view<'a, M: 'a>(patch: &'static mock::Patch) -> Element<'a, M> {
    let mut line = Row::new().push(label("IN").padding(Padding { right: 8.0, ..Padding::ZERO })).align_y(Center);
    for link in patch.chain {
        line = line.push(wire(Fill));
        line = line.push(match link {
            mock::Link::Node(name, led) => node(name, *led),
            mock::Link::Split(lanes) => split(lanes),
        });
    }
    line = line.push(wire(Fill)).push(label("OUT").padding(Padding { left: 8.0, ..Padding::ZERO }));

    container(line)
        .height(theme::SIZE_SIGNAL_LINE)
        .width(Fill)
        .padding(Padding { left: 4.0, right: 16.0 + theme::SPACE_GUTTER, ..Padding::ZERO })
        .align_y(Center)
        .into()
}

fn label<'a, M: 'a>(s: &'a str) -> container::Container<'a, M> {
    // label-caps; iced text has no letter-spacing yet.
    container(text(s).size(10).font(theme::font::SANS_SEMIBOLD).color(theme::FRAME_SUBTLE))
}

fn wire<'a, M: 'a>(width: impl Into<Length>) -> Element<'a, M> {
    container(space::horizontal().width(width).height(1)).style(theme::rule(theme::FRAME_WIRE)).into()
}

fn vbar<'a, M: 'a>() -> Element<'a, M> {
    // From the centre of the first lane to the centre of the last.
    container(container(space::vertical().width(1)).style(theme::rule(theme::FRAME_WIRE)))
        .padding(Padding { top: LANE_H / 2.0, bottom: LANE_H / 2.0, ..Padding::ZERO })
        .height(Fill)
        .into()
}

fn led_color(led: mock::Led) -> Color {
    match led {
        mock::Led::Ok => theme::LED_OK,
        mock::Led::Reduce => theme::LED_REDUCE,
        mock::Led::Clip => theme::LED_CLIP,
    }
}

fn node<'a, M: 'a>(name: &'a str, led: mock::Led) -> Element<'a, M> {
    let c = led_color(led);
    let dot = container(space::horizontal().width(5).height(5)).style(move |_: &Theme| container::Style {
        background: Some(c.into()),
        border: Border { radius: theme::radius(3.0), ..Default::default() },
        ..Default::default()
    });
    container(row![dot, text(name).size(11).color(theme::FRAME_INK_3)].spacing(6).align_y(Center))
        .height(theme::SIZE_NODE)
        .padding(Padding { left: 10.0, right: 10.0, ..Padding::ZERO })
        .align_y(Center)
        .style(|_: &Theme| container::Style {
            background: Some(theme::FRAME_NODE.into()),
            border: Border { radius: theme::radius(theme::RADIUS_CONTROL), width: 1.0, color: theme::FRAME_HAIRLINE_STRONG },
            ..Default::default()
        })
        .into()
}

fn split<'a, M: 'a>(lanes: &'static [&'static [(&'static str, mock::Led)]]) -> Element<'a, M> {
    let lanes_col: Column<'a, M> = column(lanes.iter().map(|lane| {
        let mut r = Row::new().push(wire(10)).height(LANE_H).align_y(Center);
        for (name, led) in lane.iter() {
            r = r.push(wire(Fill)).push(node(name, *led));
        }
        r.push(wire(Fill)).into()
    }))
    .spacing(LANE_GAP)
    .width(Fill);

    let h = lanes.len() as f32 * LANE_H + (lanes.len() as f32 - 1.0) * LANE_GAP;
    row![vbar(), lanes_col, vbar()].height(h).width(FillPortion(4)).into()
}
