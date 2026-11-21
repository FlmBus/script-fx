//! The patch session behind the shell: the library (patch folders), the loaded patch and its
//! ControlPanel, loading/reloading/unloading, hot reload, and the per-frame values the UI
//! reads. No UI code here.

use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use nice_plug::context::gui::GuiContext;
use nice_plug::prelude::*;
use nice_plug_iced::iced::Color;
use patch::{MACRO_SLOTS, ModDestination, ModSettings, ModulatorSpec, Patch, Route, RouteSource, Values};

use super::brand::{self, Brand};
use super::stage::Panel;
use crate::{FxParams, Shared, patches_root};

/// One entry of the library (a patch folder).
pub struct PatchInfo {
    pub folder: String,
    pub name: String,
    pub accent: Color,
    pub bg: Color,
    /// Snapshot names on the card (default: derived from the accent).
    pub ink: Option<Color>,
    /// The patch didn't load (shown in the library; loading it again shows the error).
    pub broken: bool,
    /// The card's title font and logo (`theme.library.font/logo`, else `fonts.title`/`images.logo`).
    pub brand: Brand,
}

pub struct Loaded {
    pub folder: String,
    pub dir: PathBuf,
    pub patch: Patch,
    pub panel: Option<Panel>,
    mtime: Option<SystemTime>,
}

pub struct Session {
    pub library: Vec<PatchInfo>,
    pub current: Option<Loaded>,
    /// Plain values of all params of the current patch (free ones are authoritative).
    pub free: Vec<f32>,
    /// What the UI reads this frame.
    pub values: Values,
    /// Last load/reload error. The previous patch keeps running.
    pub error: Option<String>,
    pub hot_reload: bool,
    /// Patch font files the shell still has to hand to iced.
    pub new_fonts: Vec<Vec<u8>>,
    start: Instant,
    last_poll: Instant,
    /// Routes or modulator settings changed: the graph is rebuilt once at the next tick (it takes
    /// over the playing graph's state, so this doesn't click). Until then the playing graph's
    /// settings slots no longer match the patch's routes and aren't written.
    graph_dirty: bool,
}

impl Session {
    pub fn new() -> Self {
        let now = Instant::now();
        let mut new_fonts = Vec::new();
        Self {
            library: scan_library(&mut new_fonts),
            new_fonts,
            current: None,
            free: Vec::new(),
            values: Values::default(),
            error: None,
            hot_reload: true,
            start: now,
            last_poll: now,
            graph_dirty: false,
        }
    }

    /// First frame: load what the host project restored (or `SCRIPTFX_DEV_PATCH` for a fresh
    /// instance in development). A fresh instance otherwise starts empty.
    pub fn restore(&mut self, ctx: &GuiContext, params: &FxParams, shared: &Shared) {
        let name = params.patch_name.read().unwrap().clone();
        if !name.is_empty() {
            self.load(ctx, params, shared, &name, false);
        } else if let Some(dev) = std::env::var("SCRIPTFX_DEV_PATCH").ok().filter(|d| !d.is_empty()) {
            self.load(ctx, params, shared, &dev, true);
        }
    }

    /// Loads a patch. On failure the previous patch keeps running and `error` is set, so
    /// hot-reload typos don't blank the UI. `reset`: apply the patch's defaults (patch switch)
    /// instead of keeping the values restored from the host project.
    pub fn load(&mut self, ctx: &GuiContext, params: &FxParams, shared: &Shared, folder: &str, reset: bool) {
        let dir = patches_root().join(folder);
        let mut patch = match Patch::load(&dir) {
            Ok(p) => p,
            Err(e) => {
                self.error = Some(format!("{folder}: {e}").replace('\t', "  "));
                nice_log!("patch error: {folder}: {e}");
                return;
            }
        };
        // ControlPanel: reuse the running one on hot reload so local component state survives.
        let panel = if patch.has_panel {
            let previous = self.current.as_mut().filter(|c| c.folder == folder).and_then(|c| c.panel.take());
            let built = match previous {
                Some(mut p) => p.reload().map(|_| p),
                None => Panel::load(&dir),
            };
            match built {
                Ok(p) => Some(p),
                Err(e) => {
                    self.error = Some(format!("{folder} (panel): {e}").replace('\t', "  "));
                    nice_log!("panel error: {folder}: {e}");
                    return;
                }
            }
        } else {
            None
        };

        *params.patch_name.write().unwrap() = folder.to_string();
        let setter = ctx.param_setter();
        if reset {
            let mut stored = params.param_values.write().unwrap();
            stored.clear();
            for p in &patch.params {
                stored.insert(p.id.clone(), p.default_plain());
            }
            drop(stored);
            // A new patch starts with its own mappings.
            params.mod_settings.write().unwrap().clear();
            *params.mod_edits.write().unwrap() = Default::default();
            for i in 0..patch.macros.len() {
                let p = &params.macros[i].value;
                setter.begin_set_parameter(p);
                setter.set_parameter_normalized(p, patch.macro_default(i));
                setter.end_set_parameter(p);
            }
        }
        params.apply_mod_settings(&mut patch);
        self.free = params.free_values(&patch);
        shared.publish(&patch, &self.free);
        self.graph_dirty = false;
        params.set_macro_names(&patch);
        ctx.rescan_param_info();

        nice_log!("loaded patch '{folder}' (reset={reset})");
        self.current = Some(Loaded { folder: folder.to_string(), mtime: patch_mtime(&dir), dir, patch, panel });
        self.error = None;
    }

    /// Back to "no patch": passthrough, default slot names.
    pub fn unload(&mut self, ctx: &GuiContext, params: &FxParams, shared: &Shared) {
        params.patch_name.write().unwrap().clear();
        params.param_values.write().unwrap().clear();
        let empty = Patch::empty();
        shared.publish(&empty, &[]);
        params.set_macro_names(&empty);
        ctx.rescan_param_info();
        self.current = None;
        self.free.clear();
        self.error = None;
    }

    /// Rescans the library and reloads the current patch (keeping its values).
    pub fn reload(&mut self, ctx: &GuiContext, params: &FxParams, shared: &Shared) {
        self.library = scan_library(&mut self.new_fonts);
        if let Some(folder) = self.current.as_ref().map(|c| c.folder.clone()) {
            self.load(ctx, params, shared, &folder, false);
        }
    }

    pub fn set_free(&mut self, params: &FxParams, shared: &Shared, index: usize, plain: f32) {
        let Some(cur) = &self.current else { return };
        let Some(spec) = cur.patch.params.get(index) else { return };
        self.free[index] = plain;
        shared.set_free(index, plain);
        params.param_values.write().unwrap().insert(spec.id.clone(), plain);
    }

    /// A route's settings changed (meta knob, menu, Matrix): the UI's patch, the playing graph
    /// (lock-free), and the project.
    pub fn set_route(&mut self, params: &FxParams, shared: &Shared, route: Route, settings: ModSettings) {
        let Some(cur) = &mut self.current else { return };
        let p = &mut cur.patch;
        match route {
            Route::Macro { param, k } => {
                let Some(m) = p.modulations.get_mut(param).and_then(|m| m.get_mut(k)) else { return };
                m.settings = settings;
                if !self.graph_dirty {
                    shared.set_mod(p.mod_index(param, k), settings);
                }
                params.mod_settings.write().unwrap().insert(p.mod_key(param, k), settings);
            }
            Route::Assignment(i) => {
                let Some(a) = p.assignments.get_mut(i) else { return };
                a.settings = settings;
                if !self.graph_dirty {
                    shared.set_assignment(i, settings);
                }
                params.mod_edits.write().unwrap().assignments = Some(p.assignments.clone());
            }
        }
    }

    /// Drag & drop or the Matrix: a new route from `source` to `destination`, with the source's
    /// default settings. `None`: it exists already or can't be made (macro → macro).
    pub fn add_route(&mut self, params: &FxParams, source: RouteSource, destination: ModDestination) -> Option<Route> {
        let p = &mut self.current.as_mut()?.patch;
        let route = p.add_route(source, destination)?;
        let mut edits = params.mod_edits.write().unwrap();
        match route {
            Route::Macro { param, k } => {
                let key = p.mod_key(param, k);
                params.mod_settings.write().unwrap().insert(key.clone(), p.modulations[param][k].settings);
                edits.route_added(key);
            }
            Route::Assignment(_) => edits.assignments = Some(p.assignments.clone()),
        }
        self.graph_dirty = true;
        Some(route)
    }

    /// The meta menu or the Matrix: removes a route (macro routes from the patch file too, for
    /// this project).
    pub fn remove_route(&mut self, params: &FxParams, route: Route) {
        let Some(cur) = &mut self.current else { return };
        let p = &mut cur.patch;
        let key = match route {
            Route::Macro { param, k } if p.route_source(route).is_some() => Some(p.mod_key(param, k)),
            _ => None,
        };
        if !p.remove_route(route) {
            return;
        }
        let mut edits = params.mod_edits.write().unwrap();
        match key {
            Some(key) => {
                params.mod_settings.write().unwrap().remove(&key);
                edits.route_removed(key);
            }
            None => edits.assignments = Some(p.assignments.clone()),
        }
        self.graph_dirty = true;
    }

    /// A modulator's settings changed (the drawer). Invalid settings are ignored.
    pub fn set_modulator(&mut self, params: &FxParams, spec: ModulatorSpec) {
        let Some(cur) = &mut self.current else { return };
        if cur.patch.set_modulator(spec) {
            params.mod_edits.write().unwrap().modulators = Some(cur.patch.modulators.clone());
            self.graph_dirty = true;
        }
    }

    /// Once per frame: hot reload, then the values the UI shows.
    pub fn tick(&mut self, ctx: &GuiContext, params: &FxParams, shared: &Shared) {
        self.poll_hot_reload(ctx, params, shared);
        if std::mem::take(&mut self.graph_dirty) {
            if let Some(cur) = &self.current {
                shared.publish(&cur.patch, &self.free);
            }
        }

        for i in 0..MACRO_SLOTS {
            self.values.macros[i] = params.macros[i].value.modulated_normalized_value();
        }
        (self.values.modulators, self.values.modulator_phases) = shared.modulator_values();
        if let Some(cur) = &self.current {
            let macros = self.values.macros;
            let mods = self.values.modulators;
            cur.patch.compute_params_with_modulators(&macros, &mods, &self.free, &mut self.values.params);
            self.values.bases.clear();
            self.values.bases.extend(cur.patch.params.iter().zip(&self.free).map(|(p, f)| p.normalize(*f)));
        }
        let level = |g: f32| ((util::gain_to_db(g) + 60.0) / 60.0).clamp(0.0, 1.0);
        self.values.meter_in = level(shared.meter_in.load(std::sync::atomic::Ordering::Relaxed));
        self.values.meter_out = level(shared.meter_out.load(std::sync::atomic::Ordering::Relaxed));
        shared.block_meters(&mut self.values.blocks);
        self.values.time = self.start.elapsed().as_secs_f32();
    }

    fn poll_hot_reload(&mut self, ctx: &GuiContext, params: &FxParams, shared: &Shared) {
        if !self.hot_reload || self.last_poll.elapsed().as_millis() < 400 {
            return;
        }
        self.last_poll = Instant::now();
        let Some(cur) = &mut self.current else { return };
        let mtime = patch_mtime(&cur.dir);
        if mtime != cur.mtime {
            // Remember the attempt even if it fails, so we don't retry every poll.
            cur.mtime = mtime;
            let folder = cur.folder.clone();
            self.load(ctx, params, shared, &folder, false);
        }
    }
}

/// All patch folders with what the library shows (name, colours). Evaluates each patch.
fn scan_library(new_fonts: &mut Vec<Vec<u8>>) -> Vec<PatchInfo> {
    patch::discover(&patches_root())
        .into_iter()
        .filter_map(|dir| {
            let folder = dir.file_name()?.to_str()?.to_string();
            Some(match Patch::load(&dir) {
                Ok(p) => {
                    let accent = hex(p.theme.library_accent()).unwrap_or(super::theme::c().frame_ink_2);
                    PatchInfo {
                    accent,
                    bg: hex(p.theme.library_bg()).unwrap_or(super::theme::c().frame_raised),
                    ink: hex(p.theme.library_ink()),
                    brand: brand::load(&dir, &p.theme, &p.name, accent, new_fonts),
                    name: p.name,
                    folder,
                    broken: false,
                }
                }
                Err(_) => PatchInfo {
                    name: folder.clone(),
                    folder,
                    accent: super::theme::c().frame_muted,
                    bg: super::theme::c().frame_raised,
                    broken: true,
                    ink: None,
                    brand: Brand { font: None, title: None, logo: None },
                },
            })
        })
        .collect()
}

/// `#rrggbb` → colour.
pub fn hex(s: Option<&str>) -> Option<Color> {
    let s = s?.trim().strip_prefix('#')?;
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some(Color::from_rgb8((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

/// Newest modification time of any `.luau` file in the patch folder (init.luau and everything
/// it may require, including subfolders).
fn patch_mtime(dir: &Path) -> Option<SystemTime> {
    fn walk(dir: &Path, depth: usize, newest: &mut Option<SystemTime>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            let path = e.path();
            if path.is_dir() && depth < 4 {
                walk(&path, depth + 1, newest);
            } else if path.extension().is_some_and(|x| x == "luau") {
                let t = e.metadata().and_then(|m| m.modified()).ok();
                *newest = (*newest).max(t);
            }
        }
    }
    let mut newest = None;
    walk(dir, 0, &mut newest);
    newest
}
