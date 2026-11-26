//! How modulation routes look in the shell: each source's name and colour (macros in macro
//! yellow, the modulators in their tab colours), and the meta knobs a param's knob carries.
//! The routes themselves live in the patch (`patch::routes`); edits go through the session.
//!
//! Also the drag of a source onto a target (drag & drop): what's dragged, and which targets
//! can take it.

use nice_plug_iced::iced::{Color, Point};
use patch::{ModDestination, ModulatorId, Patch, Route, RouteSource};

use super::{dock, knob};

/// The source's colour (fixed, never a patch colour).
pub fn colour(source: RouteSource) -> Color {
    match source {
        RouteSource::Macro(_) => knob::MACRO,
        RouteSource::Modulator(id) => dock::MODULATORS[id.index()].1,
    }
}

/// "Heat", "LFO 2".
pub fn name(patch: &Patch, source: RouteSource) -> &str {
    match source {
        RouteSource::Macro(slot) => patch.macros.get(slot).map_or("?", |m| m.name.as_str()),
        RouteSource::Modulator(id) => modulator_name(id),
    }
}

pub fn modulator_name(id: ModulatorId) -> &'static str {
    dock::MODULATORS[id.index()].0
}

/// "Heat → Drive", "LFO 2 → Character" (a macro destination).
pub fn title(patch: &Patch, route: Route) -> Option<String> {
    let source = name(patch, patch.route_source(route)?);
    Some(format!("{source} → {}", destination_name(patch, &patch.route_destination(route)?)))
}

pub fn destination_name<'a>(patch: &'a Patch, d: &'a ModDestination) -> &'a str {
    match d {
        ModDestination::Param(id) => patch.param_index(id).map_or(id.as_str(), |i| patch.params[i].name.as_str()),
        ModDestination::Macro(id) => patch.macro_index(id).map_or(id.as_str(), |i| patch.macros[i].name.as_str()),
    }
}

/// Param `param`'s routes and a meta knob for each (same order: the knob's `k`-th meta knob
/// is `routes[k]`).
pub fn metas(patch: &Patch, param: usize) -> (Vec<Route>, Vec<knob::Meta<'_>>) {
    patch
        .routes_to_param(param)
        .into_iter()
        .map(|(route, source, settings)| (route, knob::Meta { name: name(patch, source), settings, colour: colour(source) }))
        .unzip()
}

/// Macro `slot`'s routes (modulators on the macro) and a meta knob for each (same order).
pub fn macro_metas(patch: &Patch, slot: usize) -> (Vec<Route>, Vec<knob::Meta<'_>>) {
    let Some(id) = patch.macros.get(slot).map(|m| &m.id) else { return (Vec::new(), Vec::new()) };
    patch
        .routes()
        .into_iter()
        .filter(|r| matches!(patch.route_destination(*r), Some(ModDestination::Macro(m)) if &m == id))
        .filter_map(|r| {
            let source = patch.route_source(r)?;
            let settings = patch.route_settings(r)?;
            Some((r, knob::Meta { name: name(patch, source), settings, colour: colour(source) }))
        })
        .unzip()
}

/// Macro `slot`'s value now, after the modulators on it (normalized, clamped): its base plus
/// each route's delta at its source's live level, as the audio thread sums them.
pub fn macro_effective(patch: &Patch, values: &patch::Values, slot: usize) -> f32 {
    let Some(id) = patch.macros.get(slot).map(|m| &m.id) else { return 0.0 };
    let base = values.macros[slot];
    let delta: f32 = patch
        .routes()
        .into_iter()
        .filter(|r| matches!(patch.route_destination(*r), Some(ModDestination::Macro(m)) if &m == id))
        .filter_map(|r| {
            let level = match patch.route_source(r)? {
                RouteSource::Modulator(m) => values.modulators[m.index()],
                RouteSource::Macro(s) => values.macros[s],
            };
            Some(patch.route_settings(r)?.delta(level))
        })
        .sum();
    (base + delta).clamp(0.0, 1.0)
}

/// A source being dragged: from its title bar in the dock.
#[derive(Debug, Clone, Copy)]
pub struct SourceDrag {
    pub source: RouteSource,
    /// Where the title bar was pressed (window coordinates).
    pub start: Point,
    /// The pointer (window coordinates); `None` until it moves.
    pub at: Option<Point>,
}

/// How far the pointer must move before a press on a title bar is a drag, not a click.
const DRAG_SLOP: f32 = 4.0;

impl SourceDrag {
    /// The pointer has left the press point: a drag (a release in place is a click).
    pub fn moved(&self) -> bool {
        self.at.is_some_and(|p| (p.x - self.start.x).hypot(p.y - self.start.y) > DRAG_SLOP)
    }

    /// The colour a param knob shows while this drag runs (every param can take every source).
    pub fn param_drop(&self) -> Option<Color> {
        self.moved().then(|| colour(self.source))
    }

    /// The colour a macro cell shows: modulators can drive macros, macros can't drive macros.
    pub fn macro_drop(&self) -> Option<Color> {
        (self.moved() && matches!(self.source, RouteSource::Modulator(_))).then(|| colour(self.source))
    }
}
