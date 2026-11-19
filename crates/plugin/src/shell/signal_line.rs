//! The signal line under the window: IN, nodes joined by a 1px wire, parallel lanes for
//! splits, OUT. Built from the patch's chain: block ids are the labels, routing blocks
//! (band split, parallel, mid/side, left/right) become lanes, metered blocks light their LED
//! amber while they work (compressors reducing, riders/transient shapers moving gain).

use patch::chain::{BlockKind, BlockSpec};
use patch::{Patch, Values};

use super::theme;
use nice_plug_iced::iced::{
    Border, Center, Color, Element, Fill, FillPortion, Length, Padding, Theme,
    widget::{Column, Row, column, container, row, space, text},
};

const LANE_H: f32 = 20.0;
const LANE_GAP: f32 = 2.0;

#[derive(Clone, Copy)]
enum Led {
    Ok,
    Reduce,
}

/// Empty `patch` = nothing loaded: just IN → OUT.
pub fn view<'a, M: 'a>(patch: Option<&'a Patch>, values: &Values) -> Element<'a, M> {
    let led = |b: &BlockSpec| -> Led {
        let (Some(idle), Some(patch)) = (b.kind.idle_meter(), patch) else { return Led::Ok };
        let v = patch.meters.iter().position(|m| *m == b.id).and_then(|i| values.blocks.get(i)).copied();
        match v {
            Some(v) if (v - idle).abs() > 1.0 / 24.0 && !matches!(b.kind, BlockKind::Meter) => Led::Reduce,
            _ => Led::Ok,
        }
    };
    let mut line = Row::new().push(label("IN").padding(Padding { right: 8.0, ..Padding::ZERO })).align_y(Center);
    for b in patch.map(|p| p.chain.as_slice()).unwrap_or_default() {
        line = line.push(wire(Fill));
        line = line.push(match lanes_of(&b.kind) {
            Some(lanes) => split(lanes.iter().map(|lane| flatten(lane).into_iter().map(|b| (b.id.as_str(), led(b))).collect()).collect()),
            None => node(&b.id, led(b)),
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
    container(text(s).size(10).font(theme::font::SANS_SEMIBOLD).color(theme::c().frame_subtle))
}

fn wire<'a, M: 'a>(width: impl Into<Length>) -> Element<'a, M> {
    container(space::horizontal().width(width).height(1)).style(theme::rule(theme::c().frame_wire)).into()
}

fn vbar<'a, M: 'a>() -> Element<'a, M> {
    // From the centre of the first lane to the centre of the last.
    container(container(space::vertical().width(1)).style(theme::rule(theme::c().frame_wire)))
        .padding(Padding { top: LANE_H / 2.0, bottom: LANE_H / 2.0, ..Padding::ZERO })
        .height(Fill)
        .into()
}

/// The parallel chains of a routing block.
fn lanes_of(kind: &BlockKind) -> Option<Vec<&[BlockSpec]>> {
    Some(match kind {
        BlockKind::BandSplit { bands, .. } => bands.iter().map(|c| c.0.as_slice()).collect(),
        BlockKind::Parallel { branches, .. } => branches.iter().map(|c| c.0.as_slice()).collect(),
        BlockKind::MidSide { mid, side } => vec![mid.0.as_slice(), side.0.as_slice()],
        BlockKind::LeftRight { left, right } => vec![left.0.as_slice(), right.0.as_slice()],
        _ => return None,
    })
}

/// A lane's blocks in order; nested routing blocks are flattened into their inner blocks.
fn flatten(chain: &[BlockSpec]) -> Vec<&BlockSpec> {
    let mut out = Vec::new();
    for b in chain {
        match lanes_of(&b.kind) {
            Some(lanes) => lanes.into_iter().for_each(|l| out.extend(flatten(l))),
            None => out.push(b),
        }
    }
    out
}

fn led_color(led: Led) -> Color {
    match led {
        Led::Ok => theme::c().led_ok,
        Led::Reduce => theme::c().led_reduce,
    }
}

fn node<'a, M: 'a>(name: &'a str, led: Led) -> Element<'a, M> {
    let c = led_color(led);
    let dot = container(space::horizontal().width(5).height(5)).style(move |_: &Theme| container::Style {
        background: Some(c.into()),
        border: Border { radius: theme::radius(3.0), ..Default::default() },
        ..Default::default()
    });
    container(row![dot, text(name).size(11).color(theme::c().frame_ink_3)].spacing(6).align_y(Center))
        .height(theme::SIZE_NODE)
        .padding(Padding { left: 10.0, right: 10.0, ..Padding::ZERO })
        .align_y(Center)
        .style(|_: &Theme| container::Style {
            background: Some(theme::c().frame_node.into()),
            border: Border { radius: theme::radius(theme::RADIUS_CONTROL), width: 1.0, color: theme::c().frame_hairline_strong },
            ..Default::default()
        })
        .into()
}

fn split<'a, M: 'a>(lanes: Vec<Vec<(&'a str, Led)>>) -> Element<'a, M> {
    let n = lanes.len();
    let lanes_col: Column<'a, M> = column(lanes.into_iter().map(|lane| {
        let mut r = Row::new().push(wire(10)).height(LANE_H).align_y(Center);
        for (name, led) in lane {
            r = r.push(wire(Fill)).push(node(name, led));
        }
        r.push(wire(Fill)).into()
    }))
    .spacing(LANE_GAP)
    .width(Fill);

    let h = n as f32 * LANE_H + (n as f32 - 1.0) * LANE_GAP;
    row![vbar(), lanes_col, vbar()].height(h).width(FillPortion(4)).into()
}
