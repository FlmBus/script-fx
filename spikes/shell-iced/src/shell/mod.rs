//! The shell: library | stage (patch window + notch + chat sheet) over the signal line.
//!
//! Structure (one module per section, each with its own state, `Message`, `update`, `view`):
//! - `library`      patch/snapshot list in the sidebar
//! - `notch`        snapshot selector, save, load / reload, chat, settings
//! - `stage`        the patch window (GPU layer via a wgpu shader; patch UIs render here later)
//! - `chat`         the paper chat sheet
//! - `signal_line`  IN → nodes → OUT (view only)
//! - `empty`        the "No patch loaded" window
//!
//! This file owns the state that several sections share (which patch/snapshot is loaded,
//! whether the chat is open) and routes messages. Sections report things the shell has to act
//! on as an `Event` from their `update`; they never reach into each other.

pub mod theme;

mod chat;
mod empty;
mod icons;
mod library;
mod mock;
mod notch;
mod signal_line;
mod slide;
mod stage;

use nice_plug::prelude::AtomicF32;
use nice_plug_iced::{
    IcedNiceContext, PersistentState,
    iced::{
        self, Element, Fill, Padding, Subscription, Theme, alignment,
        animation::{Animation, Easing},
        futures::{Stream, channel::mpsc},
        widget::{column, container, row, stack},
    },
};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// What is loaded: a patch from the library and one of its snapshots.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Loaded {
    pub patch: usize,
    pub snapshot: usize,
}

pub struct Shell {
    persistent: PersistentState<Arc<AtomicF32>>,
    _ctx: IcedNiceContext,
    loaded: Option<Loaded>,
    chat_open: bool,
    /// Drives the chat sheet's slide (and the notch following it).
    chat_slide: Animation<bool>,
    /// Time of the last animation frame; `view` interpolates at this instant.
    now: Instant,
    library: library::Library,
    chat: chat::Chat,
    empty: empty::Empty,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// Host/audio thread changed something (meters); nothing to do yet, the shader reads the
    /// level itself.
    Poll,
    /// An animation frame (only subscribed while something animates).
    Frame(Instant),
    Library(library::Message),
    Notch(notch::Message),
    Chat(chat::Message),
    Empty(empty::Message),
}

impl Shell {
    pub fn new(persistent: PersistentState<Arc<AtomicF32>>, ctx: IcedNiceContext) -> Self {
        // Dev only: SPIKE_STATE=empty | nochat | neon picks the start state for screenshots.
        let start = std::env::var("SPIKE_STATE").unwrap_or_default();
        let loaded = match start.as_str() {
            "empty" => None,
            "neon" => Some(Loaded { patch: 0, snapshot: 1 }),
            _ => Some(Loaded { patch: 1, snapshot: 0 }),
        };
        let chat_open = std::env::var("SPIKE_DEMO").is_ok() || (loaded.is_some() && start != "nochat" && start != "neon");
        Self {
            persistent,
            _ctx: ctx,
            loaded,
            chat_open,
            chat_slide: {
                let a = Animation::new(chat_open).duration(Duration::from_millis(260)).easing(Easing::EaseOutCubic);
                // Dev only: SPIKE_DEMO=1 slides the chat in 2 s after start (for recordings).
                if std::env::var("SPIKE_DEMO").is_ok() {
                    Animation::new(false)
                        .duration(Duration::from_millis(260))
                        .easing(Easing::EaseOutCubic)
                        .delay(Duration::from_secs(2))
                        .go(true, Instant::now())
                } else {
                    a
                }
            },
            now: Instant::now(),
            library: library::Library::new(loaded),
            chat: chat::Chat::new(),
            empty: empty::Empty::default(),
        }
    }

    pub fn theme(&self) -> Option<Theme> {
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

    pub fn style(&self, _theme: &Theme) -> iced::theme::Style {
        iced::theme::Style { background_color: theme::FRAME, text_color: theme::FRAME_INK_3 }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let poll = iced::poll_events().map(|_| Message::Poll);
        if self.chat_slide.is_animating(self.now) {
            Subscription::batch([poll, Subscription::run(frames).map(Message::Frame)])
        } else {
            poll
        }
    }

    pub fn update(&mut self, message: Message) {
        match message {
            Message::Poll => {}
            Message::Frame(now) => self.now = now,
            Message::Library(msg) => match self.library.update(msg) {
                Some(library::Event::Load(loaded)) => self.load(Some(loaded)),
                Some(library::Event::Unload) => self.load(None),
                None => {}
            },
            Message::Notch(msg) => match msg {
                notch::Message::NextSnapshot => {
                    if let Some(l) = self.loaded {
                        let n = mock::PATCHES[l.patch].snapshots.len();
                        self.load(Some(Loaded { snapshot: (l.snapshot + 1) % n, ..l }));
                    }
                }
                notch::Message::ToggleChat => self.set_chat(!self.chat_open),
                // Not wired up in the spike.
                notch::Message::Save | notch::Message::Load | notch::Message::Reload | notch::Message::Settings => {}
            },
            Message::Chat(msg) => match self.chat.update(msg) {
                Some(chat::Event::Close) => self.set_chat(false),
                None => {}
            },
            Message::Empty(msg) => match self.empty.update(msg) {
                Some(empty::Event::Build) => self.load(Some(Loaded { patch: 0, snapshot: 0 })),
                None => {}
            },
        }
    }

    fn load(&mut self, loaded: Option<Loaded>) {
        self.loaded = loaded;
        self.library.loaded_changed(loaded);
        if loaded.is_none() {
            self.set_chat(false);
        }
    }

    fn set_chat(&mut self, open: bool) {
        self.chat_open = open;
        self.now = Instant::now();
        self.chat_slide.go_mut(open, self.now);
    }

    pub fn view(&self) -> Element<'_, Message> {
        let library = self.library.view().map(Message::Library);

        let main: Element<'_, Message> = match self.loaded {
            Some(loaded) => {
                let patch = &mock::PATCHES[loaded.patch];
                column![self.window(loaded), signal_line::view(patch)].into()
            }
            None => container(self.empty.view().map(Message::Empty))
                .padding(Padding { top: theme::SPACE_GUTTER, right: theme::SPACE_GUTTER, bottom: theme::SPACE_GUTTER, left: 0.0 })
                .into(),
        };

        container(row![library, main]).style(theme::frame).width(Fill).height(Fill).into()
    }

    /// The patch window with the shell's two intrusions: the notch and the chat sheet.
    fn window(&self, loaded: Loaded) -> Element<'_, Message> {
        let patch = &mock::PATCHES[loaded.patch];
        let level = Arc::clone(&self.persistent);

        // 0 = sheet hidden, 1 = fully in. While it's open, the notch centres on the part of the
        // window the sheet leaves visible, gliding along with it.
        let t: f32 = self.chat_slide.interpolate(0.0, 1.0, self.now);
        let sheet_w = theme::SIZE_SHEET + 2.0 * theme::SPACE_2;
        let sheet_room = t * sheet_w;
        let notch = container(notch::view(patch, loaded.snapshot, self.chat_open).map(Message::Notch))
            .width(Fill)
            .padding(Padding { right: sheet_room, ..Padding::ZERO })
            .align_x(alignment::Horizontal::Center);

        let mut layers = stack![stage::view(patch, level), notch].width(Fill).height(Fill);
        if t > 0.0 {
            let sheet = container(self.chat.view(patch).map(Message::Chat)).height(Fill).padding(theme::SPACE_2);
            layers = layers.push(slide::slide(sheet, sheet_w, (1.0 - t) * sheet_w));
        }

        container(layers)
            .padding(Padding { top: theme::SPACE_GUTTER, right: theme::SPACE_GUTTER, ..Padding::ZERO })
            .width(Fill)
            .height(Fill)
            .into()
    }
}

/// ~60 Hz frame ticks from a helper thread (the thread-pool executor has no timers). The
/// thread ends when the subscription is dropped, i.e. when nothing animates any more.
fn frames() -> impl Stream<Item = Instant> {
    let (mut tx, rx) = mpsc::channel(1);
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_micros(16_667));
            if let Err(e) = tx.try_send(Instant::now()) {
                if e.is_disconnected() {
                    break;
                }
            }
        }
    });
    rx
}
