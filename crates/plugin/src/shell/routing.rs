//! How modulation routes look in the shell: each source's name and colour (macros in macro
//! yellow, the modulators in their tab colours), and the meta knobs a param's knob carries.
//! The routes themselves live in the patch (`patch::routes`); edits go through the session.
//!
//! Also the drag of a source onto a target (drag & drop): what's dragged, and which targets
//! can take it.

use nice_plug_iced::iced::{Color, Point};
use patch::{ModDestination, ModulatorId, Patch, Route, RouteSource};

use super::{drawer, knob};

/// The source's colour (fixed, never a patch colour).
pub fn colour(source: RouteSource) -> Color {
    match source {
        RouteSource::Macro(_) => knob::MACRO,
        RouteSource::Modulator(id) => drawer::MODULATORS[id.index()].1,
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
    drawer::MODULATORS[id.index()].0
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

/// A source being dragged: from a macro's grip or a modulator tab's grip.
#[derive(Debug, Clone, Copy)]
pub struct SourceDrag {
    pub source: RouteSource,
    /// The pointer (window coordinates); `None` until it moves.
    pub at: Option<Point>,
}

impl SourceDrag {
    /// The colour a param knob shows while this drag runs (every param can take every source).
    pub fn param_drop(&self) -> Option<Color> {
        Some(colour(self.source))
    }

    /// The colour a macro cell shows: modulators can drive macros, macros can't drive macros.
    pub fn macro_drop(&self) -> Option<Color> {
        matches!(self.source, RouteSource::Modulator(_)).then(|| colour(self.source))
    }
}
