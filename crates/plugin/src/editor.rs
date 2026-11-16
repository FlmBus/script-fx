//! Plugin chrome (egui) hosting the preset's own panel (resvg → egui texture).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime};

use nih_plug::prelude::*;
use nih_plug_egui::create_egui_editor;
use nih_plug_egui::egui::{
    self, Color32, ColorImage, CursorIcon, Pos2, Rect, Sense, Stroke, TextureHandle,
    TextureOptions, Visuals, pos2, vec2,
};
use preset::{Control, MACRO_SLOTS, Preset, Source, Theme, Values};
use ui_svg::SvgPanel;

use crate::{FxParams, Shared, presets_root};

pub fn create(params: Arc<FxParams>, shared: Arc<Shared>) -> Option<Box<dyn Editor>> {
    create_egui_editor(
        params.editor_state.clone(),
        EditorState::new(),
        // Called on every window open. The editor state outlives the window, but each
        // window gets a fresh egui context: textures and visuals must be re-created.
        |ctx, state| state.on_window_open(ctx),
        move |ctx, setter, state| state.frame(ctx, setter, &params, &shared),
    )
}

struct Loaded {
    name: String,
    dir: PathBuf,
    preset: Preset,
    panel: Option<SvgPanel>,
    texture: Option<TextureHandle>,
    mtime: Option<SystemTime>,
    load_ms: f32,
}

#[derive(Clone, Copy)]
enum DragWhat {
    Macro(usize),
    Param(usize),
}

struct DragState {
    what: DragWhat,
    range: f32,
    start: f32,
    /// Accumulated vertical drag in SVG units.
    accum: f32,
}

struct EditorState {
    available: Vec<String>,
    current: Option<Loaded>,
    /// Plain values of all params of the current preset (free ones are authoritative).
    free: Vec<f32>,
    error: Option<String>,
    anim_error: Option<String>,
    values: Values,
    start: Instant,
    last_frame: Instant,
    last_poll: Instant,
    hot_reload: bool,
    drag: Option<DragState>,
    prompt: String,
    lua_us: f32,
}

enum Action {
    Switch(String),
    Reload,
}

impl EditorState {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            available: scan_presets(),
            current: None,
            free: Vec::new(),
            error: None,
            anim_error: None,
            values: Values::default(),
            start: now,
            last_frame: now,
            last_poll: now,
            hot_reload: true,
            drag: None,
            prompt: String::new(),
            lua_us: 0.0,
        }
    }

    fn on_window_open(&mut self, ctx: &egui::Context) {
        self.drag = None;
        if let Some(cur) = &mut self.current {
            cur.texture = None;
            if let Some(panel) = &mut cur.panel {
                panel.invalidate();
            }
            ctx.set_visuals(theme_visuals(&cur.preset.theme));
        }
    }

    fn frame(&mut self, ctx: &egui::Context, setter: &ParamSetter, params: &FxParams, shared: &Shared) {
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;

        if self.current.is_none() && self.error.is_none() {
            // Empty name = fresh instance (nothing restored by the host):
            // load the default preset and apply its defaults.
            let name = params.preset_name.read().unwrap().clone();
            let fresh = name.is_empty();
            let name = if fresh { crate::DEFAULT_PRESET.to_string() } else { name };
            self.load(ctx, setter, params, shared, &name, fresh);
        }
        self.poll_hot_reload(ctx, setter, params, shared);

        // Gather values for this frame.
        for i in 0..MACRO_SLOTS {
            self.values.macros[i] = params.macros[i].value.modulated_normalized_value();
        }
        if let Some(cur) = &self.current {
            let macros = self.values.macros;
            cur.preset.compute_params(&macros, &self.free, &mut self.values.params);
        }
        let level = |g: f32| ((util::gain_to_db(g) + 60.0) / 60.0).clamp(0.0, 1.0);
        self.values.meter_in = level(shared.meter_in.load(std::sync::atomic::Ordering::Relaxed));
        self.values.meter_out = level(shared.meter_out.load(std::sync::atomic::Ordering::Relaxed));
        shared.block_meters(&mut self.values.blocks);
        self.values.time = (now - self.start).as_secs_f32();
        if let (Some(cur), None) = (&self.current, &self.anim_error) {
            let t = Instant::now();
            if let Err(e) = cur.preset.animate(&mut self.values, dt) {
                self.anim_error = Some(format!("animate() disabled: {e}"));
            }
            let us = t.elapsed().as_secs_f32() * 1e6;
            self.lua_us = if self.lua_us == 0.0 { us } else { self.lua_us * 0.9 + us * 0.1 };
        }

        let mut action = None;
        self.header(ctx, &mut action);
        self.footer(ctx);
        self.sidebar(ctx, setter, params, shared);
        self.preset_panel(ctx, setter, params, shared);

        match action {
            Some(Action::Switch(name)) => self.load(ctx, setter, params, shared, &name, true),
            Some(Action::Reload) => {
                self.available = scan_presets();
                if let Some(name) = self.current.as_ref().map(|c| c.name.clone()) {
                    self.load(ctx, setter, params, shared, &name, false);
                }
            }
            None => {}
        }

        // Animated panels need continuous frames; a static one could skip this.
        ctx.request_repaint();
    }

    /// Loads a preset. On failure the previous preset keeps running and the
    /// error is shown in the footer, so hot-reload typos don't blank the UI.
    /// `reset`: apply the preset's defaults (preset switch / fresh instance)
    /// instead of keeping the values restored from the host project.
    fn load(
        &mut self,
        ctx: &egui::Context,
        setter: &ParamSetter,
        params: &FxParams,
        shared: &Shared,
        name: &str,
        reset: bool,
    ) {
        let dir = presets_root().join(name);
        let t = Instant::now();
        let result = Preset::load(&dir).map_err(|e| e.to_string()).and_then(|p| {
            let panel = match p.ui {
                Some(_) => Some(SvgPanel::load(&p).map_err(|e| e.to_string())?),
                None => None,
            };
            Ok((p, panel))
        });
        let (preset, panel) = match result {
            Ok(ok) => ok,
            Err(e) => {
                self.error = Some(format!("{name}: {e}"));
                return;
            }
        };

        *params.preset_name.write().unwrap() = name.to_string();
        if reset {
            let mut stored = params.param_values.write().unwrap();
            stored.clear();
            for p in &preset.params {
                stored.insert(p.id.clone(), p.default_plain());
            }
            drop(stored);
            for i in 0..preset.macros.len() {
                let p = &params.macros[i].value;
                setter.begin_set_parameter(p);
                setter.set_parameter_normalized(p, preset.macro_default(i));
                setter.end_set_parameter(p);
            }
        }
        self.free = params.free_values(&preset);
        shared.publish(&preset, &self.free);
        params.set_macro_names(&preset);
        setter.raw_context.rescan_param_info();
        ctx.set_visuals(theme_visuals(&preset.theme));

        nih_log!("loaded preset '{name}' (reset={reset})");
        self.current = Some(Loaded {
            name: name.to_string(),
            mtime: preset_mtime(&dir, &preset),
            dir,
            preset,
            panel,
            texture: None,
            load_ms: t.elapsed().as_secs_f32() * 1000.0,
        });
        self.error = None;
        self.anim_error = None;
        self.values.vars.clear();
        self.drag = None;
    }

    fn set_free_param(&mut self, params: &FxParams, shared: &Shared, index: usize, plain: f32) {
        let Some(cur) = &self.current else { return };
        self.free[index] = plain;
        shared.set_free(index, plain);
        params
            .param_values
            .write()
            .unwrap()
            .insert(cur.preset.params[index].id.clone(), plain);
    }

    fn poll_hot_reload(&mut self, ctx: &egui::Context, setter: &ParamSetter, params: &FxParams, shared: &Shared) {
        if !self.hot_reload || self.last_poll.elapsed().as_millis() < 400 {
            return;
        }
        self.last_poll = Instant::now();
        let Some(cur) = &mut self.current else { return };
        let mtime = preset_mtime(&cur.dir, &cur.preset);
        if mtime != cur.mtime {
            // Remember the attempt even if it fails, so we don't retry every poll.
            cur.mtime = mtime;
            let name = cur.name.clone();
            self.load(ctx, setter, params, shared, &name, false);
        }
    }

    fn header(&mut self, ctx: &egui::Context, action: &mut Option<Action>) {
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.strong("ScriptFX");
                ui.separator();
                let current = self.current.as_ref().map(|c| c.name.clone()).unwrap_or_default();
                let label = self.current.as_ref().map_or("—".to_string(), |c| c.preset.name.clone());
                egui::ComboBox::from_id_salt("preset")
                    .selected_text(label)
                    .width(180.0)
                    .show_ui(ui, |ui| {
                        for name in &self.available {
                            if ui.selectable_label(*name == current, name).clicked() && *name != current {
                                *action = Some(Action::Switch(name.clone()));
                            }
                        }
                    });
                if ui.button("⟳ Reload").clicked() {
                    *action = Some(Action::Reload);
                }
                ui.checkbox(&mut self.hot_reload, "hot reload");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(c) = &self.current {
                        if let Some(a) = &c.preset.author {
                            ui.weak(format!("by {a}"));
                        }
                        if c.preset.has_animator() {
                            ui.weak("· scripted animation ·");
                        }
                    }
                });
            });
            ui.add_space(2.0);
        });
    }

    fn footer(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("footer").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let w = ui.available_width() - 90.0;
                ui.add(
                    egui::TextEdit::singleline(&mut self.prompt)
                        .hint_text("Describe an effect, e.g. \"dusty tape with slow wobble\" (LLM not wired yet)")
                        .desired_width(w),
                );
                ui.add_enabled(false, egui::Button::new("✨ Generate"));
            });
            ui.horizontal(|ui| {
                let presets = presets_root();
                if let Some(c) = &self.current {
                    if let Some(p) = &c.panel {
                        let s = p.stats;
                        ui.weak(format!(
                            "load {:.1}ms · lua {:.0}µs · patch {:.0}µs · usvg {:.2}ms · resvg {:.2}ms · {} renders",
                            c.load_ms,
                            self.lua_us,
                            s.patch_us,
                            s.parse_us / 1000.0,
                            s.render_us / 1000.0,
                            s.renders
                        ));
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.weak(format!("presets: {}", presets.display()));
                });
            });
            let warnings = self
                .current
                .as_ref()
                .and_then(|c| c.panel.as_ref())
                .map(|p| p.warnings.clone())
                .unwrap_or_default();
            for msg in self.error.iter().chain(&self.anim_error) {
                ui.colored_label(Color32::from_rgb(255, 110, 110), msg);
            }
            for w in warnings {
                ui.colored_label(Color32::from_rgb(240, 200, 90), format!("⚠ {w}"));
            }
            ui.add_space(4.0);
        });
    }

    fn sidebar(&mut self, ctx: &egui::Context, setter: &ParamSetter, params: &FxParams, shared: &Shared) {
        let Some(cur) = &self.current else { return };
        let preset = &cur.preset;
        let mut free_change: Option<(usize, f32)> = None;

        egui::SidePanel::right("sidebar")
            .resizable(false)
            .exact_width(250.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_space(6.0);
                    ui.heading("Macros");
                    ui.weak("host-automatable");
                    ui.add_space(4.0);
                    for (slot, m) in preset.macros.iter().enumerate() {
                        let p = &params.macros[slot].value;
                        ui.label(format!("{}   (slot {})", m.name, slot + 1));
                        ui.add(
                            egui::Slider::from_get_set(0.0..=1.0, |nv| {
                                // egui's Slider calls the setter every frame (clamping),
                                // so only forward real changes to the host.
                                if let Some(v) = nv.filter(|v| {
                                    (*v as f32 - p.unmodulated_normalized_value()).abs() > 1e-6
                                }) {
                                    setter.begin_set_parameter(p);
                                    setter.set_parameter_normalized(p, v as f32);
                                    setter.end_set_parameter(p);
                                }
                                p.unmodulated_normalized_value() as f64
                            })
                            .custom_formatter(|n, _| preset.macro_text(slot, n as f32))
                            .custom_parser(|_| None),
                        );
                        ui.add_space(2.0);
                    }
                    if preset.macros.len() < MACRO_SLOTS {
                        ui.weak(format!("slots {}–{MACRO_SLOTS} unused", preset.macros.len() + 1));
                    }

                    ui.separator();
                    ui.heading("Params");
                    ui.weak("internal · saved with the project");
                    ui.add_space(4.0);
                    for (i, spec) in preset.params.iter().enumerate() {
                        let norm = self.values.params.get(i).copied().unwrap_or(0.0);
                        match preset.controls[i] {
                            Control::Free => {
                                ui.label(&spec.name);
                                let mut v = norm;
                                let resp = ui.add(
                                    egui::Slider::new(&mut v, 0.0..=1.0)
                                        .custom_formatter(|n, _| spec.display(n as f32))
                                        .custom_parser(|_| None),
                                );
                                if resp.changed() {
                                    free_change = Some((i, spec.plain(v)));
                                }
                            }
                            Control::Macro { slot, .. } => {
                                ui.horizontal(|ui| {
                                    ui.label(&spec.name);
                                    ui.weak(format!("via {}", preset.macros[slot].name));
                                });
                                ui.add(egui::ProgressBar::new(norm).text(spec.display(norm)));
                            }
                        }
                        ui.add_space(2.0);
                    }

                    ui.separator();
                    ui.label("Meters");
                    ui.add(egui::ProgressBar::new(self.values.meter_in).text("in"));
                    ui.add(egui::ProgressBar::new(self.values.meter_out).text("out"));
                    if !self.values.vars.is_empty() {
                        ui.separator();
                        ui.label("animate() vars");
                        let mut vars: Vec<_> = self.values.vars.iter().collect();
                        vars.sort_by(|a, b| a.0.cmp(b.0));
                        for (k, v) in vars {
                            ui.weak(format!("{k} = {v:.3}"));
                        }
                    }
                });
            });

        if let Some((i, plain)) = free_change {
            self.set_free_param(params, shared, i, plain);
        }
    }

    fn preset_panel(&mut self, ctx: &egui::Context, setter: &ParamSetter, params: &FxParams, shared: &Shared) {
        let mut free_change: Option<(usize, f32)> = None;
        egui::CentralPanel::default().show(ctx, |ui| {
            let avail = ui.available_rect_before_wrap();
            let Some(cur) = &mut self.current else {
                ui.centered_and_justified(|ui| ui.label("No preset loaded"));
                return;
            };
            let Some(panel) = &mut cur.panel else {
                ui.centered_and_justified(|ui| ui.label("Preset has no SVG UI — use the sidebar"));
                return;
            };

            // Fit the SVG into the available space, keeping aspect ratio.
            let (sw, sh) = panel.size();
            let scale = (avail.width() / sw).min(avail.height() / sh);
            let rect = Rect::from_center_size(avail.center(), vec2(sw * scale, sh * scale));
            let ppp = ctx.pixels_per_point();

            if panel.update(&self.values, (rect.width() * ppp) as u32, (rect.height() * ppp) as u32) {
                let pm = panel.pixmap().expect("rendered");
                let img = ColorImage::from_rgba_premultiplied(
                    [pm.width() as usize, pm.height() as usize],
                    pm.data(),
                );
                match &mut cur.texture {
                    Some(t) => t.set(img, TextureOptions::LINEAR),
                    None => cur.texture = Some(ctx.load_texture("preset-panel", img, TextureOptions::LINEAR)),
                }
            }
            if let Some(t) = &cur.texture {
                ui.painter().image(
                    t.id(),
                    rect,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }

            // Interaction: vertical drag on bound elements edits macros/params.
            let resp = ui.allocate_rect(rect, Sense::click_and_drag());
            let to_svg = |p: Pos2| ((p.x - rect.min.x) / scale, (p.y - rect.min.y) / scale);
            let hit = |p: Option<Pos2>| -> Option<(Source, f32)> {
                let (x, y) = to_svg(p?);
                panel.drag_target_at(x, y).map(|d| (d.target.clone(), d.range))
            };

            if self.drag.is_some() || hit(resp.hover_pos()).is_some() {
                ctx.set_cursor_icon(CursorIcon::ResizeVertical);
            }
            let hit_start = hit(resp.interact_pointer_pos());
            let preset = &cur.preset;

            if resp.drag_started() {
                if let Some((target, range)) = &hit_start {
                    if let Some(what) = drag_what(preset, target) {
                        let start = match what {
                            DragWhat::Macro(slot) => {
                                setter.begin_set_parameter(&params.macros[slot].value);
                                self.values.macros[slot]
                            }
                            DragWhat::Param(i) => self.values.params[i],
                        };
                        self.drag = Some(DragState { what, range: *range, start, accum: 0.0 });
                    }
                }
            }
            if resp.dragged() {
                if let Some(d) = &mut self.drag {
                    let fine = if ui.input(|i| i.modifiers.shift) { 0.2 } else { 1.0 };
                    d.accum += resp.drag_delta().y / scale * fine;
                    let v = (d.start - d.accum / d.range).clamp(0.0, 1.0);
                    match d.what {
                        DragWhat::Macro(slot) => setter.set_parameter_normalized(&params.macros[slot].value, v),
                        DragWhat::Param(i) => free_change = Some((i, preset.params[i].plain(v))),
                    }
                }
            }
            if resp.drag_stopped() {
                if let Some(DragState { what: DragWhat::Macro(slot), .. }) = self.drag.take() {
                    setter.end_set_parameter(&params.macros[slot].value);
                }
            }
            if resp.double_clicked() {
                // Double-click resets to the preset default.
                match hit_start.as_ref().and_then(|(t, _)| drag_what(preset, t)) {
                    Some(DragWhat::Macro(slot)) => {
                        let param = &params.macros[slot].value;
                        setter.begin_set_parameter(param);
                        setter.set_parameter_normalized(param, preset.macro_default(slot));
                        setter.end_set_parameter(param);
                    }
                    Some(DragWhat::Param(i)) => free_change = Some((i, preset.params[i].default_plain())),
                    None => {}
                }
            }
        });
        if let Some((i, plain)) = free_change {
            self.set_free_param(params, shared, i, plain);
        }
    }
}

/// Resolves a drag target: a macro-driven param drags its macro instead.
fn drag_what(preset: &Preset, target: &Source) -> Option<DragWhat> {
    match *target {
        Source::Macro(slot) => Some(DragWhat::Macro(slot)),
        Source::Param(i) => match preset.controls[i] {
            Control::Free => Some(DragWhat::Param(i)),
            Control::Macro { slot, .. } => Some(DragWhat::Macro(slot)),
        },
        _ => None,
    }
}

fn scan_presets() -> Vec<String> {
    preset::discover(&presets_root())
        .into_iter()
        .filter_map(|p| p.file_name()?.to_str().map(String::from))
        .collect()
}

fn preset_mtime(dir: &Path, preset: &Preset) -> Option<SystemTime> {
    let mut files = vec![dir.join("preset.lua")];
    if let Some(ui) = &preset.ui {
        files.push(ui.svg.clone());
    }
    files
        .iter()
        .filter_map(|f| std::fs::metadata(f).and_then(|m| m.modified()).ok())
        .max()
}

/// The preset theme also styles the plugin chrome.
fn theme_visuals(theme: &Theme) -> Visuals {
    let mut v = Visuals::dark();
    let c = |k: &str, d: Color32| theme.get(k).and_then(|h| Color32::from_hex(h).ok()).unwrap_or(d);
    let bg = c("bg", v.panel_fill);
    let panel = c("panel", v.extreme_bg_color);
    let edge = c("edge", v.widgets.noninteractive.bg_stroke.color);
    let accent = c("accent", v.selection.bg_fill);
    let text = c("text", Color32::LIGHT_GRAY);

    v.panel_fill = bg;
    v.window_fill = bg;
    v.extreme_bg_color = panel;
    v.faint_bg_color = panel;
    v.hyperlink_color = accent;
    v.override_text_color = Some(text);
    v.selection.bg_fill = accent;
    v.selection.stroke = Stroke::new(1.0, text);
    v.slider_trailing_fill = true;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, edge);
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.open] {
        w.bg_fill = panel;
        w.weak_bg_fill = panel;
    }
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, accent);
    v.widgets.active.bg_fill = accent;
    v.widgets.active.weak_bg_fill = accent;
    v
}
