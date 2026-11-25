//! The shell: the plugin's own UI (iced), in the "ScriptFX Shell" design system.
//! Top bar over the stage (the patch window: ControlPanel, chain or Matrix, the notch hanging
//! into it, sheets floating over it on both sides), the mod dock below it.
//!
//! One module per section, each with its own state, `Message`, `update` and `view`:
//! - `top_bar`      the wordmark, sheet openers (library left; controls, chat, settings right)
//! - `library`      the patch library (sheet, left)
//! - `notch`        view switch (Panel | Chain | Matrix); reload / snapshot selector, save, load
//! - `dock`         below the stage: macros, modulators (live mini displays), focus, and a
//!                  modulator's editor (a panel rising over the stage's lower part)
//! - `stage`        the patch window: the ControlPanel and its pointer input
//! - `chain_view`   the audio tree (notch: Chain), in place of the ControlPanel
//! - `matrix`       every modulation route as a table, add/remove (notch: Matrix)
//! - `routing`      how routes look (source names, colours, meta knobs) and the source drag
//! - `block_modal`  a block's modal (click a block in the chain; `motion` + `zoom` + the renderer's blur)
//! - `chat`         the chat sheet (Listen lives here)
//! - `controls`     the controls sheet (macros, params, patch settings)
//! - `settings`     the settings sheet (theme, interface size, passive mode)
//! - `empty`        the window when no patch is loaded
//! - `entry`        typing a value (right-click a knob)
//!
//! Drag & drop: pressing a source's title bar in the dock (a macro or a modulator) starts a
//! `SourceDrag`; once the pointer moves a chip follows it, knobs that can take it outline
//! themselves, and a release over one adds the route (`Session::add_route`). Released without
//! moving, it was a click: the dock focuses the source.
//!
//! `session` holds the non-UI state (library scan, loaded patch, hot reload, values).
//! This file owns what several sections share (which sheet is open, the frame clock), routes
//! messages and applies param changes. Sections report what the shell must act on as an
//! `Event` from their `update`; they never reach into each other.

pub mod theme;

mod block_display;
mod backdrop;
mod block_modal;
mod brand;
mod chain_view;
mod chat;
mod controls;
mod dock;
mod knob;
mod empty;
mod entry;
mod hide_cursor;
mod icons;
mod library;
mod matrix;
mod meta_menu;
mod motion;
mod notch;
mod passive;
mod pointer_lock;
mod routing;
mod press;
mod session;
mod settings;
mod sheet;
mod slide;
mod stage;
mod surface;
mod tabs;
mod top_bar;
mod zoom;

use std::sync::Arc;
use std::time::{Duration, Instant};

use nice_plug_iced::{
    IcedEditor, IcedEditorState, IcedNiceContext, IcedNiceSettings, PersistentState, application, create_iced_editor,
    iced::{
        self, Border, Center, Element, Fill, Padding, PollSubNotifier, Shadow, Subscription, Task, Theme, alignment,
        animation::{Animation, Easing},
        futures::{Stream, channel::mpsc},
        widget::{button, column, container, pin, row, space, stack, text},
    },
};

use nice_plug::context::gui::GuiContext;

use crate::captures;
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
    /// Passive mode: no patch, controls disabled, the passive card instead of the views.
    /// Mirrors `FxParams::passive` (saved with the project).
    passive: bool,
    /// The track this instance sits on, as the host names it (`IcedNiceContext::new_track_info`,
    /// VST3 `IInfoListener` / CLAP track-info). `None` = the host never told us.
    track: Option<String>,
    /// What the captures folder holds, for the settings sheet. Read when that sheet opens and
    /// after clearing (it walks the folder, so not per frame).
    captures: captures::Usage,
    /// Per-user settings (UI scale), saved on change.
    settings: UserSettings,
    /// An API key being typed (settings sheet); goes to the keychain, never to disk.
    ai_key: String,
    dev_stats: bool,
    /// Dev aid: `SCRIPTFX_DEV_RESCALE=1.5` sets the UI scale 1 s after opening (like −/+).
    dev_rescale: Option<(Instant, f32)>,
    /// The open value entry (right-click on a knob).
    entry: Option<entry::Entry>,
    /// A shell knob or meta knob is being dragged (the cursor is hidden meanwhile).
    dragging: bool,
    /// A meta knob's menu (right-click on it).
    meta_menu: Option<meta_menu::MetaMenu>,
    /// What the patch window shows (the notch's view switch).
    view: notch::View,
    /// The open block modal (block id) in the chain view.
    page: Option<String>,
    /// The chain ↔ modal transition (or a walk along the chain) while it runs.
    motion: Option<motion::Motion>,
    dock: dock::Dock,
    /// The Matrix's add row (the source picked).
    matrix: matrix::Matrix,
    /// A macro or modulator being dragged onto a knob.
    source_drag: Option<routing::SourceDrag>,
    /// The drag the last button release ended: the knob under the pointer reports the drop
    /// right after the window-wide release (see `hide_cursor`).
    dropped: Option<(routing::SourceDrag, Instant)>,
    /// Tasks produced while handling a message (focus the value entry).
    tasks: Vec<Task<Message>>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Frame(Instant),
    TopBar(top_bar::Message),
    Library(library::Message),
    MetaMenu(meta_menu::Message),
    /// A press anywhere on the page (surface, chain, block page, …): close what floats.
    Dismiss,
    /// The pointer moved while a source is dragged (window coordinates).
    Pointer(iced::Point),
    /// The left button went up somewhere: no drag runs any more.
    Released,
    Surface(surface::Message),
    Notch(notch::Message),
    Matrix(matrix::Message),
    Chain(chain_view::Event),
    Page(block_modal::Message),
    Dock(dock::Message),
    Chat(chat::Message),
    Controls(controls::Message),
    Settings(settings::Message),
    Empty(empty::Message),
    Passive(passive::Message),
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
            track: None,
            captures: captures::Usage::default(),
            settings: { let s = UserSettings::load(); apply_theme(s.theme); s },
            ai_key: String::new(),
            dev_stats: std::env::var_os("SCRIPTFX_DEV_STATS").is_some(),
            entry: None,
            meta_menu: None,
            dragging: false,
            view: notch::View::Surface,
            page: None,
            motion: None,
            dock: dock::Dock::new(),
            matrix: matrix::Matrix::default(),
            source_drag: None,
            dropped: None,
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
            // ← → walk the chain while a block's modal is open (not while a text field has them).
            if self.page.is_some() {
                iced::event::listen_with(|event, status, _| {
                    use iced::keyboard::{Event::KeyPressed, Key::Named, key::Named::*};
                    match (event, status) {
                        (iced::Event::Keyboard(KeyPressed { key: Named(ArrowLeft), .. }), iced::event::Status::Ignored) => {
                            Some(Message::Page(block_modal::Message::Walk(false)))
                        }
                        (iced::Event::Keyboard(KeyPressed { key: Named(ArrowRight), .. }), iced::event::Status::Ignored) => {
                            Some(Message::Page(block_modal::Message::Walk(true)))
                        }
                        _ => None,
                    }
                })
            } else {
                Subscription::none()
            },
            // Only while a source is dragged: every pointer move re-renders the chip.
            if self.source_drag.is_some() {
                iced::event::listen_with(|event, _, _| match event {
                    iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => Some(Message::Pointer(position)),
                    _ => None,
                })
            } else {
                Subscription::none()
            },
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
                // The host names the track whenever it changes (and once after opening).
                if let Some(info) = self.ctx.new_track_info() {
                    let name = info.name().trim();
                    self.track = (!name.is_empty()).then(|| name.to_owned());
                }
                if !self.restored {
                    self.restored = true;
                    // Dev aid: SCRIPTFX_DEV_PASSIVE=1 opens in passive mode (off-screen checks).
                    self.passive = *params.passive.read().unwrap() || std::env::var_os("SCRIPTFX_DEV_PASSIVE").is_some();
                    if !self.passive {
                        self.session.restore(ctx, &params, &shared);
                    }
                    self.reveal_current();
                    // A fresh instance has no patch: show the library to pick one. (Not in
                    // passive mode: there is nothing to pick.)
                    if self.session.current.is_none() && !self.passive {
                        self.set_library(true);
                    }
                    // Dev aid: SCRIPTFX_DEV_SHEET=chat|controls|settings|library|editor|entry opens it at start
                    // (editor: LFO 1's, in the dock).
                    match std::env::var("SCRIPTFX_DEV_SHEET").as_deref() {
                        Ok("chat") => self.open_sheet(Some(Sheet::Chat)),
                        Ok("controls") => self.open_sheet(Some(Sheet::Controls)),
                        Ok("settings") => self.open_sheet(Some(Sheet::Settings)),
                        Ok("library") => self.set_library(true),
                        Ok("editor") => {
                            let lfo1 = patch::RouteSource::Modulator(patch::ModulatorId::Lfo1);
                            self.dock.select(lfo1, Instant::now());
                            self.dock.select(lfo1, Instant::now());
                        }
                        // The value entry of the first macro, as if right-clicked at Neon's Tape knob.
                        Ok("entry") => self.open_entry(stage::Target::Macro(0), iced::Point::new(165.0, 388.0)),
                        _ => {}
                    }
                    // Dev aid: SCRIPTFX_DEV_VIEW=chain|matrix starts in that view.
                    self.view = match std::env::var("SCRIPTFX_DEV_VIEW").as_deref() {
                        Ok("chain") => notch::View::Chain,
                        Ok("matrix") => notch::View::Matrix,
                        _ => self.view,
                    };
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
                self.dock.tick(&self.session.values.modulators, &self.session.values.modulator_phases);
                self.modal_tick();
                let patch_name = self.session.current.as_ref().map(|c| c.folder.clone());
                if let Some(result) = self.listen.poll(&shared, self.track.as_deref(), patch_name.as_deref()) {
                    shared.assistant.lock().unwrap().capture_done(result);
                    self.open_sheet(Some(Sheet::Chat));
                }
                let done = shared.assistant.lock().unwrap().poll();
                if let Some(done) = done {
                    self.turn_done(done, &params, &shared);
                }
                self.chat.sync(&shared.assistant.lock().unwrap());
                if self.sheet == Some(Sheet::Chat) && self.chat.take_follow() {
                    self.tasks.push(iced::widget::operation::snap_to_end(chat::Chat::log_id()));
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
                notch::Message::View(v) => self.set_view(v),
                notch::Message::Bypass(on) => {
                    let setter = self.ctx.nice_context.param_setter();
                    setter.begin_set_parameter(&params.bypass);
                    setter.set_parameter(&params.bypass, on);
                    setter.end_set_parameter(&params.bypass);
                }
            },
            Message::Matrix(msg) => self.matrix.update(&mut self.session, &params, &shared, msg),
            Message::Dock(msg) => {
                self.now = Instant::now();
                let mods = self.session.current.as_ref().map_or(&[][..], |c| &c.patch.modulators[..]);
                match self.dock.update(msg, self.now, mods) {
                    Some(dock::Event::Macro(slot, e)) => self.macro_knob(slot, e, &params, &shared),
                    Some(dock::Event::Grab(source, at)) => self.start_source_drag(source, at),
                    Some(dock::Event::Set(spec)) => self.session.set_modulator(&params, spec),
                    Some(dock::Event::Dragging(on)) => self.dragging = on,
                    None => {}
                }
            }
            Message::Pointer(p) => {
                if let Some(d) = &mut self.source_drag {
                    d.at = Some(p);
                }
            }
            Message::Surface(surface::Message::Macro(slot, e)) => self.macro_knob(slot, e, &params, &shared),
            Message::Chat(msg) => match self.chat.update(msg) {
                Some(chat::Event::Close) => self.open_sheet(None),
                Some(chat::Event::Send(text)) => {
                    // Also when it fails: the reason is the newest entry.
                    self.chat.to_end();
                    if self.send(text, false, &params, &shared) {
                        self.chat.sent();
                    }
                }
                Some(chat::Event::Cancel) => shared.assistant.lock().unwrap().cancel(),
                Some(chat::Event::Undo(i)) => self.undo_change(i, &params, &shared),
                Some(chat::Event::Detach) => shared.assistant.lock().unwrap().attached = None,
                Some(chat::Event::Listen) => self.listen.start(&shared),
                Some(chat::Event::StopListen) => self.listen.stop(&shared),
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
                settings::Message::SetProvider(p) => {
                    self.settings.ai.provider = p;
                    self.settings.save();
                    self.ai_key.clear();
                    shared.assistant.lock().unwrap().probe(&self.settings.ai);
                }
                settings::Message::Ai(field, value) => {
                    let ai = &mut self.settings.ai;
                    match field {
                        settings::Field::Model if ai.provider == user_settings::Provider::Claude => ai.claude_model = value,
                        settings::Field::Model => ai.api_model = value,
                        settings::Field::BaseUrl => {
                            ai.base_url = value;
                            shared.assistant.lock().unwrap().forget_connection();
                        }
                        settings::Field::ClaudeDir => ai.claude_config_dir = value,
                        settings::Field::Key => {
                            self.ai_key = value;
                            return;
                        }
                    }
                    self.settings.save();
                }
                settings::Message::Probe => shared.assistant.lock().unwrap().probe(&self.settings.ai),
                settings::Message::SaveKey => {
                    let key = std::mem::take(&mut self.ai_key);
                    shared.assistant.lock().unwrap().save_key(&self.settings.ai, key);
                }
                settings::Message::TogglePassive => self.set_passive(!self.passive, ctx, &params, &shared),
                settings::Message::ClearCaptures => {
                    match captures::clear() {
                        Ok(freed) => nice_plug::nice_log!("cleared {}", freed.label()),
                        Err(e) => nice_plug::nice_log!("clearing captures failed: {e}"),
                    }
                    self.captures = captures::usage();
                }
            },
            Message::Passive(msg) => match msg {
                passive::Message::Leave => self.set_passive(false, ctx, &params, &shared),
            },
            Message::Empty(msg) => match self.empty.update(msg) {
                Some(empty::Event::Build(text)) => {
                    if self.send(text, true, &params, &shared) {
                        self.empty = empty::Empty::default();
                    }
                    self.open_sheet(Some(Sheet::Chat));
                }
                Some(empty::Event::Listen) => {
                    self.listen.start(&shared);
                    self.open_sheet(Some(Sheet::Chat));
                }
                None => {}
            },
            Message::Dismiss => self.dismiss(),
            Message::Released => {
                self.dragging = false;
                if let Some(d) = self.source_drag.take() {
                    if d.moved() {
                        self.dropped = Some((d, Instant::now()));
                    } else {
                        // Pressed and released in place: a click on the source.
                        self.now = Instant::now();
                        self.dock.select(d.source, self.now);
                    }
                }
            }
            Message::Panel(ev) => {
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
            Message::Chain(chain_view::Event::Open(id)) => {
                if self.page.is_none() {
                    self.dismiss();
                    self.now = Instant::now();
                    self.page = Some(id);
                    self.motion = Some(motion::Motion::new(motion::Kind::Open, self.now));
                }
            }
            Message::Page(block_modal::Message::Close) => self.close_modal(),
            Message::Page(block_modal::Message::Walk(forward)) => self.walk(forward),
            Message::Page(block_modal::Message::Knob(i, e)) => self.param_knob(i, e, &params, &shared),
            Message::Entry(entry::Message::Edit(text)) => {
                if let Some(e) = &mut self.entry {
                    e.text = text;
                    e.invalid = false;
                }
            }
            Message::Entry(entry::Message::Submit) => self.submit_entry(&params, &shared),
            Message::MetaMenu(meta_menu::Message::Set(settings)) => {
                if let Some(m) = &self.meta_menu {
                    self.session.set_route(&params, &shared, m.route, settings);
                }
            }
            Message::MetaMenu(meta_menu::Message::Remove) => {
                if let Some(m) = self.meta_menu.take() {
                    self.session.remove_route(&params, m.route);
                }
            }
            Message::MetaMenu(meta_menu::Message::OpenMatrix) => self.set_view(notch::View::Matrix),
            Message::Escape => {
                if self.entry.take().is_none() && self.meta_menu.take().is_none() {
                    // Closes what floats first, then the dock's editor and focus, then a block's modal.
                    let open = self.library_open || self.sheet.is_some();
                    self.set_library(false);
                    self.open_sheet(None);
                    if !open && !self.dock.escape(Instant::now()) {
                        self.close_modal();
                    }
                }
            }
            Message::DismissError => self.session.error = None,
        }
    }

    /// Sends a chat message with the loaded patch (`fresh`: as a new chat). False: nothing was sent.
    fn send(&mut self, text: String, fresh: bool, params: &FxParams, shared: &Shared) -> bool {
        let current = self.session.describe(params);
        shared.assistant.lock().unwrap().send(&self.settings.ai, text, current, fresh)
    }

    /// The assistant answered: post it, apply its change.
    fn turn_done(&mut self, done: Result<llm::Outcome, llm::Error>, params: &FxParams, shared: &Shared) {
        use crate::assistant::{Change, Item};
        let outcome = match done {
            Ok(o) => o,
            Err(e) => {
                let text = match e {
                    llm::Error::Cancelled => "Cancelled.".to_string(),
                    llm::Error::Failed(text) => text,
                };
                shared.assistant.lock().unwrap().log.push(Item::Error(text));
                return;
            }
        };
        let mut items = vec![Item::Reply(outcome.reply)];
        if let Some(p) = outcome.proposal {
            let ctx = &self.ctx.nice_context.clone();
            let root = crate::patches_root();
            let previous = self.session.current.as_ref().map(|c| c.folder.clone());
            match self.session.change_files(ctx, params, shared, &p.folder, || p.commit(&root)) {
                Ok(applied) => {
                    if applied.is_new {
                        // A new patch from the chat ends passive mode (it would load behind the card).
                        if self.passive {
                            self.passive = false;
                            *params.passive.write().unwrap() = false;
                        }
                        self.session.load(ctx, params, shared, &p.folder, true);
                        self.reveal_current();
                        self.set_library(false);
                    }
                    self.forget_targets();
                    items.push(Item::Change(Change { applied, previous, undone: false }));
                }
                Err(e) => items.push(Item::Error(format!("Couldn't write the patch: {e}"))),
            }
        }
        shared.assistant.lock().unwrap().log.extend(items);
    }

    fn undo_change(&mut self, index: usize, params: &FxParams, shared: &Shared) {
        use crate::assistant::Item;
        let (applied, previous) = match shared.assistant.lock().unwrap().log.get(index) {
            Some(Item::Change(c)) if !c.undone => (c.applied.clone(), c.previous.clone()),
            _ => return,
        };
        let ctx = &self.ctx.nice_context.clone();
        let root = crate::patches_root();
        let loaded = self.session.current.as_ref().is_some_and(|c| c.folder == applied.folder);
        let result = if applied.is_new {
            applied.undo(&root).map(|()| {
                if loaded {
                    self.session.unload(ctx, params, shared);
                    if let Some(folder) = previous {
                        self.session.load(ctx, params, shared, &folder, true);
                    }
                }
                self.session.reload(ctx, params, shared);
            })
        } else {
            self.session.change_files(ctx, params, shared, &applied.folder, || applied.undo(&root))
        };
        self.forget_targets();
        let mut a = shared.assistant.lock().unwrap();
        match result {
            Ok(()) => {
                if let Some(Item::Change(c)) = a.log.get_mut(index) {
                    c.undone = true;
                }
            }
            Err(e) => a.log.push(Item::Error(format!("Couldn't undo: {e}"))),
        }
    }

    /// The patch changed under the UI: close what points at its params, routes and blocks.
    fn forget_targets(&mut self) {
        self.entry = None;
        self.meta_menu = None;
        let gone = self.page.as_ref().is_some_and(|id| {
            self.session.current.as_ref().is_none_or(|c| !c.patch.params.iter().any(|p| p.id.starts_with(&format!("{id}."))))
        });
        if gone {
            self.page = None;
            self.motion = None;
            iced_wgpu::postfx::set(None);
        }
    }

    /// Both the notch and the meta menu use the same view transition.
    fn set_view(&mut self, view: notch::View) {
        self.view = view;
        self.meta_menu = None;
        self.entry = None;
        self.page = None;
        self.motion = None;
        iced_wgpu::postfx::set(None);
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

    /// Input on a macro's knob (the dock, the macro surface).
    fn macro_knob(&mut self, slot: usize, e: knob::KnobEvent, params: &FxParams, shared: &Shared) {
        use stage::Change::*;
        match e {
            knob::KnobEvent::Begin => {
                self.dragging = true;
                self.apply(BeginMacro(slot), params, shared)
            }
            knob::KnobEvent::Set(v) => self.apply(SetMacro(slot, v), params, shared),
            knob::KnobEvent::End => {
                self.dragging = false;
                self.apply(EndMacro(slot), params, shared)
            }
            knob::KnobEvent::Step(v) => {
                for c in [BeginMacro(slot), SetMacro(slot, v), EndMacro(slot)] {
                    self.apply(c, params, shared);
                }
            }
            knob::KnobEvent::Entry(at) => self.open_entry(stage::Target::Macro(slot), window_to_panel(at)),
            knob::KnobEvent::Grab(at) => self.start_source_drag(patch::RouteSource::Macro(slot), at),
            // A modulator dropped on the macro: it modulates the macro (and so all it drives).
            knob::KnobEvent::Drop => {
                let Some(source) = self.take_drop() else { return };
                let Some(id) = self.session.current.as_ref().and_then(|c| c.patch.macros.get(slot)).map(|m| m.id.clone()) else { return };
                self.session.add_route(params, source, patch::ModDestination::Macro(id));
            }
            // The k-th meta knob is the macro's k-th route (`routing::macro_metas`).
            knob::KnobEvent::Meta(k, ev) => {
                let Some(p) = self.session.current.as_ref().map(|c| &c.patch) else { return };
                let Some(&route) = routing::macro_metas(p, slot).0.get(k) else { return };
                self.meta_event(route, ev, params, shared);
            }
            knob::KnobEvent::Mode(_) => {}
        }
    }

    /// Input on a block page's knob (param index). Free params only: the page shows the ones a
    /// macro drives read-only.
    fn param_knob(&mut self, i: usize, e: knob::KnobEvent, params: &FxParams, shared: &Shared) {
        let Some(spec) = self.session.current.as_ref().and_then(|c| c.patch.params.get(i)).cloned() else { return };
        match e {
            knob::KnobEvent::Set(v) | knob::KnobEvent::Step(v) => {
                let plain = spec.plain(v);
                self.apply(stage::Change::SetFree(i, plain), params, shared);
            }
            // Free params aren't host parameters: no automation gesture to open or close.
            knob::KnobEvent::Begin => self.dragging = true,
            knob::KnobEvent::End => self.dragging = false,
            knob::KnobEvent::Entry(at) => self.open_entry(stage::Target::Param(i), window_to_panel(at)),
            knob::KnobEvent::Meta(k, ev) => {
                // The knob's k-th meta knob is its k-th route (`routing::metas`).
                let Some(p) = self.session.current.as_ref().map(|c| &c.patch) else { return };
                let Some(&route) = routing::metas(p, i).0.get(k) else { return };
                self.meta_event(route, ev, params, shared);
            }
            knob::KnobEvent::Drop => {
                let Some(source) = self.take_drop() else { return };
                self.session.add_route(params, source, patch::ModDestination::Param(spec.id.clone()));
            }
            // Block knobs have no grip, and no modes yet.
            knob::KnobEvent::Grab(_) | knob::KnobEvent::Mode(_) => {}
        }
    }

    /// Input on a route's meta knob (on a block knob or a macro).
    fn meta_event(&mut self, route: patch::Route, ev: knob::MetaEvent, params: &FxParams, shared: &Shared) {
        let Some(p) = self.session.current.as_ref().map(|c| &c.patch) else { return };
        let (Some(current), Some(reset)) = (p.route_settings(route), p.route_reset(route)) else { return };
        match ev {
            knob::MetaEvent::Begin => self.dragging = true,
            knob::MetaEvent::End => self.dragging = false,
            knob::MetaEvent::Amount(a) => self.session.set_route(params, shared, route, patch::ModSettings { amount: a, ..current }),
            knob::MetaEvent::Reset => self.session.set_route(params, shared, route, reset),
            knob::MetaEvent::Menu(at) => self.meta_menu = Some(meta_menu::MetaMenu { route, at }),
        }
    }

    // ---- the block modal ----------------------------------------------------------------------

    /// Back to the chain: from where an opening is now, or from the open modal.
    fn close_modal(&mut self) {
        if self.page.is_none() {
            return;
        }
        self.now = Instant::now();
        self.motion = match &self.motion {
            Some(m) if m.kind == motion::Kind::Close => return,
            Some(m) if m.kind == motion::Kind::Open => Some(motion::Motion::close_from(m.openness(self.now), self.now)),
            _ => Some(motion::Motion::new(motion::Kind::Close, self.now)),
        };
    }

    /// ‹ › and ← →: the previous / next block along the chain (into containers, depth-first).
    fn walk(&mut self, forward: bool) {
        let (Some(id), None) = (&self.page, &self.motion) else { return };
        let Some(cur) = &self.session.current else { return };
        let Some(next) = neighbour(&cur.patch.chain, id, forward) else { return };
        self.now = Instant::now();
        self.motion = Some(motion::Motion::new(motion::Kind::Pan { from: id.clone(), forward }, self.now));
        self.page = Some(next);
    }

    /// Every frame: ends a finished transition, drops a modal whose block is gone (reload), and
    /// sets the renderer's zoom blur while the camera moves (none at rest).
    fn modal_tick(&mut self) {
        if let Some(m) = &self.motion
            && m.done(self.now)
        {
            if m.kind == motion::Kind::Close {
                self.page = None;
            }
            self.motion = None;
        }
        let gone = match (&self.page, &self.session.current) {
            (Some(id), Some(cur)) => find_block(&cur.patch.chain, id).is_none(),
            (Some(_), None) => true,
            _ => false,
        };
        if gone || self.view != notch::View::Chain {
            self.page = None;
            self.motion = None;
        }
        iced_wgpu::postfx::set(self.modal_look().map(|look| self.blur(&look)));
    }

    /// Where the modal sits: centred in the room the notch and a modulator's editor leave (it
    /// moves up as the editor rises).
    fn modal_target(&self) -> iced::Rectangle {
        let (w, h) = theme::stage_size();
        let size = block_modal::SIZE;
        let top = notch::size().1 + theme::SPACE_2;
        let bottom = h - self.editor_height() - theme::SPACE_2;
        let y = ((top + bottom - size.height) / 2.0).max(top);
        iced::Rectangle::new(iced::Point::new((w - size.width) / 2.0, y), size)
    }

    /// Block `id` in the chain (as last drawn); a small box in the middle until it has been.
    fn node_rect(&self, id: &str) -> iced::Rectangle {
        chain_view::rect(id).unwrap_or_else(|| {
            let c = self.modal_target().center();
            iced::Rectangle::new(iced::Point::new(c.x - 40.0, c.y - 15.0), iced::Size::new(80.0, 30.0))
        })
    }

    /// The modal at this frame; `None` without one.
    fn modal_look(&self) -> Option<Look> {
        let id = self.page.as_deref()?;
        let cur = self.session.current.as_ref()?;
        let block = find_block(&cur.patch.chain, id)?;
        let target = self.modal_target();
        let node = self.node_rect(id);
        let container = chain_view::is_container(&block.kind);
        let still = Look {
            camera: motion::open(node, target, 1.0).camera,
            rect: target,
            radius: Look::RADIUS,
            box_alpha: 1.0,
            open: 1.0,
            body: Body { id: id.to_string(), scale: 1.0, at: target.position(), alpha: 1.0, title: true },
            label: None,
            velocity: 0.0,
            grid: motion::octaves(motion::zoom_factor(node.size(), target.size())),
        };
        let Some(m) = &self.motion else { return Some(still) };
        let p = m.progress(self.now);
        let velocity = (std::f32::consts::PI * p).sin();
        match &m.kind {
            motion::Kind::Open | motion::Kind::Close => {
                let e = motion::ease(m.openness(self.now));
                let f = motion::open(node, target, e);
                // The block's label grows into the title (riding along with the box).
                let small = chain_view::NAME / block_modal::TITLE_SIZE;
                let from = f.rect.position() + chain_view::label_offset(container, small * block_modal::TITLE_H);
                let to = f.body_at + iced::Vector::new(block_modal::TITLE_AT.x, block_modal::TITLE_AT.y) * f.body_scale;
                let label = (iced::Point::new(lerp(from.x, to.x, e), lerp(from.y, to.y, e)), lerp(small, f.body_scale, e));
                Some(Look {
                    camera: f.camera,
                    rect: f.rect,
                    radius: lerp(if container { 12.0 } else { theme::RADIUS_BUTTON }, Look::RADIUS, e),
                    // A container's card shows through until the box fills in.
                    box_alpha: if container { (e / 0.4).clamp(0.0, 1.0) } else { 1.0 },
                    open: e,
                    // The content fades in once the box is opaque (a container's box fills in first).
                    body: Body { id: id.to_string(), scale: f.body_scale, at: f.body_at, alpha: ((e - if container { 0.4 } else { 0.3 }) / 0.5).clamp(0.0, 1.0), title: container },
                    // A container's card shows its own header until the box covers it: no second
                    // label flying; its title comes in with the content.
                    label: (!container).then_some(label),
                    velocity,
                    grid: motion::grid_zoom(f.camera.scale, motion::zoom_factor(node.size(), target.size())),
                })
            }
            motion::Kind::Pan { from, forward } => {
                let e = motion::ease(p);
                let from_rect = self.node_rect(from);
                let camera = motion::pan(from_rect, node, target, e);
                let octaves = |r: iced::Rectangle| motion::octaves(motion::zoom_factor(r.size(), target.size()));
                let grid = lerp(octaves(from_rect), octaves(node), e);
                // The old content slides out, the new one in.
                let dir = if *forward { 1.0 } else { -1.0 };
                let (shown, alpha, dx) = if p < 0.5 { (from.clone(), 1.0 - 2.0 * p, -dir * 28.0 * 2.0 * p) } else { (id.to_string(), 2.0 * p - 1.0, dir * 28.0 * (2.0 - 2.0 * p)) };
                Some(Look {
                    camera,
                    body: Body { id: shown, scale: 1.0, at: target.position() + iced::Vector::new(dx, 0.0), alpha, title: true },
                    // The chain has faded out: only the dots slide, nothing to blur.
                    velocity: 0.0,
                    grid,
                    ..still
                })
            }
        }
    }

    /// The renderer's blur for this frame of the modal: the chain under the camera only (it's
    /// drawn as the `backdrop`; the modal, its arrows and everything over the stage are drawn
    /// after it and stay sharp). `rect` keeps the samples inside the stage.
    fn blur(&self, look: &Look) -> iced_wgpu::postfx::RadialBlur {
        let (w, h) = theme::stage_size();
        let (x, y) = (theme::SPACE_GUTTER, theme::SIZE_TOP_BAR + theme::SPACE_GUTTER);
        let c = look.rect.center();
        iced_wgpu::postfx::RadialBlur {
            center: [x + c.x, y + c.y],
            rect: [x, y, w, h],
            // strongest mid-way, none at the ends
            amount: Look::BLUR * look.velocity,
            soft: 0.0,
            keep: [[0.0; 4]; iced_wgpu::postfx::KEEP],
        }
    }

    /// The modulator editor's height over the stage now (0 while closed or stepped aside).
    fn editor_height(&self) -> f32 {
        if self.source_drag.as_ref().is_some_and(routing::SourceDrag::moved) { 0.0 } else { self.dock.editor_height(self.now) }
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
            stage::Target::Param(i) => (p.params[i].name.clone(), p.params[i].display(v.base(i))),
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

    /// Passive mode: the instance only listens. Turning it on **unloads the patch** (the Lua
    /// runtime and the cached layers go with it, the signal passes through) and closes
    /// everything that floats; turning it off leaves an instance without a patch, so the
    /// library opens to pick one. Mirrored into the project (`FxParams::passive`).
    fn set_passive(&mut self, on: bool, ctx: &GuiContext, params: &FxParams, shared: &Shared) {
        if self.passive == on {
            return;
        }
        self.passive = on;
        *params.passive.write().unwrap() = on;
        if on {
            self.session.unload(ctx, params, shared);
            self.page = None;
            self.motion = None;
            self.view = notch::View::Surface;
            self.dismiss();
        } else {
            self.set_library(true);
        }
    }

    /// A press on the page (any view: surface, chain, block modal, …) closes what floats over it:
    /// the library, a sheet, the value entry, the meta menu. New floating things belong in here
    /// too. The dock stays as it is (its editor and focus close with Escape).
    fn dismiss(&mut self) {
        self.set_library(false);
        self.open_sheet(None);
        self.entry = None;
        self.meta_menu = None;
    }

    /// A source's title bar was pressed (at this window point): drag it (onto a knob).
    fn start_source_drag(&mut self, source: patch::RouteSource, start: iced::Point) {
        self.meta_menu = None;
        self.entry = None;
        self.dropped = None;
        self.source_drag = Some(routing::SourceDrag { source, start, at: None });
    }

    /// A knob reported a drop: the drag that just ended (if it was this recent), once.
    fn take_drop(&mut self) -> Option<patch::RouteSource> {
        let (d, at) = self.dropped.take()?;
        (at.elapsed() < Duration::from_millis(500)).then_some(d.source)
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
            if s == Sheet::Chat {
                self.chat.to_end();
            }
            // The settings sheet reports what the captures folder holds; walking it once per
            // open is cheap, per frame would not be.
            if s == Sheet::Settings {
                self.captures = captures::usage();
                self.plugin.shared.assistant.lock().unwrap().probe(&self.settings.ai);
            }
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

        let main: Element<'_, Message> = if self.passive {
            // Passive mode: no patch, no views, no notch — just the card (sheets still open
            // over it, so the user can reach settings and the chat).
            let card = passive::view(passive::Props {
                track: self.track.as_deref(),
                left: self.library_t() * library_width(),
                right: self.slide_t() * sheet_width(),
            })
            .map(Message::Passive);
            let mut layers = stack![container(card).padding(Padding { bottom: theme::SPACE_GUTTER, ..Padding::ZERO })].width(Fill).height(Fill);
            for sheet in [self.library_layer(), self.sheet_layer()].into_iter().flatten() {
                layers = layers.push(sheet);
            }
            container(layers).padding(gutters).into()
        } else {
            match &s.current {
            Some(_) => self.window(),
            None => {
                // The card slides aside with open sheets, like the notch.
                let empty = self.empty.view(self.library_t() * library_width(), self.slide_t() * sheet_width()).map(Message::Empty);
                let mut layers = stack![container(empty).padding(Padding { bottom: theme::SPACE_GUTTER, ..Padding::ZERO })].width(Fill).height(Fill);
                for sheet in [self.library_layer(), self.sheet_layer()].into_iter().flatten() {
                    layers = layers.push(sheet);
                }
                container(layers.push(self.error_card())).padding(gutters).into()
                }
            }
        };

        let top_bar = top_bar::view(top_bar::Props {
            library_open: self.library_open,
            controls_available: s.current.is_some(),
            controls_open: self.sheet == Some(Sheet::Controls),
            chat_open: self.sheet == Some(Sheet::Chat),
            settings_open: self.sheet == Some(Sheet::Settings),
            passive: self.passive,
        })
        .map(Message::TopBar);
        let window = container(column![top_bar, main]).style(theme::frame).width(Fill).height(Fill);
        // While a knob, fader or meta knob drags, the cursor is hidden everywhere.
        let panel_drag = s.current.as_ref().and_then(|c| c.panel.as_ref()).is_some_and(|p| p.dragging());
        let window = hide_cursor::hide_cursor(window, self.dragging || panel_drag, Message::Released);
        match self.drag_chip() {
            Some(chip) => stack![window, chip].into(),
            None => window.into(),
        }
    }

    /// The dragged source's chip, next to the pointer: a dot in its colour and its name.
    fn drag_chip(&self) -> Option<Element<'_, Message>> {
        let d = self.source_drag.as_ref().filter(|d| d.moved())?;
        let at = d.at?;
        let patch = &self.session.current.as_ref()?.patch;
        let colour = routing::colour(d.source);
        let dot = container(space::horizontal().width(8).height(8)).style(move |_: &Theme| container::Style {
            background: Some(colour.into()),
            border: Border { radius: theme::radius(4.0), ..Default::default() },
            ..Default::default()
        });
        let chip = container(row![dot, text(routing::name(patch, d.source).to_string()).size(11.5).font(theme::font::SANS_MEDIUM).color(theme::c().frame_ink)].spacing(6).align_y(Center))
            .padding(Padding { top: 4.0, bottom: 4.0, left: 8.0, right: 10.0 })
            .style(move |_: &Theme| container::Style {
                background: Some(theme::c().frame_raised.into()),
                border: Border { radius: theme::radius(theme::RADIUS_BUTTON), width: 1.0, color: theme::alpha(colour, 0.6) },
                shadow: Shadow { color: theme::shadow(0.4), offset: iced::Vector::new(0.0, 4.0), blur_radius: 12.0 },
                ..Default::default()
            });
        Some(pin(chip).x(at.x + 14.0).y(at.y + 10.0).into())
    }

    /// The stage with the shell's intrusions (the notch, the sheets), the dock below it. The
    /// stage's content is laid out at its full size and drawn scaled into the room the dock
    /// leaves (`fit`); the notch and the sheets aren't scaled.
    fn window(&self) -> Element<'_, Message> {
        let Some(cur) = &self.session.current else { return column![].into() };
        let bg = session::hex(cur.patch.theme.color("bg")).unwrap_or(theme::c().frame_raised);

        // While sheets are open the notch centres on the part of the window they leave visible,
        // gliding along with them.
        let notch = notch::view(notch::Props {
            snapshot: "Init",
            sync: if self.session.error.is_some() { notch::Sync::Error } else { notch::Sync::Ok },
            bypassed: self.plugin.params.bypass.value(),
        })
        .map(Message::Notch);
        // The view switch: three icons in the stage's top-right corner (left of a sheet).
        let switch = container(notch::view_switch(self.view).map(Message::Notch))
            .width(Fill)
            .padding(Padding { top: SWITCH_INSET, right: SWITCH_INSET + self.slide_t() * sheet_width(), ..Padding::ZERO })
            .align_x(alignment::Horizontal::Right);
        let notch = container(notch)
            .width(Fill)
            .padding(Padding { left: self.library_t() * library_width(), right: self.slide_t() * sheet_width(), ..Padding::ZERO })
            .align_x(alignment::Horizontal::Center);

        let (content, ground): (Element<'_, Message>, iced::Color) = match self.view {
            notch::View::Chain => (self.chain_content(cur), theme::c().frame_raised),
            notch::View::Matrix => (self.matrix.view(&cur.patch, &self.session.values).map(Message::Matrix), theme::c().frame_raised),
            notch::View::Surface => match &cur.panel {
                Some(panel) => (stage::view(panel, bg).map(Message::Panel), bg),
                None => {
                    let brand = self.session.library.iter().find(|p| p.folder == cur.folder).map(|p| &p.brand);
                    (surface::view(&cur.patch, brand, &self.session.values, self.source_drag.as_ref().and_then(routing::SourceDrag::macro_drop)).map(Message::Surface), bg)
                }
            },
        };
        // The stage's content, in its own coordinates: the page, a value entry, the meta menu.
        let mut stage = stack![press::on_any_press(content, Message::Dismiss)].width(Fill).height(Fill);
        if let Some(e) = &self.entry {
            // Right of the click point, clamped into the stage.
            let (s, offset) = stage::panel_fit();
            let (w, h) = theme::stage_size();
            let x = (offset.x + e.at.x * s + 14.0).min(w - entry::SIZE.0 - 8.0).max(8.0);
            let y = (offset.y + e.at.y * s - entry::SIZE.1 / 2.0).min(h - entry::SIZE.1 - 8.0).max(8.0);
            stage = stage.push(pin(e.view().map(Message::Entry)).x(x).y(y));
        }
        if let Some(layer) = self.meta_menu_layer() {
            stage = stage.push(layer);
        }
        let stage = container(stage).width(Fill).height(Fill).style(move |_: &Theme| container::Style {
            background: Some(ground.into()),
            border: Border { radius: theme::radius(theme::RADIUS_WINDOW), ..Default::default() },
            ..Default::default()
        });

        let mut layers = stack![stage, notch, switch].width(Fill).height(Fill);
        // The modulator's editor rises from the dock over the stage's lower part; it steps aside
        // while a source is dragged (so whatever is under it takes the drop).
        if self.editor_height() > 0.0 {
            if let Some(editor) = self.dock.editor_view(self.now, &cur.patch.modulators) {
                layers = layers.push(container(editor.map(Message::Dock)).width(Fill).height(Fill).align_y(alignment::Vertical::Bottom));
            }
        }
        for layer in [self.library_layer(), self.sheet_layer()].into_iter().flatten() {
            layers = layers.push(layer);
        }
        layers = layers.push(self.error_card());
        if self.dev_stats {
            layers = layers.push(self.stats());
        }

        let dock = self.dock.view(&cur.patch, &self.session.values, self.source_drag.as_ref()).map(Message::Dock);
        column![container(layers).width(Fill).height(theme::stage_size().1), dock]
            .spacing(theme::SPACE_2)
            .padding(Padding { top: theme::SPACE_GUTTER, right: theme::SPACE_GUTTER, left: theme::SPACE_GUTTER, bottom: theme::SPACE_GUTTER })
            .width(Fill)
            .height(Fill)
            .into()
    }

    /// The chain view, with a block's modal over it: the camera on the chain, a veil, the box, the
    /// modal's content (scaled to the box, fading in), the block's label growing into the title,
    /// and the arrows beside the box. See `motion` and `docs/research/mockups/block-modal.html`.
    fn chain_content<'a>(&'a self, cur: &'a session::Loaded) -> Element<'a, Message> {
        let values = &self.session.values;
        let look = self.modal_look();
        // The modal stands in for its block: the block isn't drawn under it (a container's card
        // only once the box covers it), nor the one a walk comes from.
        let page = self.page.as_deref();
        let is_card = page.and_then(|id| find_block(&cur.patch.chain, id)).is_some_and(|b| chain_view::is_container(&b.kind));
        let hidden = match (&look, &self.motion) {
            (None, _) => [None, None],
            (Some(l), m) => {
                let from = match m.as_ref().map(|m| &m.kind) {
                    Some(motion::Kind::Pan { from, .. }) => Some(from.as_str()),
                    _ => None,
                };
                [page.filter(|_| !is_card || l.box_alpha >= 1.0), from]
            }
        };
        let chain = chain_view::view(&cur.patch, values, look.is_none(), hidden, look.is_none()).map(Message::Chain);
        let Some(look) = look else { return chain };
        let target = self.modal_target();
        // The box's ground: the block's colour, solid (a container's card starts as the editor's
        // ground and turns into it).
        let ground = if is_card { mix(theme::c().frame_raised, theme::c().frame_node, (look.open / 0.5).clamp(0.0, 1.0)) } else { theme::c().frame_node };

        // The editor's ground, the chain under the camera fading out into it, and the ground's dot
        // grid zooming along (endlessly: it ends at its normal spacing). Open, the modal sits on the
        // plain editor.
        let open = look.open;
        let window = |colour: iced::Color| {
            move |_: &Theme| container::Style {
                background: Some(colour.into()),
                border: Border { radius: theme::radius(theme::RADIUS_WINDOW), ..Default::default() },
                ..Default::default()
            }
        };
        let ground_colour = theme::c().frame_raised;
        let world = zoom::zoom(chain, look.camera.scale, look.camera.origin).offset(look.camera.offset);
        let fade = ((open - 0.25) / 0.6).clamp(0.0, 1.0);
        let cam = look.camera;
        let d = 2f32.powf(look.grid);
        let first = iced::Point::new(chain_view::DOT_STEP / 2.0, chain_view::DOT_STEP / 2.0);
        let anchor = iced::Point::new(
            cam.origin.x + cam.offset.x + (first.x - cam.origin.x) * d,
            cam.origin.y + cam.offset.y + (first.y - cam.origin.y) * d,
        );
        // A click beside the modal closes it.
        let veil = iced::widget::mouse_area(container(chain_view::zoom_dots(anchor, look.grid)).width(Fill).height(Fill).style(window(theme::alpha(ground_colour, fade))))
            .on_press(Message::Page(block_modal::Message::Close));
        // The chain under the camera is the blur's backdrop; the box, content, label and arrows
        // are drawn over it, sharp.
        let under = stack![container(column![]).width(Fill).height(Fill).style(window(ground_colour)), world, veil, stage::corners()].width(Fill).height(Fill);
        let mut layers = stack![backdrop::backdrop(under)].width(Fill).height(Fill);

        // The box: the block, grown. `Idle`: clicks on it don't fall through to the veil.
        let (r, a, radius) = (look.rect, look.box_alpha, look.radius);
        let card = container(column![]).width(r.width).height(r.height).style(move |_: &Theme| container::Style {
            background: Some(theme::alpha(ground, a).into()),
            border: Border { radius: theme::radius(radius), width: 1.0, color: theme::alpha(theme::c().frame_hairline_strong, theme::c().frame_hairline_strong.a * a) },
            shadow: Shadow { color: theme::shadow(0.5 * open), offset: iced::Vector::new(0.0, 24.0 * open), blur_radius: 60.0 * open },
            ..Default::default()
        });
        layers = layers.push(pin(iced::widget::mouse_area(card).interaction(iced::mouse::Interaction::Idle)).x(r.x).y(r.y));

        // The content, laid out at the modal's place and drawn scaled into the box.
        if let Some(block) = find_block(&cur.patch.chain, &look.body.id).filter(|_| look.body.alpha > 0.0) {
            let body = block_modal::view(&cur.patch, values, block, self.source_drag.as_ref(), look.body.title).map(Message::Page);
            let offset = look.body.at - target.position();
            let body = container(pin(body).x(target.x).y(target.y)).width(Fill).height(Fill);
            layers = layers.push(zoom::zoom(body, look.body.scale, target.position()).offset(offset));
            // It fades in: a veil in the box's colour over it.
            if look.body.alpha < 1.0 {
                let veil = 1.0 - look.body.alpha;
                let cover = container(column![]).width(r.width).height(r.height).style(move |_: &Theme| container::Style {
                    background: Some(theme::alpha(ground, veil).into()),
                    border: Border { radius: theme::radius(radius), ..Default::default() },
                    ..Default::default()
                });
                layers = layers.push(pin(cover).x(r.x).y(r.y));
            }
        }
        // The block's label, growing into the title.
        if let (Some((at, scale)), Some(block)) = (look.label, self.page.as_deref().and_then(|id| find_block(&cur.patch.chain, id))) {
            let home = target.position() + iced::Vector::new(block_modal::TITLE_AT.x, block_modal::TITLE_AT.y);
            let title = container(pin(block_modal::title(&cur.patch, values, block, 1.0)).x(home.x).y(home.y)).width(Fill).height(Fill);
            layers = layers.push(zoom::zoom(title, scale, home).offset(at - home));
        }
        // ‹ › beside the box: floating chevrons, faint where the chain ends.
        let shown = ((look.open - 0.6) / 0.4).clamp(0.0, 1.0);
        if shown > 0.0 {
            let id = self.page.as_deref().unwrap_or_default();
            let (left, right) = look.arrows();
            for (forward, at) in [(false, left), (true, right)] {
                let live = neighbour(&cur.patch.chain, id, forward).is_some();
                let colour = theme::alpha(theme::c().frame_muted, shown * if live { 1.0 } else { 0.25 });
                let icon = icons::icon_rotated(icons::CHEVRON_RIGHT, 22.0, 2.0, colour, if forward { 0.0 } else { 180.0 });
                let cursor = if live { iced::mouse::Interaction::Pointer } else { iced::mouse::Interaction::Idle };
                let mut arrow = iced::widget::mouse_area(container(icon).center(Look::ARROW)).interaction(cursor);
                if live && look.open >= 1.0 && self.motion.is_none() {
                    arrow = arrow.on_press(Message::Page(block_modal::Message::Walk(forward)));
                }
                layers = layers.push(pin(arrow).x(at.x).y(at.y));
            }
        }
        layers.into()
    }

    /// 0 = right sheet hidden, 1 = fully in.
    fn slide_t(&self) -> f32 {
        self.slide.interpolate(0.0, 1.0, self.now)
    }

    /// 0 = library hidden, 1 = fully in.
    fn library_t(&self) -> f32 {
        self.library_slide.interpolate(0.0, 1.0, self.now)
    }

    /// The meta knob's menu, right of the click, clamped into the stage.
    fn meta_menu_layer(&self) -> Option<Element<'_, Message>> {
        let menu = self.meta_menu.as_ref()?;
        let p = &self.session.current.as_ref()?.patch;
        let title = routing::title(p, menu.route)?;
        let (settings, reset) = (p.route_settings(menu.route)?, p.route_reset(menu.route)?);
        let (w, h) = theme::stage_size();
        let at = menu.at - iced::Vector::new(theme::SPACE_GUTTER, theme::SIZE_TOP_BAR + theme::SPACE_GUTTER);
        // The click is on a meta knob in the cell's top-left: clear the cell (64 px) to its right.
        let x = (at.x + 64.0).min(w - meta_menu::WIDTH - 8.0).max(8.0);
        let y = (at.y - 12.0).min(h - meta_menu::HEIGHT - 8.0).max(8.0);
        Some(pin(menu.view(title, settings, reset).map(Message::MetaMenu)).x(x).y(y).into())
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
                ai: &self.settings.ai,
                ai_key: &self.ai_key,
                ai_connection: self.plugin.shared.assistant.lock().unwrap().connection(),
                theme: self.settings.theme,
                ui_scale: self.settings.ui_scale,
                passive: self.passive,
                passive_available: true,
                captures: self.captures,
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

/// One frame of the block modal: where the camera is, the box, the content, the label.
struct Look {
    camera: motion::Camera,
    rect: iced::Rectangle,
    radius: f32,
    /// The box's opacity (a container's card shows through at first).
    box_alpha: f32,
    /// 0 = the chain, 1 = the modal open (eased): the chain fading out, the shadow.
    open: f32,
    body: Body,
    /// The block's label growing into the title: where, how big (`None` = the title draws itself).
    label: Option<(iced::Point, f32)>,
    /// How fast the camera moves (0..1): the zoom blur.
    velocity: f32,
    /// The ground's dot grid: how many octaves it has zoomed (see `chain_view::zoom_dots`).
    grid: f32,
}

/// The modal's content in a frame (a walk shows the previous block's while it slides out).
struct Body {
    id: String,
    scale: f32,
    at: iced::Point,
    alpha: f32,
    title: bool,
}

impl Look {
    /// The zoom blur's strength mid-way: how far a pixel smears towards the centre.
    const BLUR: f32 = 0.22;
    const RADIUS: f32 = 16.0;
    /// The arrows' hit area, and their gap to the box.
    const ARROW: f32 = 36.0;
    const ARROW_GAP: f32 = 14.0;

    /// Where ‹ and › sit: beside the box, centred on it.
    fn arrows(&self) -> (iced::Rectangle, iced::Rectangle) {
        let r = self.rect;
        let y = r.center_y() - Self::ARROW / 2.0;
        let size = iced::Size::new(Self::ARROW, Self::ARROW);
        (
            iced::Rectangle::new(iced::Point::new(r.x - Self::ARROW_GAP - Self::ARROW, y), size),
            iced::Rectangle::new(iced::Point::new(r.x + r.width + Self::ARROW_GAP, y), size),
        )
    }
}

fn mix(a: iced::Color, b: iced::Color, t: f32) -> iced::Color {
    iced::Color { r: lerp(a.r, b.r, t), g: lerp(a.g, b.g, t), b: lerp(a.b, b.b, t), a: lerp(a.a, b.a, t) }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// The block before / after `id` along the chain (depth-first: a container, then its blocks).
fn neighbour(chain: &[patch::BlockSpec], id: &str, forward: bool) -> Option<String> {
    let mut order = Vec::new();
    patch::chain::walk(chain, &mut |b| order.push(b.id.as_str()));
    let i = order.iter().position(|b| *b == id)?;
    let j = if forward { i + 1 } else { i.checked_sub(1)? };
    order.get(j).map(|s| s.to_string())
}

/// A block anywhere in the chain (inside containers too), by id.
fn find_block<'a>(chain: &'a [patch::BlockSpec], id: &str) -> Option<&'a patch::BlockSpec> {
    let mut found = None;
    patch::chain::walk(chain, &mut |b| {
        if found.is_none() && b.id == id {
            found = Some(b);
        }
    });
    found
}

/// A window point → panel coordinates (where the value entry is placed): minus the patch
/// window's origin, minus the panel's offset, unscaled.
fn window_to_panel(at: iced::Point) -> iced::Point {
    let (s, offset) = stage::panel_fit();
    let p = at - iced::Vector::new(theme::SPACE_GUTTER, theme::SIZE_TOP_BAR + theme::SPACE_GUTTER);
    iced::Point::new((p.x - offset.x) / s, (p.y - offset.y) / s)
}

/// The view switch's distance from the stage's top-right corner.
const SWITCH_INSET: f32 = 10.0;

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
