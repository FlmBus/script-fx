//! The shell: the plugin's own UI (iced), in the "ScriptFX Shell" design system.
//! Library | patch window (ControlPanel + notch + paper sheets) over the signal line.
//!
//! One module per section, each with its own state, `Message`, `update` and `view`:
//! - `library`      patch list in the sidebar
//! - `notch`        snapshot selector, save, load / reload, chat, settings
//! - `stage`        the patch window: the ControlPanel and its pointer input
//! - `chat`         the chat sheet (Listen lives here)
//! - `controls`     the controls sheet (macros, params, patch settings)
//! - `signal_line`  IN → blocks → OUT (view only)
//! - `empty`        the window when no patch is loaded
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
mod icons;
mod library;
mod notch;
mod session;
mod sheet;
mod signal_line;
mod slide;
mod stage;

use std::sync::Arc;
use std::time::{Duration, Instant};

use nice_plug_iced::{
    IcedEditor, IcedEditorState, IcedNiceContext, IcedNiceSettings, PersistentState, application, create_iced_editor,
    iced::{
        self, Border, Element, Fill, Padding, PollSubNotifier, Subscription, Task, Theme, alignment,
        animation::{Animation, Easing},
        futures::{Stream, channel::mpsc},
        widget::{button, column, container, row, stack, text},
    },
};

use crate::listen::Listen;
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
    /// The open sheet; `shown` keeps the last one drawn while it slides out.
    sheet: Option<Sheet>,
    shown: Sheet,
    slide: Animation<bool>,
    now: Instant,
    /// Macro slot being dragged on the controls sheet (host gesture open).
    dragging_macro: Option<usize>,
    dev_stats: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    Frame(Instant),
    Library(library::Message),
    Notch(notch::Message),
    Chat(chat::Message),
    Controls(controls::Message),
    Empty(empty::Message),
    Panel(stage::Pointer),
    DismissError,
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
            now: Instant::now(),
            dragging_macro: None,
            dev_stats: std::env::var_os("SCRIPTFX_DEV_STATS").is_some(),
        }
    }

    fn theme(&self) -> Option<Theme> {
        Some(Theme::custom(
            "ScriptFX Shell",
            iced::theme::Palette {
                background: theme::FRAME,
                text: theme::FRAME_INK_3,
                primary: theme::FRAME_INVERT,
                success: theme::LED_OK,
                warning: theme::LED_REDUCE,
                danger: theme::LED_CLIP,
            },
        ))
    }

    fn style(&self, _: &Theme) -> iced::theme::Style {
        iced::theme::Style { background_color: theme::FRAME, text_color: theme::FRAME_INK_3 }
    }

    /// The frame clock drives everything that moves: meters, animated components, the sheet
    /// slide, hot reload, Listen progress.
    fn subscription(&self) -> Subscription<Message> {
        Subscription::run(frames).map(Message::Frame)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        self.handle(message);
        // Patch fonts found by a library scan: hand them to iced.
        let fonts = std::mem::take(&mut self.session.new_fonts);
        Task::batch(fonts.into_iter().map(|bytes| iced::font::load(bytes).discard()))
    }

    fn handle(&mut self, message: Message) {
        let Plugin { params, shared } = &*self.plugin;
        let (params, shared) = (params.clone(), shared.clone());
        let ctx = &self.ctx.nice_context.clone();
        match message {
            Message::Frame(now) => {
                self.now = now;
                if !self.restored {
                    self.restored = true;
                    self.session.restore(ctx, &params, &shared);
                    self.reveal_current();
                    // Dev aid: SCRIPTFX_DEV_SHEET=chat|controls opens a sheet at start.
                    match std::env::var("SCRIPTFX_DEV_SHEET").as_deref() {
                        Ok("chat") => self.open_sheet(Some(Sheet::Chat)),
                        Ok("controls") => self.open_sheet(Some(Sheet::Controls)),
                        _ => {}
                    }
                    // Dev aid: SCRIPTFX_DEV_AUTOLISTEN=10 starts a 10 s listen right away.
                    if let Some(s) = std::env::var("SCRIPTFX_DEV_AUTOLISTEN").ok().and_then(|s| s.parse::<f32>().ok()) {
                        self.listen.seconds = s;
                        self.listen.start(&shared);
                        self.open_sheet(Some(Sheet::Chat));
                    }
                }
                self.session.tick(ctx, &params, &shared);
                if let Some(result) = self.listen.poll(&shared) {
                    self.chat.capture_done(result);
                    self.open_sheet(Some(Sheet::Chat));
                }
                let scale = stage::panel_fit().0 * self.ctx.window_size().scale_factor as f32;
                let Session { current, values, .. } = &mut self.session;
                if let Some(cur) = current {
                    if let Some(panel) = &mut cur.panel {
                        panel.update(&cur.patch, values, scale);
                    }
                }
            }
            Message::Library(msg) => match self.library.update(msg) {
                Some(library::Event::Load(folder)) => {
                    self.session.load(ctx, &params, &shared, &folder, true);
                    self.reveal_current();
                }
                Some(library::Event::Unload) => {
                    self.session.unload(ctx, &params, &shared);
                    self.open_sheet(None);
                }
                None => {}
            },
            Message::Notch(msg) => match msg {
                notch::Message::Reload => self.session.reload(ctx, &params, &shared),
                notch::Message::ToggleChat => self.toggle_sheet(Sheet::Chat),
                notch::Message::ToggleControls => self.toggle_sheet(Sheet::Controls),
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
            Message::Empty(msg) => match self.empty.update(msg) {
                Some(e) => match e {},
                None => {}
            },
            Message::Panel(ev) => {
                let Some(cur) = &mut self.session.current else { return };
                let Some(panel) = &mut cur.panel else { return };
                let changes = panel.pointer(ev, &cur.patch, &self.session.values);
                for c in changes {
                    self.apply(c, &params, &shared);
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
        }
    }

    fn reveal_current(&mut self) {
        if let Some(cur) = &self.session.current {
            self.library.reveal(&cur.folder);
        }
    }

    fn toggle_sheet(&mut self, s: Sheet) {
        self.open_sheet(if self.sheet == Some(s) { None } else { Some(s) });
    }

    fn open_sheet(&mut self, s: Option<Sheet>) {
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
        let library = self.library.view(&s.library, s.current.as_ref().map(|c| c.folder.as_str())).map(Message::Library);

        let main: Element<'_, Message> = match &s.current {
            Some(_) => column![self.window(), signal_line::view(s.current.as_ref().map(|c| &c.patch), &s.values)].into(),
            None => {
                let empty = container(self.empty.view().map(Message::Empty)).padding(Padding {
                    top: theme::SPACE_GUTTER,
                    right: theme::SPACE_GUTTER,
                    bottom: theme::SPACE_GUTTER,
                    left: 0.0,
                });
                stack![empty, self.error_card()].into()
            }
        };

        container(row![library, main]).style(theme::frame).width(Fill).height(Fill).into()
    }

    /// The patch window with the shell's two intrusions: the notch and a paper sheet.
    fn window(&self) -> Element<'_, Message> {
        let Some(cur) = &self.session.current else { return column![].into() };
        let bg = session::hex(cur.patch.theme.color("bg")).unwrap_or(theme::FRAME_RAISED);

        // 0 = sheet hidden, 1 = fully in. While a sheet is open the notch centres on the part
        // of the window it leaves visible, gliding along with it.
        let t: f32 = self.slide.interpolate(0.0, 1.0, self.now);
        let sheet_w = theme::SIZE_SHEET + 2.0 * theme::SPACE_2;
        let notch = notch::view(notch::Props {
            snapshot: "Init",
            sync: if self.session.error.is_some() { notch::Sync::Error } else { notch::Sync::Ok },
            chat_open: self.sheet == Some(Sheet::Chat),
            controls_open: self.sheet == Some(Sheet::Controls),
        })
        .map(Message::Notch);
        let notch = container(notch)
            .width(Fill)
            .padding(Padding { right: t * sheet_w, ..Padding::ZERO })
            .align_x(alignment::Horizontal::Center);

        let mut layers = stack![stage::view(&cur.patch, cur.panel.as_ref(), bg).map(Message::Panel), notch].width(Fill).height(Fill);
        if t > 0.0 {
            let sheet: Element<'_, Message> = match self.shown {
                Sheet::Chat => self.chat.view(&cur.patch.name, self.listen.status(&self.plugin.shared), self.listen.seconds).map(Message::Chat),
                Sheet::Controls => controls::view(&cur.patch, &self.session.values, self.session.hot_reload).map(Message::Controls),
            };
            let sheet = container(sheet).height(Fill).padding(theme::SPACE_2);
            layers = layers.push(slide::slide(sheet, sheet_w, (1.0 - t) * sheet_w));
        }
        layers = layers.push(self.error_card());
        if self.dev_stats {
            layers = layers.push(self.stats());
        }

        container(layers)
            .padding(Padding { top: theme::SPACE_GUTTER, right: theme::SPACE_GUTTER, ..Padding::ZERO })
            .width(Fill)
            .height(Fill)
            .into()
    }

    /// The last load error, as a paper card at the bottom left of the window.
    fn error_card(&self) -> Element<'_, Message> {
        let Some(err) = &self.session.error else { return column![].into() };
        let card = container(
            row![
                column![
                    text("Couldn't load the patch").size(12.5).font(theme::font::PAPER_SEMIBOLD).color(theme::PAPER_INK),
                    text(err.as_str()).size(11).font(theme::font::PAPER_MONO).color(theme::PAPER_INK_2),
                    text("The previous version keeps running. Fix the file and it reloads.").size(11).font(theme::font::PAPER).color(theme::PAPER_MUTED),
                ]
                .spacing(4)
                .width(Fill),
                button(container(icons::icon(icons::CLOSE, 10.0, 2.5, theme::PAPER_INK)).center(Fill))
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
            border: Border { radius: theme::radius(theme::RADIUS_FIELD), width: 1.0, color: theme::LED_CLIP },
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
        container(text(s).size(10.5).font(theme::font::MONO).color(theme::alpha(theme::FRAME_INK, 0.7)))
            .width(Fill)
            .height(Fill)
            .padding(10)
            .align_right(Fill)
            .align_bottom(Fill)
            .into()
    }
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
