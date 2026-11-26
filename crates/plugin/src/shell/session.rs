//! The patch session behind the shell: the library (patch folders), the loaded patch and its
//! patch surface, loading/reloading/unloading, hot reload, and the per-frame values the UI
//! reads. No UI code here.

use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use nice_plug::context::gui::GuiContext;
use nice_plug::prelude::*;
use nice_plug_iced::iced::Color;
use patch::{MACRO_SLOTS, ModDestination, ModSettings, ModulatorSpec, Patch, Route, RouteSource, Values};

use super::brand::{self, Brand};
use super::stage::Surface;
use crate::snapshots::{self, Snapshot};
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
    /// The patch's saved snapshots (sorted; Init isn't listed).
    pub snapshots: Vec<String>,
}

pub struct Loaded {
    pub folder: String,
    pub dir: PathBuf,
    pub patch: Patch,
    pub surface: Option<Surface>,
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
    /// Where snapshots are stored (`snapshots::root`).
    snap_root: PathBuf,
    /// The selected snapshot as it sounds on the loaded patch (`Snapshot::resolve`): what
    /// "modified" compares against.
    baseline: Option<Snapshot>,
    /// The sound differs from the selected snapshot (checked a few times a second).
    pub modified: bool,
    last_compare: Instant,
}

impl Session {
    pub fn new() -> Self {
        let now = Instant::now();
        let mut new_fonts = Vec::new();
        let snap_root = snapshots::root();
        Self {
            library: scan_library(&mut new_fonts, &snap_root),
            snap_root,
            baseline: None,
            modified: false,
            last_compare: now,
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
        // The patch's surface: reuse the running one on hot reload so component state survives.
        let surface = if patch.has_surface {
            let previous = self.current.as_mut().filter(|c| c.folder == folder).and_then(|c| c.surface.take());
            let built = match previous {
                Some(mut p) => p.reload().map(|_| p),
                None => Surface::load(&dir),
            };
            match built {
                Ok(p) => Some(p),
                Err(e) => {
                    self.error = Some(format!("{folder} (surface): {e}").replace('\t', "  "));
                    nice_log!("surface error: {folder}: {e}");
                    return;
                }
            }
        } else {
            None
        };

        *params.patch_name.write().unwrap() = folder.to_string();
        let setter = ctx.param_setter();
        if reset {
            // A patch's defaults: Init.
            params.snapshot.write().unwrap().clear();
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
        self.current = Some(Loaded { folder: folder.to_string(), mtime: patch_mtime(&dir), dir, patch, surface });
        self.error = None;
        self.refresh_baseline(params);
    }

    /// Runs a change to a patch's files (the assistant's commit or undo) and loads the result.
    /// The project's values stay, except where the change moved a default: there the new default
    /// applies (the saved value would hide the change).
    pub fn change_files<T>(
        &mut self,
        ctx: &GuiContext,
        params: &FxParams,
        shared: &Shared,
        folder: &str,
        change: impl FnOnce() -> std::io::Result<T>,
    ) -> std::io::Result<T> {
        let dir = patches_root().join(folder);
        let loaded = self.current.as_ref().is_some_and(|c| c.folder == folder);
        let old = if loaded { Patch::load(&dir).ok() } else { None };
        let out = change()?;
        if loaded {
            let new = Patch::load(&dir).ok();
            let macros = old.as_ref().zip(new.as_ref()).map(|(old, new)| carry_over(params, old, new)).unwrap_or_default();
            self.load(ctx, params, shared, folder, false);
            let setter = ctx.param_setter();
            for (i, v) in macros.into_iter().enumerate() {
                let p = &params.macros[i].value;
                if p.unmodulated_normalized_value() != v {
                    setter.begin_set_parameter(p);
                    setter.set_parameter_normalized(p, v);
                    setter.end_set_parameter(p);
                }
            }
        }
        self.library = scan_library(&mut self.new_fonts, &self.snap_root);
        Ok(out)
    }

    /// The loaded patch's folder and values, for the assistant. `None`: no patch, or a folder
    /// outside the library (the name comes from the project file).
    pub fn describe(&self, params: &FxParams) -> Option<(String, String)> {
        let cur = self.current.as_ref()?;
        let mut parts = Path::new(&cur.folder).components();
        if !matches!((parts.next(), parts.next()), (Some(std::path::Component::Normal(_)), None)) {
            return None;
        }
        let p = &cur.patch;
        let mut s = String::from("macros (0..1):\n");
        for (i, m) in p.macros.iter().enumerate() {
            s += &format!("- {}: {:.3}\n", m.id, params.macros[i].value.unmodulated_normalized_value());
        }
        s += "params (the knob's base value, before modulation):\n";
        for (spec, v) in p.params.iter().zip(&self.free) {
            s += &format!("- {}: {}\n", spec.id, spec.display_plain(*v));
        }
        let edits = params.mod_edits.read().unwrap();
        if !edits.is_empty() {
            s += &format!("the user's routing edits on top of the code: {}\n", serde_json::to_string(&*edits).unwrap_or_default());
        }
        Some((cur.folder.clone(), s))
    }

    /// Back to "no patch": passthrough, default slot names.
    pub fn unload(&mut self, ctx: &GuiContext, params: &FxParams, shared: &Shared) {
        params.patch_name.write().unwrap().clear();
        params.snapshot.write().unwrap().clear();
        self.baseline = None;
        self.modified = false;
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
        self.library = scan_library(&mut self.new_fonts, &self.snap_root);
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

    // ---- snapshots ------------------------------------------------------------------------------

    /// The selected snapshot ("Init" = the patch's defaults).
    pub fn snapshot(&self, params: &FxParams) -> String {
        let name = params.snapshot.read().unwrap();
        if name.is_empty() { snapshots::INIT.to_string() } else { name.clone() }
    }

    /// The loaded patch's saved snapshots.
    pub fn snapshot_names(&self) -> &[String] {
        let Some(cur) = &self.current else { return &[] };
        self.library.iter().find(|p| p.folder == cur.folder).map_or(&[], |p| &p.snapshots[..])
    }

    /// Init, then the saved ones: what ‹ › step through.
    pub fn snapshot_cycle(&self) -> Vec<String> {
        std::iter::once(snapshots::INIT.to_string()).chain(self.snapshot_names().iter().cloned()).collect()
    }

    /// Recalls a snapshot of the loaded patch (Init: its defaults). Replaces the current sound.
    pub fn recall(&mut self, ctx: &GuiContext, params: &FxParams, shared: &Shared, name: &str) -> Result<(), String> {
        let Some(folder) = self.current.as_ref().map(|c| c.folder.clone()) else { return Err("no patch loaded".into()) };
        if name == snapshots::INIT {
            self.load(ctx, params, shared, &folder, true);
            return Ok(());
        }
        let snap = snapshots::read(&self.snap_root, &folder, name).map_err(|e| format!("Couldn't read “{name}”: {e}"))?;
        snap.store(params);
        *params.snapshot.write().unwrap() = name.to_string();
        self.load(ctx, params, shared, &folder, false);
        // Macros are host parameters: set them with gestures, once the patch is in.
        let Some(cur) = &self.current else { return Ok(()) };
        let setter = ctx.param_setter();
        for (i, m) in cur.patch.macros.iter().enumerate() {
            let p = &params.macros[i].value;
            let v = snap.macros.get(&m.id).copied().unwrap_or_else(|| cur.patch.macro_default(i)).clamp(0.0, 1.0);
            if p.unmodulated_normalized_value() != v {
                setter.begin_set_parameter(p);
                setter.set_parameter_normalized(p, v);
                setter.end_set_parameter(p);
            }
        }
        self.compare(params);
        Ok(())
    }

    /// Saves the current sound as `name` (`overwrite`: replacing a snapshot of that name is
    /// intended) and selects it. Returns the stored name.
    pub fn save_snapshot(&mut self, params: &FxParams, name: &str, overwrite: bool) -> Result<String, String> {
        let name = snapshots::check_name(name)?;
        let Some(cur) = &self.current else { return Err("no patch loaded".into()) };
        if !overwrite && snapshots::exists(&self.snap_root, &cur.folder, &name) {
            return Err("a snapshot with that name exists".into());
        }
        let snap = Snapshot::capture(&cur.folder, params, &cur.patch, &self.free);
        snapshots::write(&self.snap_root, &cur.folder, &name, &snap).map_err(|e| format!("Couldn't save: {e}"))?;
        *params.snapshot.write().unwrap() = name.clone();
        self.rescan_snapshots();
        self.refresh_baseline(params);
        Ok(name)
    }

    /// Renames a saved snapshot (the selection follows). Returns the new name.
    pub fn rename_snapshot(&mut self, params: &FxParams, from: &str, to: &str) -> Result<String, String> {
        let to = snapshots::check_name(to)?;
        let Some(cur) = &self.current else { return Err("no patch loaded".into()) };
        snapshots::rename(&self.snap_root, &cur.folder, from, &to).map_err(|e| format!("Couldn't rename: {e}"))?;
        let mut selected = params.snapshot.write().unwrap();
        if *selected == from {
            *selected = to.clone();
        }
        drop(selected);
        self.rescan_snapshots();
        Ok(to)
    }

    /// Deletes a saved snapshot. If it was selected, the sound stays and counts as a change to
    /// Init.
    pub fn delete_snapshot(&mut self, params: &FxParams, name: &str) -> Result<(), String> {
        let Some(cur) = &self.current else { return Err("no patch loaded".into()) };
        snapshots::delete(&self.snap_root, &cur.folder, name).map_err(|e| format!("Couldn't delete: {e}"))?;
        if *params.snapshot.read().unwrap() == name {
            params.snapshot.write().unwrap().clear();
            self.refresh_baseline(params);
        }
        self.rescan_snapshots();
        Ok(())
    }

    /// Copies a snapshot file into the loaded patch's snapshots (under a free name based on the
    /// file's) and recalls it. Refuses one saved from another patch.
    pub fn import_snapshot(&mut self, ctx: &GuiContext, params: &FxParams, shared: &Shared, path: &Path) -> Result<String, String> {
        let snap = snapshots::read_file(path).map_err(|e| format!("Couldn't import: {e}"))?;
        let Some(cur) = &self.current else { return Err("no patch loaded".into()) };
        if !snap.patch.is_empty() && snap.patch != cur.folder {
            return Err(format!("That snapshot is for the patch “{}”, not this one.", snap.patch));
        }
        let base = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Imported");
        let name = snapshots::free_name(&self.snap_root, &cur.folder, base);
        let snap = Snapshot { patch: cur.folder.clone(), ..snap };
        snapshots::write(&self.snap_root, &cur.folder, &name, &snap).map_err(|e| format!("Couldn't import: {e}"))?;
        self.rescan_snapshots();
        self.recall(ctx, params, shared, &name)?;
        Ok(name)
    }

    /// Writes the current sound to a file (`.json` added if missing).
    pub fn export_snapshot(&self, params: &FxParams, path: &Path) -> Result<(), String> {
        let Some(cur) = &self.current else { return Err("no patch loaded".into()) };
        let snap = Snapshot::capture(&cur.folder, params, &cur.patch, &self.free);
        let path = if path.extension().is_some_and(|x| x == "json") { path.to_path_buf() } else { path.with_extension("json") };
        snapshots::write_file(&path, &snap).map_err(|e| format!("Couldn't export: {e}"))
    }

    /// The selected snapshot, resolved against the loaded patch. A snapshot file that's gone or
    /// unreadable falls back to Init (the sound stays, now as a change to Init).
    fn refresh_baseline(&mut self, params: &FxParams) {
        let Some(cur) = &self.current else {
            self.baseline = None;
            return;
        };
        let name = params.snapshot.read().unwrap().clone();
        let stored = if name.is_empty() {
            Snapshot::default()
        } else {
            match snapshots::read(&self.snap_root, &cur.folder, &name) {
                Ok(s) => s,
                Err(e) => {
                    nice_log!("snapshot '{name}' of '{}': {e}; back to Init", cur.folder);
                    params.snapshot.write().unwrap().clear();
                    Snapshot::default()
                }
            }
        };
        self.baseline = Some(stored.resolve(&cur.folder, &cur.patch));
        self.compare(params);
    }

    /// Updates `modified`.
    fn compare(&mut self, params: &FxParams) {
        self.last_compare = Instant::now();
        self.modified = match (&self.current, &self.baseline) {
            (Some(cur), Some(base)) => !Snapshot::capture(&cur.folder, params, &cur.patch, &self.free).same_sound(base),
            _ => false,
        };
    }

    /// The loaded patch's snapshot list, after a change on disk.
    fn rescan_snapshots(&mut self) {
        let Some(cur) = &self.current else { return };
        let names = snapshots::list(&self.snap_root, &cur.folder);
        if let Some(p) = self.library.iter_mut().find(|p| p.folder == cur.folder) {
            p.snapshots = names;
        }
    }

    /// Once per frame: hot reload, then the values the UI shows.
    pub fn tick(&mut self, ctx: &GuiContext, params: &FxParams, shared: &Shared) {
        self.poll_hot_reload(ctx, params, shared);
        if self.last_compare.elapsed().as_millis() >= 200 {
            self.compare(params);
        }
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
        self.values.tempo = shared.tempo.load(std::sync::atomic::Ordering::Relaxed);
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

/// Adapts the project's saved values to a changed patch file. Returns the macros' new values:
/// macros are matched by id, so one that moved to another slot keeps its value.
fn carry_over(params: &FxParams, old: &Patch, new: &Patch) -> Vec<f32> {
    let mut stored = params.param_values.write().unwrap();
    for spec in &new.params {
        let before = old.param_index(&spec.id).map(|i| old.params[i].default_plain());
        if before != Some(spec.default_plain()) {
            stored.insert(spec.id.clone(), spec.default_plain());
        }
    }
    drop(stored);

    let routes = |p: &Patch| -> std::collections::HashMap<String, ModSettings> {
        (0..p.modulations.len())
            .flat_map(|i| (0..p.modulations[i].len()).map(move |k| (i, k)))
            .map(|(i, k)| (p.mod_key(i, k), p.modulations[i][k].patch))
            .collect()
    };
    let (was, is) = (routes(old), routes(new));
    let changed = |key: &String| is.contains_key(key) && was.get(key) != is.get(key);
    params.mod_settings.write().unwrap().retain(|key, _| !changed(key));
    let mut edits = params.mod_edits.write().unwrap();
    edits.removed.retain(|key| !changed(key));
    if old.assignments != new.assignments {
        edits.assignments = None;
    }
    if old.modulators != new.modulators {
        edits.modulators = None;
    }
    drop(edits);

    let value = |i: usize| match old.macro_index(&new.macros[i].id) {
        Some(o) if old.macro_default(o) == new.macro_default(i) => params.macros[o].value.unmodulated_normalized_value(),
        _ => new.macro_default(i),
    };
    (0..new.macros.len()).map(value).collect()
}

/// All patch folders with what the library shows (name, colours). Evaluates each patch.
fn scan_library(new_fonts: &mut Vec<Vec<u8>>, snap_root: &Path) -> Vec<PatchInfo> {
    patch::discover(&patches_root())
        .into_iter()
        .filter_map(|dir| {
            let folder = dir.file_name()?.to_str()?.to_string();
            let snapshots = snapshots::list(snap_root, &folder);
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
                    snapshots,
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
                    snapshots,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn patch(name: &str, src: &str) -> Patch {
        let dir = std::env::temp_dir().join(format!("scriptfx-carry-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("init.luau"), src).unwrap();
        Patch::load(&dir).unwrap()
    }

    #[test]
    fn a_changed_default_wins_over_the_saved_value() {
        let src = |db: i32, freq: i32, d: f32| {
            format!(
                "local g = Blocks.Gain.new('g', {{ db = {db} }})\nlocal l = Blocks.LowPass.new('l', {{ freq = {freq} }})\n\
                 return Patch.new('T', {{ chain = Chain.new({{ g, l }}), macros = {{ Macro.new('m', 'M', {{ default = {d} }}):drives(l.params.q) }} }})"
            )
        };
        let (old, new) = (patch("a", &src(0, 8000, 0.5)), patch("b", &src(-6, 8000, 0.25)));
        let params = FxParams::default();
        params.param_values.write().unwrap().extend([("g.db".to_string(), 3.0), ("l.freq".to_string(), 500.0)]);
        assert_eq!(carry_over(&params, &old, &new), vec![0.25]);
        let stored = params.param_values.read().unwrap();
        assert_eq!((stored["g.db"], stored["l.freq"]), (-6.0, 500.0));
    }
}
