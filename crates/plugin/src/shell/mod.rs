//! The shell: the plugin's own UI (iced), in the "ScriptFX Shell" design system.
//! Top bar over the stage (the patch window: ControlPanel, chain or Matrix, the notch hanging
//! into it, sheets floating over it on both sides), the mod dock below it.
//!
//! One module per section, each with its own state, `Message`, `update` and `view`:
//! - `top_bar`      the wordmark, sheet openers (library left; controls, chat, settings right)
//! - `library`      the patch library (sheet, left)
//! - `notch`        view switch (Panel | Chain | Matrix); reload / snapshot selector, save, load
//! - `snapshot_menu` the notch's snapshot menu (recall, save as, rename, revert, export, delete)
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
mod chain_surface;
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
mod snapshot_menu;
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
mod viewport;
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
    /// The snapshot menu (the notch's selector).
    snapshot_menu: Option<snapshot_menu::Menu>,
    /// Snapshot import/export dialogs.
    dialogs: crate::dialog::Dialogs,
    /// What the patch window shows (the notch's view switch).
    surface: viewport::SurfaceId,
    /// The chain surface: the block tree, and the modal it zooms into. Owns its own state.
    chain: chain_surface::ChainSurface,
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
    SnapshotMenu(snapshot_menu::Message),
    /// A press anywhere on the page (surface, chain, block page, …): close what floats.
    Dismiss,
    /// The pointer moved while a source is dragged (window coordinates).
    Pointer(iced::Point),
    /// The left button went up somewhere: no drag runs any more.
    Released,
    Surface(surface::Message),
    Notch(notch::Message),
    Matrix(matrix::Message),
    Chain(chain_surface::Message),
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
            snapshot_menu: None,
            dialogs: Default::default(),
            dragging: false,
            surface: viewport::SurfaceId::DEFAULT,
            chain: chain_surface::ChainSurface::default(),
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
            if self.chain.open_block().is_some() {
                iced::event::listen_with(|event, status, _| {
                    use iced::keyboard::{Event::KeyPressed, Key::Named, key::Named::*};
                    match (event, status) {
                        (iced::Event::Keyboard(KeyPressed { key: Named(ArrowLeft), .. }), iced::event::Status::Ignored) => {
                            Some(Message::Chain(chain_surface::Message::Walk(false)))
                        }
                        (iced::Event::Keyboard(KeyPressed { key: Named(ArrowRight), .. }), iced::event::Status::Ignored) => {
                            Some(Message::Chain(chain_surface::Message::Walk(true)))
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
                    // Dev aid: SCRIPTFX_DEV_SHEET=chat|controls|settings|library|editor|entry|snapshots|save
                    // opens it at start (editor: LFO 1's, in the dock; save: the snapshot menu naming).
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
                        Ok("snapshots") => self.snapshot_menu = Some(snapshot_menu::Menu::list()),
                        Ok("save") => self.open_naming(snapshot_menu::Purpose::SaveAs, "Warm bus".into()),
                        _ => {}
                    }
                    // Dev aid: SCRIPTFX_DEV_SURFACE=chain|matrix starts on that surface.
                    if let Some(id) = std::env::var("SCRIPTFX_DEV_SURFACE").ok().and_then(|k| viewport::SurfaceId::from_key(&k)) {
                        self.surface = id;
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
                if let Some(picked) = self.dialogs.poll() {
                    self.dialog_done(picked, ctx, &params, &shared);
                }
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
                Some(library::Event::Load(folder, snapshot)) => {
                    self.snapshot_menu = None;
                    let loaded = self.session.current.as_ref().is_some_and(|c| c.folder == folder);
                    if !loaded || snapshot == crate::snapshots::INIT {
                        self.session.load(ctx, &params, &shared, &folder, true);
                    }
                    if snapshot != crate::snapshots::INIT && self.session.current.as_ref().is_some_and(|c| c.folder == folder) {
                        if let Err(e) = self.session.recall(ctx, &params, &shared, &snapshot) {
                            self.snapshot_error(e);
                        }
                    }
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
                notch::Message::Surface(id) => self.set_surface(id),
                notch::Message::Bypass(on) => {
                    let setter = self.ctx.nice_context.param_setter();
                    setter.begin_set_parameter(&params.bypass);
                    setter.set_parameter(&params.bypass, on);
                    setter.end_set_parameter(&params.bypass);
                }
                notch::Message::Step(forward) => {
                    self.snapshot_menu = None;
                    let cycle = self.session.snapshot_cycle();
                    let at = cycle.iter().position(|n| *n == self.session.snapshot(&params)).unwrap_or(0);
                    let next = if forward { (at + 1) % cycle.len() } else { (at + cycle.len() - 1) % cycle.len() };
                    if let Err(e) = self.session.recall(ctx, &params, &shared, &cycle[next]) {
                        self.snapshot_error(e);
                    }
                }
                notch::Message::Menu => {
                    if self.snapshot_menu.take().is_none() {
                        self.entry = None;
                        self.meta_menu = None;
                        self.snapshot_menu = Some(snapshot_menu::Menu::list());
                    }
                }
                notch::Message::Save => {
                    let name = self.session.snapshot(&params);
                    if name == crate::snapshots::INIT {
                        self.open_naming(snapshot_menu::Purpose::SaveAs, String::new());
                    } else {
                        // Saving over the selected snapshot.
                        match self.session.save_snapshot(&params, &name, true) {
                            Ok(_) => self.snapshot_menu = None,
                            Err(e) => self.snapshot_error(e),
                        }
                    }
                }
                notch::Message::Import => self.dialogs.import(),
            },
            Message::SnapshotMenu(msg) => self.snapshot_menu_message(msg, ctx, &params, &shared),
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
            Message::Chain(chain_surface::Message::Open(id)) => {
                if self.chain.open_block().is_none() {
                    self.dismiss();
                    self.now = Instant::now();
                    self.chain.open_modal(id, self.now);
                }
            }
            Message::Chain(chain_surface::Message::Close) => {
                self.now = Instant::now();
                self.chain.close_modal(self.now);
            }
            Message::Chain(chain_surface::Message::Walk(forward)) => {
                self.now = Instant::now();
                // Disjoint fields: the chain surface borrows mutably, the session only shared.
                if let Some(cur) = &self.session.current {
                    self.chain.walk(forward, &cur.patch.chain, self.now);
                }
            }
            Message::Chain(chain_surface::Message::Knob(i, e)) => self.param_knob(i, e, &params, &shared),
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
            Message::MetaMenu(meta_menu::Message::OpenMatrix) => self.set_surface(viewport::SurfaceId::Matrix),
            Message::Escape => {
                // The snapshot menu steps back from naming/confirming first, then closes.
                if let Some(m) = &mut self.snapshot_menu {
                    if matches!(m.mode, snapshot_menu::Mode::List) {
                        self.snapshot_menu = None;
                    } else {
                        m.mode = snapshot_menu::Mode::List;
                        m.error = None;
                    }
                } else if self.entry.take().is_none() && self.meta_menu.take().is_none() {
                    // Closes what floats first, then the dock's editor and focus, then a block's modal.
                    let open = self.library_open || self.sheet.is_some();
                    self.set_library(false);
                    self.open_sheet(None);
                    if !open && !self.dock.escape(Instant::now()) {
                        self.now = Instant::now();
                        self.chain.close_modal(self.now);
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
        if self.chain.targets_gone(self.session.current.as_ref().map(|c| &c.patch)) {
            self.chain.clear();
            iced_wgpu::postfx::set(None);
        }
    }

    /// Both the notch and the meta menu switch surfaces through here.
    fn set_surface(&mut self, id: viewport::SurfaceId) {
        self.surface = id;
        self.meta_menu = None;
        self.entry = None;
        self.chain.clear();
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

    /// What the chain surface needs from the shell to lay itself out this frame.
    fn chain_ctx<'a>(&'a self, patch: &'a patch::Patch) -> chain_surface::Ctx<'a> {
        chain_surface::Ctx {
            patch,
            values: &self.session.values,
            source_drag: self.source_drag.as_ref(),
            now: self.now,
            editor_height: self.editor_height(),
        }
    }

    /// Every frame: let the chain surface age its transition, then set the renderer's zoom blur
    /// from the modal's `Look` (none at rest). The blur stays here: it drives the renderer.
    fn modal_tick(&mut self) {
        let patch = self.session.current.as_ref().map(|c| &c.patch);
        self.chain.tick(patch, self.surface.owns_block_modal(), self.now);
        let look = self
            .session
            .current
            .as_ref()
            .and_then(|cur| self.chain.look(&self.chain_ctx(&cur.patch)));
        iced_wgpu::postfx::set(look.map(|look| self.blur(&look)));
    }

    /// The renderer's blur for this frame of the modal: the chain under the camera only (it's
    /// drawn as the `backdrop`; the modal, its arrows and everything over the stage are drawn
    /// after it and stay sharp). `rect` keeps the samples inside the stage.
    fn blur(&self, look: &chain_surface::Look) -> iced_wgpu::postfx::RadialBlur {
        let (w, h) = theme::stage_size();
        let (x, y) = (theme::SPACE_GUTTER, theme::SIZE_TOP_BAR + theme::SPACE_GUTTER);
        let c = look.rect.center();
        iced_wgpu::postfx::RadialBlur {
            center: [x + c.x, y + c.y],
            rect: [x, y, w, h],
            // strongest mid-way, none at the ends
            amount: chain_surface::Look::BLUR * look.velocity,
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
            self.chain.clear();
            self.surface = viewport::SurfaceId::DEFAULT;
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
        self.snapshot_menu = None;
    }

    // ---- snapshots ------------------------------------------------------------------------------

    fn snapshot_menu_message(&mut self, msg: snapshot_menu::Message, ctx: &GuiContext, params: &FxParams, shared: &Shared) {
        use snapshot_menu::{Message as M, Mode, Purpose};
        let current = self.session.snapshot(params);
        let Some(menu) = &mut self.snapshot_menu else { return };
        menu.error = None;
        match msg {
            M::Recall(name) => {
                self.snapshot_menu = None;
                if let Err(e) = self.session.recall(ctx, params, shared, &name) {
                    self.snapshot_error(e);
                }
            }
            M::Revert => {
                self.snapshot_menu = None;
                if let Err(e) = self.session.recall(ctx, params, shared, &current) {
                    self.snapshot_error(e);
                }
            }
            M::SaveAs => self.open_naming(Purpose::SaveAs, String::new()),
            M::Rename => self.open_naming(Purpose::Rename(current.clone()), current),
            M::Delete => menu.mode = Mode::ConfirmDelete(current),
            M::ConfirmDelete => {
                let Mode::ConfirmDelete(name) = &menu.mode else { return };
                match self.session.delete_snapshot(params, &name.clone()) {
                    Ok(()) => self.snapshot_menu = Some(snapshot_menu::Menu::list()),
                    Err(e) => self.snapshot_error(e),
                }
            }
            M::Export => {
                self.snapshot_menu = None;
                self.dialogs.export(&current);
            }
            M::Edit(t) => {
                if let Mode::Name { text, .. } = &mut menu.mode {
                    *text = t;
                }
            }
            M::Submit => {
                let Mode::Name { purpose, text } = &menu.mode else { return };
                let result = match purpose.clone() {
                    Purpose::SaveAs => self.session.save_snapshot(params, &text.clone(), false),
                    Purpose::Rename(from) => self.session.rename_snapshot(params, &from, &text.clone()),
                };
                match result {
                    Ok(_) => self.snapshot_menu = None,
                    Err(e) => {
                        if let Some(m) = &mut self.snapshot_menu {
                            m.error = Some(e);
                        }
                        self.tasks.push(iced::widget::operation::focus(snapshot_menu::input_id()));
                    }
                }
            }
            M::Back => menu.mode = Mode::List,
        }
    }

    /// The snapshot menu with a name field, focused and selected.
    fn open_naming(&mut self, purpose: snapshot_menu::Purpose, text: String) {
        self.entry = None;
        self.meta_menu = None;
        self.snapshot_menu = Some(snapshot_menu::Menu::naming(purpose, text));
        self.tasks.push(iced::widget::operation::focus(snapshot_menu::input_id()));
        self.tasks.push(iced::widget::operation::select_all(snapshot_menu::input_id()));
    }

    /// A snapshot action failed: say so in the snapshot menu (opened if needed).
    fn snapshot_error(&mut self, e: String) {
        nice_plug::nice_log!("snapshot: {e}");
        let menu = self.snapshot_menu.get_or_insert_with(snapshot_menu::Menu::list);
        menu.mode = snapshot_menu::Mode::List;
        menu.error = Some(e);
    }

    /// A file dialog answered (import or export).
    fn dialog_done(&mut self, picked: crate::dialog::Picked, ctx: &GuiContext, params: &FxParams, shared: &Shared) {
        let result = match picked {
            crate::dialog::Picked::Import(path) => self.session.import_snapshot(ctx, params, shared, &path).map(|_| ()),
            crate::dialog::Picked::Export(path) => self.session.export_snapshot(params, &path),
        };
        if let Err(e) = result {
            self.snapshot_error(e);
        }
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
        let snapshot = self.session.snapshot(&self.plugin.params);
        let notch = notch::view(notch::Props {
            snapshot: snapshot.clone(),
            modified: self.session.modified,
            can_step: !self.session.snapshot_names().is_empty(),
            menu_open: self.snapshot_menu.is_some(),
            sync: if self.session.error.is_some() { notch::Sync::Error } else { notch::Sync::Ok },
            bypassed: self.plugin.params.bypass.value(),
        })
        .map(Message::Notch);
        // The view switch: three icons in the stage's top-right corner (left of a sheet).
        let surfaces = viewport::surfaces(&cur.patch);
        let switch = container(notch::surface_switch(&surfaces, self.surface).map(Message::Notch))
            .width(Fill)
            .padding(Padding { top: SWITCH_INSET, right: SWITCH_INSET + self.slide_t() * sheet_width(), ..Padding::ZERO })
            .align_x(alignment::Horizontal::Right);
        let notch = container(notch)
            .width(Fill)
            .padding(Padding { left: self.library_t() * library_width(), right: self.slide_t() * sheet_width(), ..Padding::ZERO })
            .align_x(alignment::Horizontal::Center);

        let (content, ground): (Element<'_, Message>, iced::Color) = match self.surface {
            viewport::SurfaceId::Chain => {
                let ctx = self.chain_ctx(&cur.patch);
                let look = self.chain.look(&ctx);
                (self.chain.view(&ctx, look).map(Message::Chain), theme::c().frame_raised)
            }
            viewport::SurfaceId::Matrix => (self.matrix.view(&cur.patch, &self.session.values).map(Message::Matrix), theme::c().frame_raised),
            // A patch surface: the patch's own, or the shell's default macro surface.
            viewport::SurfaceId::Patch(_) => match &cur.panel {
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
        if let Some(menu) = self.snapshot_menu_layer(&snapshot) {
            layers = layers.push(menu);
        }
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

    /// The snapshot menu, hanging under the notch (centred like it).
    fn snapshot_menu_layer(&self, current: &str) -> Option<Element<'_, Message>> {
        let menu = self.snapshot_menu.as_ref()?;
        let cycle = self.session.snapshot_cycle();
        let card = menu.view(cycle, current, self.session.modified).map(Message::SnapshotMenu);
        Some(
            container(card)
                .width(Fill)
                .padding(Padding {
                    top: notch::size().1 + theme::SPACE_HAIR,
                    left: self.library_t() * library_width(),
                    right: self.slide_t() * sheet_width(),
                    ..Padding::ZERO
                })
                .align_x(alignment::Horizontal::Center)
                .into(),
        )
    }

    /// The library sheet, over the left side of the window.
    fn library_layer(&self) -> Option<Element<'_, Message>> {
        let t = self.library_t();
        if t <= 0.0 {
            return None;
        }
        let s = &self.session;
        let snapshot = s.snapshot(&self.plugin.params);
        let sheet = self.library.view(&s.library, s.current.as_ref().map(|c| (c.folder.as_str(), snapshot))).map(Message::Library);
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
