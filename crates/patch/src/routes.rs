//! Modulation routes as one list: a patch has two kinds of route, macro slots
//! ([`Patch::modulations`], macro → param) and modulator assignments ([`Patch::assignments`],
//! Follow/LFO/Step → param or macro). The editor shows and edits both alike (meta knobs, the
//! Matrix, drag & drop), so this module gives them one identity ([`Route`]) and one set of
//! operations: list, read, add, remove.
//!
//! What the user changed against the patch file is kept as [`ModEdits`] (saved with the project
//! and applied again on every load, so hot reload keeps them). Slot *settings* (amount, polarity,
//! …) of macro routes are saved separately, by [`Patch::mod_key`].

use serde::{Deserialize, Serialize};

use crate::{
    ModAssignment, ModDestination, ModSettings, Modulation, ModulatorId, ModulatorSpec, Patch, validate_modulators,
};

/// Where a route's signal comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RouteSource {
    /// A macro slot.
    Macro(usize),
    Modulator(ModulatorId),
}

/// One route of the current patch. Indices: only valid until the routes change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Route {
    /// Param `param`'s `k`-th macro slot.
    Macro { param: usize, k: usize },
    /// `Patch::assignments[i]`.
    Assignment(usize),
}

impl RouteSource {
    /// The settings a new route starts with: macros and Follow move one way (unipolar, 50 %),
    /// LFOs and Step swing around the knob (bipolar, 25 % each way: a 50 % range).
    pub fn default_settings(self) -> ModSettings {
        match self {
            RouteSource::Macro(_) | RouteSource::Modulator(ModulatorId::Follow) => ModSettings { amount: 0.5, ..Default::default() },
            RouteSource::Modulator(_) => ModSettings { amount: 0.5, bipolar: true, ..Default::default() },
        }
    }
}

impl Patch {
    /// Every route into param `param`: its macro slots first, then the assignments, in patch order.
    pub fn routes_to_param(&self, param: usize) -> Vec<(Route, RouteSource, ModSettings)> {
        let Some(spec) = self.params.get(param) else { return Vec::new() };
        let macros = self.modulations[param]
            .iter()
            .enumerate()
            .map(|(k, m)| (Route::Macro { param, k }, RouteSource::Macro(m.slot), m.settings));
        let assignments = self.assignments.iter().enumerate().filter_map(|(i, a)| {
            matches!(&a.destination, ModDestination::Param(id) if *id == spec.id)
                .then_some((Route::Assignment(i), RouteSource::Modulator(a.source), a.settings))
        });
        macros.chain(assignments).collect()
    }

    /// Every route of the patch: macro slots (by param), then assignments.
    pub fn routes(&self) -> Vec<Route> {
        let macros = self.modulations.iter().enumerate().flat_map(|(param, m)| (0..m.len()).map(move |k| Route::Macro { param, k }));
        macros.chain((0..self.assignments.len()).map(Route::Assignment)).collect()
    }

    pub fn route_source(&self, r: Route) -> Option<RouteSource> {
        Some(match r {
            Route::Macro { param, k } => RouteSource::Macro(self.modulations.get(param)?.get(k)?.slot),
            Route::Assignment(i) => RouteSource::Modulator(self.assignments.get(i)?.source),
        })
    }

    pub fn route_destination(&self, r: Route) -> Option<ModDestination> {
        Some(match r {
            Route::Macro { param, k } => {
                self.modulations.get(param)?.get(k)?;
                ModDestination::Param(self.params[param].id.clone())
            }
            Route::Assignment(i) => self.assignments.get(i)?.destination.clone(),
        })
    }

    pub fn route_settings(&self, r: Route) -> Option<ModSettings> {
        Some(match r {
            Route::Macro { param, k } => self.modulations.get(param)?.get(k)?.settings,
            Route::Assignment(i) => self.assignments.get(i)?.settings,
        })
    }

    /// What Reset goes back to: the patch's settings for a macro slot, the source's defaults
    /// for an assignment.
    pub fn route_reset(&self, r: Route) -> Option<ModSettings> {
        Some(match r {
            Route::Macro { param, k } => self.modulations.get(param)?.get(k)?.patch,
            Route::Assignment(i) => RouteSource::Modulator(self.assignments.get(i)?.source).default_settings(),
        })
    }

    /// The route from `source` to `destination`, if there is one.
    pub fn find_route(&self, source: RouteSource, destination: &ModDestination) -> Option<Route> {
        match (source, destination) {
            (RouteSource::Macro(slot), ModDestination::Param(id)) => {
                let param = self.param_index(id)?;
                let k = self.modulations[param].iter().position(|m| m.slot == slot)?;
                Some(Route::Macro { param, k })
            }
            (RouteSource::Modulator(id), dest) => {
                self.assignments.iter().position(|a| a.source == id && a.destination == *dest).map(Route::Assignment)
            }
            (RouteSource::Macro(_), ModDestination::Macro(_)) => None,
        }
    }

    /// Adds a route with the source's default settings. `None`: it exists already, the source or
    /// destination is unknown, or it's a macro → macro route (macros only drive params).
    pub fn add_route(&mut self, source: RouteSource, destination: ModDestination) -> Option<Route> {
        if self.find_route(source, &destination).is_some() {
            return None;
        }
        let settings = source.default_settings();
        match (source, &destination) {
            (RouteSource::Macro(slot), ModDestination::Param(id)) => {
                self.macros.get(slot)?;
                let param = self.param_index(id)?;
                self.modulations[param].push(Modulation { slot, settings, patch: settings });
                Some(Route::Macro { param, k: self.modulations[param].len() - 1 })
            }
            (RouteSource::Macro(_), ModDestination::Macro(_)) => None,
            (RouteSource::Modulator(id), dest) => {
                let known = match dest {
                    ModDestination::Param(p) => self.param_index(p).is_some(),
                    ModDestination::Macro(m) => self.macro_index(m).is_some(),
                };
                known.then(|| {
                    self.assignments.push(ModAssignment { source: id, destination: destination.clone(), settings });
                    Route::Assignment(self.assignments.len() - 1)
                })
            }
        }
    }

    /// Removes a route. Later routes of the same kind move up (their indices change).
    pub fn remove_route(&mut self, r: Route) -> bool {
        match r {
            Route::Macro { param, k } if self.modulations.get(param).is_some_and(|m| k < m.len()) => {
                self.modulations[param].remove(k);
                true
            }
            Route::Assignment(i) if i < self.assignments.len() => {
                self.assignments.remove(i);
                true
            }
            _ => false,
        }
    }

    /// Replaces one modulator's settings (validated like the patch file's). `false`: invalid.
    pub fn set_modulator(&mut self, spec: ModulatorSpec) -> bool {
        let mut all = self.modulators.clone();
        let i = spec.id.index();
        if i >= all.len() {
            return false;
        }
        all[i] = spec;
        match validate_modulators(all) {
            Ok(v) => {
                self.modulators = v;
                true
            }
            Err(_) => false,
        }
    }
}

/// The routing changes the user made against the patch file, saved with the project.
/// Macro routes by key (`"param id<macro id"`, [`Patch::mod_key`]); assignments and modulator
/// settings as whole lists once touched (`None` = what the patch defines).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModEdits {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub assignments: Option<Vec<ModAssignment>>,
    pub modulators: Option<Vec<ModulatorSpec>>,
}

impl ModEdits {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Applies the edits to a freshly loaded patch. Anything that no longer fits the patch (a
    /// renamed param, an invalid setting) is skipped, so an edited file never fails to load.
    pub fn apply(&self, patch: &mut Patch) {
        for key in &self.removed {
            if let Some(r) = macro_route_by_key(patch, key) {
                patch.remove_route(r);
            }
        }
        for key in &self.added {
            let Some((param, mac)) = key.split_once('<') else { continue };
            let Some(slot) = patch.macro_index(mac) else { continue };
            patch.add_route(RouteSource::Macro(slot), ModDestination::Param(param.to_string()));
        }
        if let Some(list) = &self.assignments {
            patch.assignments.clear();
            for a in list {
                if patch.add_route(RouteSource::Modulator(a.source), a.destination.clone()).is_some() {
                    patch.assignments.last_mut().unwrap().settings = sanitized(a.settings);
                }
            }
        }
        if let Some(list) = &self.modulators {
            if let Ok(v) = validate_modulators(list.clone()) {
                patch.modulators = v;
            }
        }
    }

    /// A macro route was added by the user (key: [`Patch::mod_key`]).
    pub fn route_added(&mut self, key: String) {
        if let Some(i) = self.removed.iter().position(|k| *k == key) {
            self.removed.remove(i);
        } else if !self.added.contains(&key) {
            self.added.push(key);
        }
    }

    /// A macro route was removed by the user.
    pub fn route_removed(&mut self, key: String) {
        if let Some(i) = self.added.iter().position(|k| *k == key) {
            self.added.remove(i);
        } else if !self.removed.contains(&key) {
            self.removed.push(key);
        }
    }
}

fn macro_route_by_key(patch: &Patch, key: &str) -> Option<Route> {
    let (param, mac) = key.split_once('<')?;
    let slot = patch.macro_index(mac)?;
    patch.find_route(RouteSource::Macro(slot), &ModDestination::Param(param.to_string()))
}

/// Saved settings clamped into range (a hand-edited or old project can't break the graph).
fn sanitized(s: ModSettings) -> ModSettings {
    let clamp = |v: f32| if v.is_finite() { v.clamp(-1.0, 1.0) } else { 0.0 };
    ModSettings { amount: clamp(s.amount), curve: clamp(s.curve), ..s }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patch(src: &str) -> (Patch, std::path::PathBuf) {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let d = std::env::temp_dir().join(format!("patch-routes-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("init.luau"), src).unwrap();
        (Patch::load(&d).unwrap(), d)
    }

    const SRC: &str = r#"
        local g = Blocks.Gain.new("g", { db = 0 })
        local f = Blocks.LowPass.new("f", { freq = 1000 })
        local heat = Macro.new("heat", "Heat"):drives(g.params.db)
        local air = Macro.new("air", "Air"):drives(f.params.freq)
        return Patch.new("T", { chain = Chain.new { g, f }, macros = { heat, air } })
    "#;

    #[test]
    fn add_list_and_remove_routes_of_both_kinds() {
        let (mut p, _d) = patch(SRC);
        let db = p.param_index("g.db").unwrap();
        assert_eq!(p.routes_to_param(db).len(), 1);

        let lfo = p.add_route(RouteSource::Modulator(ModulatorId::Lfo2), ModDestination::Param("g.db".into())).unwrap();
        let air = p.add_route(RouteSource::Macro(1), ModDestination::Param("g.db".into())).unwrap();
        assert_eq!(air, Route::Macro { param: db, k: 1 });
        // Duplicates and macro → macro are refused.
        assert!(p.add_route(RouteSource::Macro(1), ModDestination::Param("g.db".into())).is_none());
        assert!(p.add_route(RouteSource::Modulator(ModulatorId::Lfo2), ModDestination::Param("g.db".into())).is_none());
        assert!(p.add_route(RouteSource::Macro(0), ModDestination::Macro("air".into())).is_none());
        assert!(p.add_route(RouteSource::Modulator(ModulatorId::Step), ModDestination::Param("nope".into())).is_none());
        let to_macro = p.add_route(RouteSource::Modulator(ModulatorId::Follow), ModDestination::Macro("air".into())).unwrap();

        let routes = p.routes_to_param(db);
        assert_eq!(routes.iter().map(|r| r.1).collect::<Vec<_>>(), [
            RouteSource::Macro(0),
            RouteSource::Macro(1),
            RouteSource::Modulator(ModulatorId::Lfo2)
        ]);
        assert!(p.route_settings(lfo).unwrap().bipolar);
        assert_eq!(p.routes().len(), 2 + 1 + 2);

        assert!(p.remove_route(lfo));
        // The Follow → macro assignment moved up.
        assert_eq!(p.route_source(Route::Assignment(0)), Some(RouteSource::Modulator(ModulatorId::Follow)));
        assert!(!p.remove_route(to_macro)); // index 1 is gone now
        assert!(p.remove_route(Route::Assignment(0)));
        assert!(p.remove_route(Route::Macro { param: db, k: 0 }));
        assert_eq!(p.routes_to_param(db), vec![(Route::Macro { param: db, k: 0 }, RouteSource::Macro(1), RouteSource::Macro(1).default_settings())]);
    }

    #[test]
    fn edits_replay_onto_a_fresh_load() {
        let (mut p, d) = patch(SRC);
        let mut edits = ModEdits::default();
        let removed = p.mod_key(p.param_index("g.db").unwrap(), 0);
        p.remove_route(Route::Macro { param: p.param_index("g.db").unwrap(), k: 0 });
        edits.route_removed(removed);
        let r = p.add_route(RouteSource::Macro(0), ModDestination::Param("f.freq".into())).unwrap();
        let Route::Macro { param, k } = r else { panic!() };
        edits.route_added(p.mod_key(param, k));
        p.add_route(RouteSource::Modulator(ModulatorId::Lfo1), ModDestination::Param("f.freq".into()));
        p.assignments[0].settings.amount = -0.7;
        edits.assignments = Some(p.assignments.clone());
        let mut lfo = p.modulators[1].clone();
        if let crate::ModulatorSettings::Lfo { rate_hz, .. } = &mut lfo.settings {
            *rate_hz = 7.0;
        }
        assert!(p.set_modulator(lfo.clone()));
        edits.modulators = Some(p.modulators.clone());

        // Saved and restored as JSON, as the host project does.
        let edits: ModEdits = serde_json::from_str(&serde_json::to_string(&edits).unwrap()).unwrap();
        let mut fresh = Patch::load(&d).unwrap();
        edits.apply(&mut fresh);
        assert_eq!(fresh.modulations, p.modulations);
        assert_eq!(fresh.assignments, p.assignments);
        assert_eq!(fresh.modulators[1], lfo);

        // Adding back what was removed cancels out.
        let mut e = ModEdits::default();
        e.route_removed("a<b".into());
        e.route_added("a<b".into());
        assert!(e.is_empty());
    }

    #[test]
    fn invalid_modulator_settings_are_refused() {
        let (mut p, _d) = patch(SRC);
        let mut lfo = p.modulators[1].clone();
        if let crate::ModulatorSettings::Lfo { rate_hz, .. } = &mut lfo.settings {
            *rate_hz = 500.0;
        }
        assert!(!p.set_modulator(lfo));
        let wrong = ModulatorSpec { id: ModulatorId::Follow, settings: p.modulators[1].settings.clone() };
        assert!(!p.set_modulator(wrong));
    }
}
