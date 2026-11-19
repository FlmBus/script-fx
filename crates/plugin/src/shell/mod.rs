//! The shell: the plugin's own UI (iced), in the "ScriptFX Shell" design system.
//! Top bar over the patch window (ControlPanel + notch, sheets floating over it on both sides)
//! over the signal line.
//!
//! One module per section, each with its own state, `Message`, `update` and `view`:
//! - `top_bar`      the wordmark, sheet openers (library left; controls, chat, settings right)
//! - `library`      the patch library (sheet, left)
//! - `notch`        reload / snapshot selector, save, load
//! - `stage`        the patch window: the ControlPanel and its pointer input
//! - `chat`         the chat sheet (Listen lives here)
//! - `controls`     the controls sheet (macros, params, patch settings)
//! - `settings`     the settings sheet (theme, interface size, passive mode)
//! - `signal_line`  IN → blocks → OUT (view only)
//! - `empty`        the window when no patch is loaded
//! - `entry`        typing a value (right-click a knob)
//!
//! `session` holds the non-UI state (library scan, loaded patch, hot reload, values).
//! This file owns what several sections share (which sheet is open, the frame clock), routes
//! messages and applies param changes. Sections report what the shell must act on as an
//! `Event` from their `update`; they never reach into each other.

pub mod theme;

mod brand;
mod chat;
mod controls;
mod empty;
mod entry;
mod icons;
mod library;
mod notch;
mod session;
mod settings;
mod sheet;
mod signal_line;
mod slide;
mod stage;
mod top_bar;

use std::sync::Arc;
use std::time::{Duration, Instant};

use nice_plug_iced::{
    IcedEditor, IcedEditorState, IcedNiceContext, IcedNiceSettings, PersistentState, application, create_iced_editor,
    iced::{
        self, Border, Element, Fill, Padding, PollSubNotifier, Subscription, Task, Theme, alignment,
        animation::{Animation, Easing},
        futures::{Stream, channel::mpsc},
        widget::{button, column, container, pin, row, stack, text},
    },
};

use crate::listen::Listen;
use crate::user_settings::{self, UserSettings};
use crate::{FxParams, Shared};
use session::Session;

/// What the editor keeps between window opens.
pub struct Plugin {
    pub params: Arc<FxParams>,
    pub shared: Arc<Shared>,
}

pub fn create(
    editor_state: Arc<IcedEditorState>,
    notifier: PollSubNotifier,
    params: Arc<FxParams>,
    shared: Arc<Shared>,
) -> Option<IcedEditor> {
    // Frame pacing (see vendor/baseview/PATCHES.md): always_redraw draws on every baseview frame
    // (its 15 ms frame timer); animated components and meters need that anyway. The poll timer
    // also draws, and at its default 1/64 s it fired ~1000×/s here: keep it slow (the editor
    // doesn't use iced's poll events; its own frame clock is `frames()`).
    let settings = IcedNiceSettings::new().with_always_redraw(true).with_poll_interval(Duration::from_millis(250));
    create_iced_editor(editor_state, Plugin { params, shared }, notifier, settings, |persistent, ctx| {
        let mut app = application(persistent, ctx, Shell::new, Shell::update, Shell::view)
            .theme(Shell::theme)
            .scale_factor(Shell::scale_factor)
            .style(Shell::style)
            .subscription(Shell::subscription)
            .default_font(theme::font::SANS);
        for font in theme::font::FILES {
            app = app.font(*font);
        }
        Ok(app.run())
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Sheet {
    Chat,
    Controls,
    Settings,
}

pub struct Shell {
    plugin: PersistentState<Plugin>,
    ctx: IcedNiceContext,
    session: Session,
    restored: bool,
    library: library::Library,
    chat: chat::Chat,
    empty: empty::Empty,
    listen: Listen,
    /// The library sheet (left slot) and its slide.
    library_open: bool,
    library_slide: Animation<bool>,
    /// The open right-hand sheet; `shown` keeps the last one drawn while it slides out.
    sheet: Option<Sheet>,
    shown: Sheet,
    slide: Animation<bool>,
    now: Instant,
    /// Macro slot being dragged on the controls sheet (host gesture open).
    dragging_macro: Option<usize>,
    /// Passive mode (not built yet: the top bar's switch is disabled).
    passive: bool,
    /// Per-user settings (UI scale), saved on change.
    settings: UserSettings,
    dev_stats: bool,
    /// Dev aid: `SCRIPTFX_DEV_RESCALE=1.5` sets the UI scale 1 s after opening (like −/+).
    dev_rescale: Option<(Instant, f32)>,
    /// The open value entry (right-click on a knob).
    entry: Option<entry::Entry>,
    /// Tasks produced while handling a message (focus the value entry).
    tasks: Vec<Task<Message>>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Frame(Instant),
    TopBar(top_bar::Message),
    Library(library::Message),
    Notch(notch::Message),
    Chat(chat::Message),
    Controls(controls::Message),
    Settings(settings::Message),
    Empty(empty::Message),
    Panel(stage::Pointer),
    DismissError,
    Entry(entry::Message),
    /// Escape anywhere (also when a text field has the focus).
    Escape,
}

impl Shell {
    fn new(plugin: PersistentState<Plugin>, ctx: IcedNiceContext) -> Self {
        Self {
            plugin,
            ctx,
            session: Session::new(),
            restored: false,
            library: library::Library::new(),
            chat: chat::Chat::new(),
            empty: empty::Empty::default(),
            listen: Listen::new(),
            sheet: None,
            shown: Sheet::Chat,
            slide: Animation::new(false).duration(Duration::from_millis(260)).easing(Easing::EaseOutCubic),
            library_open: false,
            library_slide: Animation::new(false).duration(Duration::from_millis(260)).easing(Easing::EaseOutCubic),
            now: Instant::now(),
            dragging_macro: None,
            passive: false,
            settings: { let s = UserSettings::load(); apply_theme(s.theme); s },
            dev_stats: std::env::var_os("SCRIPTFX_DEV_STATS").is_some(),
            entry: None,
            tasks: Vec::new(),
            dev_rescale: std::env::var("SCRIPTFX_DEV_RESCALE").ok().and_then(|s| s.parse().ok()).map(|v| (Instant::now() + Duration::from_secs(1), v)),
        }
    }

    fn theme(&self) -> Option<Theme> {
        Some(Theme::custom(
            "ScriptFX Shell",
            iced::theme::Palette {
                background: theme::c().frame,
                text: theme::c().frame_ink_3,
                primary: theme::c().frame_invert,
                success: theme::c().led_ok,
                warning: theme::c().led_reduce,
                danger: theme::c().led_clip,
            },
        ))
    }

    /// The user's UI scale, on top of the system's. When it changes, iced resizes the window
    /// (through the host) and lays the shell out at the new scale.
    fn scale_factor(&self) -> f32 {
        self.settings.ui_scale
    }

    fn style(&self, _: &Theme) -> iced::theme::Style {
        iced::theme::Style { background_color: theme::c().frame, text_color: theme::c().frame_ink_3 }
    }

    /// The frame clock drives everything that moves: meters, animated components, the sheet
    /// slide, hot reload, Listen progress.
    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            Subscription::run(frames).map(Message::Frame),
            iced::event::listen_with(|event, _, _| match event {
                iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape), .. }) => {
                    Some(Message::Escape)
                }
                _ => None,
            }),
        ])
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        self.handle(message);
        // Patch fonts found by a library scan: hand them to iced.
        let fonts = std::mem::take(&mut self.session.new_fonts);
        let fonts = fonts.into_iter().map(|bytes| iced::font::load(bytes).discard());
        Task::batch(fonts.chain(std::mem::take(&mut self.tasks)))
    }

    fn handle(&mut self, message: Message) {
        let Plugin { params, shared } = &*self.plugin;
        let (params, shared) = (params.clone(), shared.clone());
        let ctx = &self.ctx.nice_context.clone();
        match message {
            Message::Frame(now) => {
                // "System" follows the OS while open (read off-thread, see theme::system_is_light).
                apply_theme(self.settings.theme);
                self.now = now;
                if !self.restored {
                    self.restored = true;
                    self.session.restore(ctx, &params, &shared);
                    self.reveal_current();
                    // A fresh instance has no patch: show the library to pick one.
                    if self.session.current.is_none() {
                        self.set_library(true);
                    }
                    // Dev aid: SCRIPTFX_DEV_SHEET=chat|controls opens a sheet at start.
                    match std::env::var("SCRIPTFX_DEV_SHEET").as_deref() {
                        Ok("chat") => self.open_sheet(Some(Sheet::Chat)),
                        Ok("controls") => self.open_sheet(Some(Sheet::Controls)),
                        Ok("settings") => self.open_sheet(Some(Sheet::Settings)),
                        Ok("library") => self.set_library(true),
                        // The value entry of the first macro, as if right-clicked at Neon's Tape knob.
                        Ok("entry") => self.open_entry(stage::Target::Macro(0), iced::Point::new(165.0, 388.0)),
                        _ => {}
                    }
                    // Dev aid: SCRIPTFX_DEV_AUTOLISTEN=10 starts a 10 s listen right away.
                    if let Some(s) = std::env::var("SCRIPTFX_DEV_AUTOLISTEN").ok().and_then(|s| s.parse::<f32>().ok()) {
                        self.listen.seconds = s;
                        self.listen.start(&shared);
                        self.open_sheet(Some(Sheet::Chat));
                    }
                }
                if let Some((at, v)) = self.dev_rescale {
                    if now >= at {
                        self.dev_rescale = None;
                        self.set_ui_scale(v);
                    }
                }
                self.session.tick(ctx, &params, &shared);
                if let Some(result) = self.listen.poll(&shared) {
                    self.chat.capture_done(result);
                    self.open_sheet(Some(Sheet::Chat));
                }
                icons::set_scale(self.ctx.window_size().scale_factor as f32);
                let scale = stage::panel_fit().0 * self.ctx.window_size().scale_factor as f32;
                let Session { current, values, .. } = &mut self.session;
                if let Some(cur) = current {
                    if let Some(panel) = &mut cur.panel {
                        panel.update(&cur.patch, values, scale);
                    }
                }
            }
            Message::TopBar(msg) => match msg {
                top_bar::Message::ToggleLibrary => self.set_library(!self.library_open),
                top_bar::Message::ToggleControls => self.toggle_sheet(Sheet::Controls),
                top_bar::Message::ToggleChat => self.toggle_sheet(Sheet::Chat),
                top_bar::Message::ToggleSettings => self.toggle_sheet(Sheet::Settings),
            },
            Message::Library(msg) => match self.library.update(msg) {
                Some(library::Event::Load(folder)) => {
                    self.session.load(ctx, &params, &shared, &folder, true);
                    self.reveal_current();
                }
                Some(library::Event::Unload) => {
                    self.session.unload(ctx, &params, &shared);
                    // Chat and settings stay open; the controls belong to the patch.
                    if self.sheet == Some(Sheet::Controls) {
                        self.open_sheet(None);
                    }
                }
                Some(library::Event::Close) => self.set_library(false),
                None => {}
            },
            Message::Notch(msg) => match msg {
                notch::Message::Reload => self.session.reload(ctx, &params, &shared),
            },
            Message::Chat(msg) => match self.chat.update(msg) {
                Some(chat::Event::Close) => self.open_sheet(None),
                Some(chat::Event::Listen) => self.listen.start(&shared),
                Some(chat::Event::CancelListen) => self.listen.cancel(&shared),
                Some(chat::Event::CycleLength) => {
                    self.listen.seconds = if self.listen.seconds >= 30.0 { 10.0 } else { self.listen.seconds + 10.0 };
                }
                None => {}
            },
            Message::Controls(msg) => self.controls(msg, &params, &shared),
            Message::Settings(msg) => match msg {
                settings::Message::Close => self.open_sheet(None),
                settings::Message::SetTheme(mode) => {
                    self.settings.theme = mode;
                    apply_theme(mode);
                    self.settings.save();
                }
                settings::Message::ScaleDown => self.set_ui_scale(self.settings.ui_scale - user_settings::UI_SCALE_STEP),
                settings::Message::ScaleUp => self.set_ui_scale(self.settings.ui_scale + user_settings::UI_SCALE_STEP),
                settings::Message::TogglePassive => self.passive = !self.passive,
            },
            Message::Empty(msg) => match self.empty.update(msg) {
                Some(e) => match e {},
                None => {}
            },
            Message::Panel(ev) => {
                // A click into the panel closes the sheets and a value entry.
                if matches!(ev, stage::Pointer::Pressed(_) | stage::Pointer::RightPressed(_)) {
                    self.set_library(false);
                    self.open_sheet(None);
                    self.entry = None;
                }
                let Some(cur) = &mut self.session.current else { return };
                let Some(panel) = &mut cur.panel else { return };
                let changes = panel.pointer(ev, &cur.patch, &self.session.values);
                for c in changes {
                    match c {
                        stage::Change::OpenEntry { target, at } => self.open_entry(target, at),
                        c => self.apply(c, &params, &shared),
                    }
                }
            }
            Message::Entry(entry::Message::Edit(text)) => {
                if let Some(e) = &mut self.entry {
                    e.text = text;
                    e.invalid = false;
                }
            }
            Message::Entry(entry::Message::Submit) => self.submit_entry(&params, &shared),
            Message::Escape => {
                if self.entry.take().is_none() {
                    self.set_library(false);
                    self.open_sheet(None);
                }
            }
            Message::DismissError => self.session.error = None,
        }
    }

    fn controls(&mut self, msg: controls::Message, params: &FxParams, shared: &Shared) {
        match msg {
            controls::Message::Close => self.open_sheet(None),
            controls::Message::Macro(slot, v) => {
                if self.dragging_macro != Some(slot) {
                    if let Some(prev) = self.dragging_macro.take() {
                        self.apply(stage::Change::EndMacro(prev), params, shared);
                    }
                    self.apply(stage::Change::BeginMacro(slot), params, shared);
                    self.dragging_macro = Some(slot);
                }
                self.apply(stage::Change::SetMacro(slot, v), params, shared);
            }
            controls::Message::MacroReleased(slot) => {
                if self.dragging_macro.take() == Some(slot) {
                    self.apply(stage::Change::EndMacro(slot), params, shared);
                }
            }
            controls::Message::Param(i, norm) => {
                if let Some(plain) = self.session.current.as_ref().and_then(|c| c.patch.params.get(i)).map(|p| p.plain(norm)) {
                    self.session.set_free(params, shared, i, plain);
                }
            }
            controls::Message::HotReload(on) => self.session.hot_reload = on,
        }
    }

    /// Applies a param change: macros go to the host (automation gestures), free params into
    /// the playing graph and the project state.
    fn apply(&mut self, change: stage::Change, params: &FxParams, shared: &Shared) {
        let setter = self.ctx.nice_context.param_setter();
        match change {
            stage::Change::BeginMacro(slot) => setter.begin_set_parameter(&params.macros[slot].value),
            stage::Change::SetMacro(slot, v) => setter.set_parameter_normalized(&params.macros[slot].value, v),
            stage::Change::EndMacro(slot) => setter.end_set_parameter(&params.macros[slot].value),
            stage::Change::SetFree(i, plain) => self.session.set_free(params, shared, i, plain),
            stage::Change::OpenEntry { .. } => {}
        }
    }

    fn set_ui_scale(&mut self, v: f32) {
        let v = user_settings::snap_ui_scale(v);
        if v == self.settings.ui_scale {
            return;
        }
        self.settings.ui_scale = v;
        // Remembered for the next window open; `scale_factor()` resizes this one.
        self.ctx.set_user_scale_factor(v);
        self.settings.save();
    }

    /// Right-click on a control: a value entry with its current value, selected.
    fn open_entry(&mut self, target: stage::Target, at: iced::Point) {
        let Some(cur) = &self.session.current else { return };
        let (p, v) = (&cur.patch, &self.session.values);
        let (label, text) = match target {
            stage::Target::Macro(slot) => (p.macros[slot].name.clone(), p.macro_text(slot, v.macros[slot])),
            stage::Target::Param(i) => (p.params[i].name.clone(), p.params[i].display(v.params.get(i).copied().unwrap_or(0.0))),
        };
        self.entry = Some(entry::Entry { target, at, label, text, invalid: false });
        self.tasks.push(iced::widget::operation::focus(entry::input_id()));
        self.tasks.push(iced::widget::operation::select_all(entry::input_id()));
    }

    /// Enter in the value entry: parse in the control's unit and apply, or mark it invalid.
    fn submit_entry(&mut self, params: &FxParams, shared: &Shared) {
        let (Some(e), Some(cur)) = (&mut self.entry, &self.session.current) else { return };
        let p = &cur.patch;
        let changes = match e.target {
            stage::Target::Macro(slot) => p
                .macro_from_text(slot, &e.text)
                .map(|v| vec![stage::Change::BeginMacro(slot), stage::Change::SetMacro(slot, v), stage::Change::EndMacro(slot)]),
            stage::Target::Param(i) => p.params.get(i).and_then(|s| s.parse_plain(&e.text)).map(|v| vec![stage::Change::SetFree(i, v)]),
        };
        match changes {
            Some(changes) => {
                self.entry = None;
                for c in changes {
                    self.apply(c, params, shared);
                }
            }
            None => e.invalid = true,
        }
    }

    fn reveal_current(&mut self) {
        if let Some(cur) = &self.session.current {
            self.library.reveal(&cur.folder);
        }
    }

    /// Left and right sheets are exclusive: opening one closes the other (both slide).
    fn set_library(&mut self, open: bool) {
        if open {
            self.open_sheet(None);
        }
        if self.library_open != open {
            self.library_open = open;
            self.now = Instant::now();
            self.library_slide.go_mut(open, self.now);
        }
    }

    fn toggle_sheet(&mut self, s: Sheet) {
        self.open_sheet(if self.sheet == Some(s) { None } else { Some(s) });
    }

    /// Opens a right-hand sheet (closing the library, see `set_library`), or closes it.
    fn open_sheet(&mut self, s: Option<Sheet>) {
        if s.is_some() {
            self.set_library(false);
        }
        let was_open = self.sheet.is_some();
        self.sheet = s;
        if let Some(s) = s {
            self.shown = s;
        }
        // Switching between sheets swaps the content; only opening/closing slides.
        if was_open != s.is_some() {
            self.now = Instant::now();
            self.slide.go_mut(s.is_some(), self.now);
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let s = &self.session;
        let gutters = Padding { top: theme::SPACE_GUTTER, right: theme::SPACE_GUTTER, left: theme::SPACE_GUTTER, bottom: 0.0 };

        let main: Element<'_, Message> = match &s.current {
            Some(_) => column![self.window(), signal_line::view(s.current.as_ref().map(|c| &c.patch), &s.values)].into(),
            None => {
                // The card slides aside with open sheets, like the notch.
                let empty = self.empty.view(self.library_t() * library_width(), self.slide_t() * sheet_width()).map(Message::Empty);
                let mut layers = stack![container(empty).padding(Padding { bottom: theme::SPACE_GUTTER, ..Padding::ZERO })].width(Fill).height(Fill);
                for sheet in [self.library_layer(), self.sheet_layer()].into_iter().flatten() {
                    layers = layers.push(sheet);
                }
                container(layers.push(self.error_card())).padding(gutters).into()
            }
        };

        let top_bar = top_bar::view(top_bar::Props {
            library_open: self.library_open,
            controls_available: s.current.is_some(),
            controls_open: self.sheet == Some(Sheet::Controls),
            chat_open: self.sheet == Some(Sheet::Chat),
            settings_open: self.sheet == Some(Sheet::Settings),
        })
        .map(Message::TopBar);
        container(column![top_bar, main]).style(theme::frame).width(Fill).height(Fill).into()
    }

    /// The patch window with the shell's intrusions: the notch and the sheets.
    fn window(&self) -> Element<'_, Message> {
        let Some(cur) = &self.session.current else { return column![].into() };
        let bg = session::hex(cur.patch.theme.color("bg")).unwrap_or(theme::c().frame_raised);

        // While sheets are open the notch centres on the part of the window they leave visible,
        // gliding along with them.
        let notch = notch::view(notch::Props {
            snapshot: "Init",
            sync: if self.session.error.is_some() { notch::Sync::Error } else { notch::Sync::Ok },
        })
        .map(Message::Notch);
        let notch = container(notch)
            .width(Fill)
            .padding(Padding { left: self.library_t() * library_width(), right: self.slide_t() * sheet_width(), ..Padding::ZERO })
            .align_x(alignment::Horizontal::Center);

        let mut layers = stack![stage::view(&cur.patch, cur.panel.as_ref(), bg).map(Message::Panel), notch].width(Fill).height(Fill);
        for sheet in [self.library_layer(), self.sheet_layer()].into_iter().flatten() {
            layers = layers.push(sheet);
        }
        if let Some(e) = &self.entry {
            // Right of the click point, clamped into the stage.
            let (s, offset) = stage::panel_fit();
            let (w, h) = theme::stage_size();
            let x = (offset.x + e.at.x * s + 14.0).min(w - entry::SIZE.0 - 8.0).max(8.0);
            let y = (offset.y + e.at.y * s - entry::SIZE.1 / 2.0).min(h - entry::SIZE.1 - 8.0).max(8.0);
            layers = layers.push(pin(e.view().map(Message::Entry)).x(x).y(y));
        }
        layers = layers.push(self.error_card());
        if self.dev_stats {
            layers = layers.push(self.stats());
        }

        container(layers)
            .padding(Padding { top: theme::SPACE_GUTTER, right: theme::SPACE_GUTTER, left: theme::SPACE_GUTTER, bottom: 0.0 })
            .width(Fill)
            .height(Fill)
            .into()
    }

    /// 0 = right sheet hidden, 1 = fully in.
    fn slide_t(&self) -> f32 {
        self.slide.interpolate(0.0, 1.0, self.now)
    }

    /// 0 = library hidden, 1 = fully in.
    fn library_t(&self) -> f32 {
        self.library_slide.interpolate(0.0, 1.0, self.now)
    }

    /// The library sheet, over the left side of the window.
    fn library_layer(&self) -> Option<Element<'_, Message>> {
        let t = self.library_t();
        if t <= 0.0 {
            return None;
        }
        let s = &self.session;
        let sheet = self.library.view(&s.library, s.current.as_ref().map(|c| c.folder.as_str())).map(Message::Library);
        let sheet = container(sheet).height(Fill).padding(theme::SPACE_2);
        Some(slide::slide_left(sheet, library_width(), (1.0 - t) * library_width()).into())
    }

    /// The open (or sliding) paper sheet, over the right side of the window. The controls sheet
    /// needs a patch; the chat (global) and the settings don't.
    fn sheet_layer(&self) -> Option<Element<'_, Message>> {
        let t = self.slide_t();
        if t <= 0.0 {
            return None;
        }
        let sheet: Element<'_, Message> = match (self.shown, &self.session.current) {
            (Sheet::Chat, _) => self.chat.view(self.listen.status(&self.plugin.shared), self.listen.seconds).map(Message::Chat),
            (Sheet::Controls, Some(cur)) => controls::view(&cur.patch, &self.session.values, self.session.hot_reload).map(Message::Controls),
            (Sheet::Controls, None) => return None,
            (Sheet::Settings, _) => settings::view(settings::Props {
                theme: self.settings.theme,
                ui_scale: self.settings.ui_scale,
                passive: self.passive,
                passive_available: false,
            })
            .map(Message::Settings),
        };
        let sheet = container(sheet).height(Fill).padding(theme::SPACE_2);
        Some(slide::slide(sheet, sheet_width(), (1.0 - t) * sheet_width()).into())
    }

    /// The last load error, as a paper card at the bottom left of the window.
    fn error_card(&self) -> Element<'_, Message> {
        let Some(err) = &self.session.error else { return column![].into() };
        let card = container(
            row![
                column![
                    text("Couldn't load the patch").size(12.5).font(theme::font::PAPER_SEMIBOLD).color(theme::c().paper_ink),
                    text(err.as_str()).size(11).font(theme::font::PAPER_MONO).color(theme::c().paper_ink_2),
                    text("The previous version keeps running. Fix the file and it reloads.").size(11).font(theme::font::PAPER).color(theme::c().paper_muted),
                ]
                .spacing(4)
                .width(Fill),
                button(container(icons::icon(icons::CLOSE, 10.0, 2.5, theme::c().paper_ink)).center(Fill))
                    .on_press(Message::DismissError)
                    .width(24)
                    .height(24)
                    .padding(0)
                    .style(theme::paper_bare),
            ]
            .spacing(8),
        )
        .padding(12)
        .max_width(520)
        .style(|t: &Theme| container::Style {
            border: Border { radius: theme::radius(theme::RADIUS_FIELD), width: 1.0, color: theme::c().led_clip },
            shadow: theme::paper_sheet(t).shadow,
            ..theme::paper_raised(t)
        });
        container(card).width(Fill).height(Fill).padding(14).align_bottom(Fill).into()
    }

    /// Dev aid (`SCRIPTFX_DEV_STATS=1`): frame cost and component stats.
    fn stats(&self) -> Element<'_, Message> {
        let f = iced_wgpu::frame_stats::get();
        let gpu = f.gpu_ms.map_or("n/a".to_string(), |g| format!("{g:.2}"));
        let mut s = format!("{:.0} fps · render cpu {:.2} ms · gpu {gpu} ms", f.fps, f.render_ms);
        if let Some(p) = self.session.current.as_ref().and_then(|c| c.panel.as_ref()) {
            let st = p.stats();
            s += &format!(" · panel {:.2} ms, {} components, {} rasters", st.total_us / 1000.0, st.nodes, st.rasters);
        }
        container(text(s).size(10.5).font(theme::font::MONO).color(theme::alpha(theme::c().frame_ink, 0.7)))
            .width(Fill)
            .height(Fill)
            .padding(10)
            .align_right(Fill)
            .align_bottom(Fill)
            .into()
    }
}

/// Picks the palette for a theme mode.
fn apply_theme(mode: user_settings::ThemeMode) {
    use user_settings::ThemeMode::*;
    theme::set_light(match mode {
        System => theme::system_is_light(),
        Dark => false,
        Light => true,
    });
}

/// The library sheet plus its margin.
fn library_width() -> f32 {
    theme::SIZE_LIBRARY + 2.0 * theme::SPACE_2
}

/// A right-hand sheet plus its margin.
fn sheet_width() -> f32 {
    theme::SIZE_SHEET + 2.0 * theme::SPACE_2
}

/// ~60 Hz frame ticks from a helper thread (the thread-pool executor has no timers). The
/// thread ends when the editor closes (the receiver is dropped).
fn frames() -> impl Stream<Item = Instant> {
    let (mut tx, rx) = mpsc::channel(1);
    std::thread::spawn(move || {
        loop {
            let us = std::env::var("SCRIPTFX_FRAME_US").ok().and_then(|v| v.parse().ok()).unwrap_or(16_667);
            std::thread::sleep(Duration::from_micros(us));
            if let Err(e) = tx.try_send(Instant::now()) {
                if e.is_disconnected() {
                    break;
                }
            }
        }
    });
    rx
}
