//! The shell: the plugin's own UI (iced), in the "ScriptFX Shell" design system.
//! Top bar over the patch window (ControlPanel + notch at the top, drawer at the bottom, sheets
//! floating over it on both sides).
//!
//! One module per section, each with its own state, `Message`, `update` and `view`:
//! - `top_bar`      the wordmark, sheet openers (library left; controls, chat, settings right)
//! - `library`      the patch library (sheet, left)
//! - `notch`        view switch (Panel | Chain | Matrix); reload / snapshot selector, save, load
//! - `drawer`       rises from the window's bottom edge: modulator tabs and the open modulator
//! - `macro_rail`   the patch's macros, a card on the left under the top bar
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
//! Drag & drop: pressing a macro's grip (rail) or a modulator tab's grip (drawer) starts a
//! `SourceDrag`; a chip follows the pointer, knobs that can take it outline themselves, and a
//! release over one adds the route (`Session::add_route`).
//!
//! `session` holds the non-UI state (library scan, loaded patch, hot reload, values).
//! This file owns what several sections share (which sheet is open, the frame clock), routes
//! messages and applies param changes. Sections report what the shell must act on as an
//! `Event` from their `update`; they never reach into each other.

pub mod theme;

mod block_display;
mod block_modal;
mod brand;
mod chain_surface;
mod chain_view;
mod chat;
mod controls;
mod drawer;
mod knob;
mod empty;
mod entry;
mod hide_cursor;
mod icons;
mod library;
mod macro_rail;
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
    /// The macro rail (left, under the top bar) and its slide.
    macros_open: bool,
    macros_slide: Animation<bool>,
    macro_rail: macro_rail::Rail,
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
    surface: viewport::SurfaceId,
    /// The chain surface: the block tree, and the modal it zooms into. Owns its own state.
    chain: chain_surface::ChainSurface,
    drawer: drawer::Drawer,
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
    Macros(macro_rail::Message),
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
    Chain(chain_surface::Message),
    Drawer(drawer::Message),
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
            macros_open: false,
            macros_slide: Animation::new(false).duration(Duration::from_millis(260)).easing(Easing::EaseOutCubic),
            macro_rail: macro_rail::Rail::new(),
            now: Instant::now(),
            dragging_macro: None,
            passive: false,
            track: None,
            captures: captures::Usage::default(),
            settings: { let s = UserSettings::load(); apply_theme(s.theme); s },
            dev_stats: std::env::var_os("SCRIPTFX_DEV_STATS").is_some(),
            entry: None,
            meta_menu: None,
            dragging: false,
            surface: viewport::SurfaceId::DEFAULT,
            chain: chain_surface::ChainSurface::default(),
            drawer: drawer::Drawer::new(),
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
                    // Dev aid: SCRIPTFX_DEV_SHEET=chat|controls|settings|library|drawer|entry opens it at start.
                    match std::env::var("SCRIPTFX_DEV_SHEET").as_deref() {
                        Ok("chat") => self.open_sheet(Some(Sheet::Chat)),
                        Ok("controls") => self.open_sheet(Some(Sheet::Controls)),
                        Ok("settings") => self.open_sheet(Some(Sheet::Settings)),
                        Ok("library") => self.set_library(true),
                        Ok("macros") => self.set_macros(true),
                        Ok("drawer") => self.drawer.set_open(true, Instant::now()),
                        // The value entry of the first macro, as if right-clicked at Neon's Tape knob.
                        Ok("entry") => self.open_entry(stage::Target::Macro(0), iced::Point::new(165.0, 388.0)),
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
                self.drawer.tick(&self.session.values.modulators, &self.session.values.modulator_phases);
                self.modal_tick();
                let patch_name = self.session.current.as_ref().map(|c| c.folder.clone());
                if let Some(result) = self.listen.poll(&shared, self.track.as_deref(), patch_name.as_deref()) {
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
                top_bar::Message::ToggleMacros => self.set_macros(!self.macros_open),
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
                notch::Message::Surface(id) => self.set_surface(id),
            },
            Message::Matrix(msg) => self.matrix.update(&mut self.session, &params, &shared, msg),
            Message::Drawer(msg) => {
                self.now = Instant::now();
                let mods = self.session.current.as_ref().map_or(&[][..], |c| &c.patch.modulators[..]);
                match self.drawer.update(msg, self.now, mods) {
                    Some(drawer::Event::Grab(id)) => self.start_source_drag(patch::RouteSource::Modulator(id)),
                    Some(drawer::Event::Set(spec)) => self.session.set_modulator(&params, spec),
                    Some(drawer::Event::Dragging(on)) => self.dragging = on,
                    None => {}
                }
            }
            Message::Pointer(p) => {
                if let Some(d) = &mut self.source_drag {
                    d.at = Some(p);
                }
            }
            Message::Surface(surface::Message::Macro(slot, e)) => self.macro_knob(slot, e, &params, &shared),
            Message::Macros(msg) => {
                self.now = Instant::now();
                match self.macro_rail.update(msg, self.now) {
                    Some(macro_rail::Event::Macro(slot, e)) => self.macro_knob(slot, e, &params, &shared),
                    Some(macro_rail::Event::Close) => self.set_macros(false),
                    None => {}
                }
            }
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
                Some(e) => match e {},
                None => {}
            },
            Message::Dismiss => self.dismiss(),
            Message::Released => {
                self.dragging = false;
                if let Some(d) = self.source_drag.take() {
                    self.dropped = Some((d, Instant::now()));
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
                if self.entry.take().is_none() && self.meta_menu.take().is_none() {
                    // Closes what floats first; with nothing open, closes a block's modal.
                    let open = self.library_open || self.macros_open || self.sheet.is_some() || self.drawer.is_open();
                    self.set_library(false);
                    self.set_macros(false);
                    self.open_sheet(None);
                    self.close_drawer();
                    if !open {
                        self.now = Instant::now();
                        self.chain.close_modal(self.now);
                    }
                }
            }
            Message::DismissError => self.session.error = None,
        }
    }

    /// Both the notch and the meta menu switch surfaces through here.
    fn set_surface(&mut self, id: viewport::SurfaceId) {
        self.surface = id;
        self.meta_menu = None;
        self.entry = None;
        self.chain.clear();
        iced_wgpu::postfx::set(None);
        if !id.wants_drawer() {
            self.close_drawer();
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

    /// Input on one of the drawer's macro knobs.
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
            knob::KnobEvent::Grab => self.start_source_drag(patch::RouteSource::Macro(slot)),
            // A modulator dropped on the macro: it modulates the macro (and so all it drives).
            knob::KnobEvent::Drop => {
                let Some(source) = self.take_drop() else { return };
                let Some(id) = self.session.current.as_ref().and_then(|c| c.patch.macros.get(slot)).map(|m| m.id.clone()) else { return };
                self.session.add_route(params, source, patch::ModDestination::Macro(id));
            }
            // Macro cells have no meta knobs or modes.
            knob::KnobEvent::Meta(..) | knob::KnobEvent::Mode(_) => {}
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
                let (Some(current), Some(reset)) = (p.route_settings(route), p.route_reset(route)) else { return };
                match ev {
                    knob::MetaEvent::Begin => self.dragging = true,
                    knob::MetaEvent::End => self.dragging = false,
                    knob::MetaEvent::Amount(a) => self.session.set_route(params, shared, route, patch::ModSettings { amount: a, ..current }),
                    knob::MetaEvent::Reset => self.session.set_route(params, shared, route, reset),
                    knob::MetaEvent::Menu(at) => self.meta_menu = Some(meta_menu::MetaMenu { route, at }),
                }
            }
            knob::KnobEvent::Drop => {
                let Some(source) = self.take_drop() else { return };
                self.session.add_route(params, source, patch::ModDestination::Param(spec.id.clone()));
            }
            // Block knobs have no grip, and no modes yet.
            knob::KnobEvent::Grab | knob::KnobEvent::Mode(_) => {}
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
            drawer_height: self.drawer.height(self.now),
        }
    }

    /// Every frame: let the chain surface age its transition, then set the renderer's zoom blur
    /// from the modal's `Look` (none at rest). The blur stays here: only the shell knows what else
    /// is on screen and has to stay sharp.
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

    /// The renderer's blur for this frame of the modal.
    fn blur(&self, look: &chain_surface::Look) -> iced_wgpu::postfx::RadialBlur {
        let (w, h) = theme::stage_size();
        let (x, y) = (theme::SPACE_GUTTER, theme::SIZE_TOP_BAR + theme::SPACE_GUTTER);
        let at = |r: iced::Rectangle| [x + r.x, y + r.y, r.width, r.height];
        let c = look.rect.center();
        // The chevrons stay sharp once the camera rests (only their stroke: a larger area would
        // show the unblurred chain behind them).
        let chevron = |r: iced::Rectangle| {
            if look.velocity > 0.0 { return [0.0; 4] }
            let c = r.center();
            [x + c.x - 7.0, y + c.y - 12.0, 14.0, 24.0]
        };
        let (left, right) = look.arrows();
        // What floats over the chain stays sharp: the notch (top centre), the drawer at its current
        // height (bottom centre, it may be sliding), the macro rail (top left), the modal and its
        // arrows, and whatever floats anywhere (a menu, a value entry, a dragged chip: no soft
        // blur then, they can sit over the chain).
        let (nw, nh) = notch::size();
        let dw = theme::drawer_width() + 2.0 * theme::RADIUS_WINDOW;
        let dh = self.drawer.height(self.now);
        let rail = if self.macros_t() > 0.0 { [x, y, macros_width(), macro_rail::HEIGHT + 2.0 * theme::SPACE_2] } else { [0.0; 4] };
        let library = self.library_t() * library_width();
        let sheet = self.slide_t() * sheet_width();
        iced_wgpu::postfx::RadialBlur {
            center: [x + c.x, y + c.y],
            rect: [x, y, w, h],
            // strongest mid-way, none at the ends
            amount: chain_surface::Look::BLUR * look.velocity,
            soft: 0.0,
            keep: [
                [x + (w - nw + library - sheet) / 2.0, y, nw, nh],
                [x + (w - dw) / 2.0, y + h - dh, dw, dh],
                rail,
                at(look.rect),
                chevron(left),
                chevron(right),
                [x, y, library, h],
                [x + w - sheet, y, sheet, h],
            ],
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
            self.set_macros(false);
        }
        if self.library_open != open {
            self.library_open = open;
            self.now = Instant::now();
            self.library_slide.go_mut(open, self.now);
        }
    }

    /// The macro rail shares the left side with the library: opening one closes the other.
    fn set_macros(&mut self, open: bool) {
        if open {
            self.set_library(false);
        }
        if self.macros_open != open {
            self.macros_open = open;
            self.now = Instant::now();
            self.macros_slide.go_mut(open, self.now);
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

    /// A press on the page (any view: surface, chain, block page, …) closes everything that
    /// floats over it: sheets on both sides, the macro rail, the drawer, a value entry. New
    /// floating things belong in here too.
    /// A press on the page closes what floats over it: the library, a sheet, the drawer, the value
    /// entry, the meta menu. The macro rail stays (a tool for the page: turn a macro, drag it onto
    /// a knob); it closes with its own button and Escape.
    fn dismiss(&mut self) {
        self.set_library(false);
        self.open_sheet(None);
        self.close_drawer();
        self.entry = None;
        self.meta_menu = None;
    }

    /// A macro's or modulator's grip was pressed: drag it (onto a knob).
    fn start_source_drag(&mut self, source: patch::RouteSource) {
        self.meta_menu = None;
        self.entry = None;
        self.dropped = None;
        self.source_drag = Some(routing::SourceDrag { source, at: None });
    }

    /// A knob reported a drop: the drag that just ended (if it was this recent), once.
    fn take_drop(&mut self) -> Option<patch::RouteSource> {
        let (d, at) = self.dropped.take()?;
        (at.elapsed() < Duration::from_millis(500)).then_some(d.source)
    }

    fn close_drawer(&mut self) {
        if self.drawer.is_open() {
            self.now = Instant::now();
            self.drawer.set_open(false, self.now);
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
            // The settings sheet reports what the captures folder holds; walking it once per
            // open is cheap, per frame would not be.
            if s == Sheet::Settings {
                self.captures = captures::usage();
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
            macros_available: s.current.as_ref().is_some_and(|c| !c.patch.macros.is_empty()),
            macros_open: self.macros_open,
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
        let d = self.source_drag.as_ref()?;
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

    /// The patch window with the shell's intrusions: the notch and the sheets.
    fn window(&self) -> Element<'_, Message> {
        let Some(cur) = &self.session.current else { return column![].into() };
        let bg = session::hex(cur.patch.theme.color("bg")).unwrap_or(theme::c().frame_raised);

        // While sheets are open the notch centres on the part of the window they leave visible,
        // gliding along with them.
        let notch = notch::view(notch::Props {
            surfaces: viewport::surfaces(&cur.patch),
            current: self.surface,
            snapshot: "Init",
            sync: if self.session.error.is_some() { notch::Sync::Error } else { notch::Sync::Ok },
        })
        .map(Message::Notch);
        let notch = container(notch)
            .width(Fill)
            .padding(Padding { left: self.library_t() * library_width(), right: self.slide_t() * sheet_width(), ..Padding::ZERO })
            .align_x(alignment::Horizontal::Center);

        // The drawer hangs into the window from its bottom edge, centred (sheets slide over it).
        let drawer = container(self.drawer.view(self.now, &cur.patch.modulators).map(Message::Drawer))
            .width(Fill)
            .height(Fill)
            .align_x(alignment::Horizontal::Center)
            .align_y(alignment::Vertical::Bottom);

        let content: Element<'_, Message> = match self.surface {
            viewport::SurfaceId::Chain => {
                let ctx = self.chain_ctx(&cur.patch);
                let look = self.chain.look(&ctx);
                self.chain.view(&ctx, look).map(Message::Chain)
            }
            viewport::SurfaceId::Matrix => self.matrix.view(&cur.patch, &self.session.values).map(Message::Matrix),
            // A patch surface: the patch's own, or the shell's default macro surface.
            viewport::SurfaceId::Patch(_) => match &cur.panel {
                Some(panel) => stage::view(panel, bg).map(Message::Panel),
                None => {
                    let brand = self.session.library.iter().find(|p| p.folder == cur.folder).map(|p| &p.brand);
                    surface::view(&cur.patch, brand, &self.session.values).map(Message::Surface)
                }
            },
        };
        let content = press::on_any_press(content, Message::Dismiss);
        let mut layers = stack![content, notch, drawer].width(Fill).height(Fill);
        for layer in [self.macros_layer(), self.library_layer(), self.sheet_layer()].into_iter().flatten() {
            layers = layers.push(layer);
        }
        if let Some(e) = &self.entry {
            // Right of the click point, clamped into the stage.
            let (s, offset) = stage::panel_fit();
            let (w, h) = theme::stage_size();
            let x = (offset.x + e.at.x * s + 14.0).min(w - entry::SIZE.0 - 8.0).max(8.0);
            let y = (offset.y + e.at.y * s - entry::SIZE.1 / 2.0).min(h - entry::SIZE.1 - 8.0).max(8.0);
            layers = layers.push(pin(e.view().map(Message::Entry)).x(x).y(y));
        }
        if let Some(layer) = self.meta_menu_layer() {
            layers = layers.push(layer);
        }
        layers = layers.push(self.error_card());
        if self.dev_stats {
            layers = layers.push(self.stats());
        }

        container(layers)
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

    /// 0 = macro rail hidden, 1 = fully in.
    fn macros_t(&self) -> f32 {
        self.macros_slide.interpolate(0.0, 1.0, self.now)
    }

    /// The macro rail, over the left side of the window, under the top bar.
    fn macros_layer(&self) -> Option<Element<'_, Message>> {
        let t = self.macros_t();
        if t <= 0.0 {
            return None;
        }
        let cur = self.session.current.as_ref()?;
        let values = &self.session.values;
        let macros = (0..cur.patch.macros.len())
            .map(|slot| {
                let v = values.macros[slot];
                knob::Macro {
                    slot,
                    label: &cur.patch.macros[slot].name,
                    value_text: cur.patch.macro_text(slot, v),
                    value: v,
                    default: cur.patch.macro_default(slot),
                    drop: self.source_drag.as_ref().and_then(routing::SourceDrag::macro_drop),
                }
            })
            .collect();
        let rail = self.macro_rail.view(self.now, macros).map(Message::Macros);
        let rail = container(rail).height(Fill).padding(theme::SPACE_2);
        Some(slide::slide_left(rail, macros_width(), (1.0 - t) * macros_width()).into())
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

/// The library sheet plus its margin.
fn library_width() -> f32 {
    theme::SIZE_LIBRARY + 2.0 * theme::SPACE_2
}

/// The macro rail plus its margin.
fn macros_width() -> f32 {
    macro_rail::WIDTH + 2.0 * theme::SPACE_2
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
