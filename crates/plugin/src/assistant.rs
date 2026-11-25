//! The chat's state and its LLM worker. Lives in `Shared`, so a chat and a running turn survive
//! closing the editor; the worker only reports back, the shell writes and loads patches.
//! Not saved to disk yet, and one chat per plugin instance.

use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use llm::{Applied, Backend, Cancel, ClaudeCli, Error, Message, Model, OpenAiCompat, Outcome, Progress, Role, Turn};

use crate::listen::CaptureResult;
use crate::patches_root;
use crate::user_settings::{Ai, Provider};

const KEYCHAIN_SERVICE: &str = "ScriptFX";

pub enum Item {
    User { text: String, capture: Option<Arc<CaptureResult>> },
    Reply(String),
    Capture(Arc<CaptureResult>),
    Change(Change),
    Error(String),
    /// A new conversation: the model sees nothing from before it.
    NewChat,
}

/// A patch change the assistant made.
pub struct Change {
    pub applied: Applied,
    /// The patch that was loaded before a new one was.
    pub previous: Option<String>,
    pub undone: bool,
}

#[derive(Default)]
struct JobState {
    progress: Option<Progress>,
    done: Option<Result<Outcome, Error>>,
}

struct Job {
    cancel: Cancel,
    state: Arc<Mutex<JobState>>,
    thread: Option<JoinHandle<()>>,
    started: Instant,
}

impl Drop for Job {
    /// The thread runs our code: it must end before the host may unload the library. Cancelling
    /// kills the CLI or drops the request, so this is short.
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[derive(Default)]
pub struct Assistant {
    pub log: Vec<Item>,
    /// Goes with the next message.
    pub attached: Option<Arc<CaptureResult>>,
    job: Option<Job>,
    /// Helper threads (login check, keychain) and the settings sheet's status line.
    helpers: Vec<JoinHandle<()>>,
    connection: Arc<Mutex<Connection>>,
    /// The newest helper's; a newer one cancels it.
    probe: Cancel,
}

/// The chosen provider, as the settings sheet shows it.
#[derive(Default, Clone)]
pub struct Connection {
    pub status: String,
    /// Empty = not (yet) known: the model is typed instead.
    pub models: Vec<Model>,
}

impl Drop for Assistant {
    fn drop(&mut self) {
        self.job = None;
        self.probe.cancel();
        for t in self.helpers.drain(..) {
            let _ = t.join();
        }
    }
}

impl Assistant {
    pub fn capture_done(&mut self, result: Arc<CaptureResult>) {
        self.log.push(Item::Capture(result.clone()));
        self.attached = Some(result);
    }

    /// Starts a turn, in a new conversation if `fresh` (an attached capture goes along).
    /// False: nothing was sent (why is in the log), keep the draft.
    pub fn send(&mut self, ai: &Ai, text: String, current: Option<(String, String)>, fresh: bool) -> bool {
        if self.job.is_some() {
            return false;
        }
        if ai.provider == Provider::Off {
            self.log.push(Item::Error("No AI is connected. Pick one in Settings.".into()));
            return false;
        }
        if fresh && !self.history().is_empty() {
            self.log.push(Item::NewChat);
        }
        let history = self.history();
        let capture = self.attached.take();
        let turn = Turn {
            root: patches_root(),
            current,
            history,
            text: text.clone(),
            capture: capture.as_ref().map(|c| c.llm_json()),
        };
        self.log.push(Item::User { text, capture });

        let (cancel, state, ai) = (Cancel::default(), Arc::new(Mutex::new(JobState::default())), ai.clone());
        let (c, s) = (cancel.clone(), state.clone());
        let thread = std::thread::spawn(move || {
            let work = || {
                let backend = backend(&ai).map_err(Error::Failed)?;
                llm::run(&turn, &*backend, &c, &|p| s.lock().unwrap().progress = Some(p))
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
                .unwrap_or_else(|_| Err(Error::Failed("Something went wrong inside the plugin; nothing changed.".into())));
            s.lock().unwrap().done = Some(result);
        });
        self.job = Some(Job { cancel, state, thread: Some(thread), started: Instant::now() });
        true
    }

    pub fn cancel(&mut self) {
        if let Some(j) = &self.job {
            j.cancel.cancel();
        }
    }

    /// The finished turn's result, once.
    pub fn poll(&mut self) -> Option<Result<Outcome, Error>> {
        let done = self.job.as_ref()?.state.lock().unwrap().done.take()?;
        self.job = None;
        Some(done)
    }

    /// What the running turn is doing.
    pub fn activity(&self) -> Option<String> {
        let j = self.job.as_ref()?;
        let what = if j.cancel.is_cancelled() {
            "Cancelling…".to_string()
        } else {
            match j.state.lock().unwrap().progress {
                Some(Progress::Checking) => "Checking the patch…".into(),
                Some(Progress::Repairing(n)) => format!("Fixing errors ({n} of 2)…"),
                _ => "Thinking…".into(),
            }
        };
        Some(format!("{what} {} s", j.started.elapsed().as_secs()))
    }

    /// The conversation for the model. Patch sources aren't repeated: each turn sends them fresh.
    fn history(&self) -> Vec<Message> {
        let mut out: Vec<Message> = Vec::new();
        let start = self.log.iter().rposition(|i| matches!(i, Item::NewChat)).map_or(0, |i| i + 1);
        for item in &self.log[start..] {
            let (role, text) = match item {
                Item::User { text, .. } => (Role::User, text.clone()),
                Item::Reply(text) => (Role::Assistant, text.clone()),
                Item::Change(c) => {
                    let what = if c.applied.is_new { "created" } else { "changed" };
                    let undone = if c.undone { "; the user undid this" } else { "" };
                    (Role::Assistant, format!("[{what} patch `{}`{undone}]", c.applied.folder))
                }
                Item::Capture(_) | Item::Error(_) | Item::NewChat => continue,
            };
            match out.last_mut().filter(|m| m.role == role) {
                Some(m) => m.text += &format!("\n\n{text}"),
                None => out.push(Message { role, text }),
            }
        }
        out
    }

    // ---- settings -------------------------------------------------------------------------------

    pub fn connection(&self) -> Connection {
        self.connection.lock().unwrap().clone()
    }

    /// Finds out whether the chosen provider is ready and which models it has.
    pub fn probe(&mut self, ai: &Ai) {
        let ai = ai.clone();
        self.helper(move |cancel| connect(&ai, cancel));
    }

    /// Saves (or, if empty, removes) the key for the API address, then connects with it.
    pub fn save_key(&mut self, ai: &Ai, key: String) {
        let ai = ai.clone();
        self.helper(move |cancel| match write_key(&ai.base_url, key.trim()) {
            Ok(()) => connect(&ai, cancel),
            Err(status) => Connection { status, models: Vec::new() },
        });
    }

    /// The address changed: what we know is about another one.
    pub fn forget_connection(&mut self) {
        self.probe.cancel();
        *self.connection.lock().unwrap() = Connection { status: "Enter connects".into(), models: Vec::new() };
    }

    fn helper(&mut self, f: impl FnOnce(&Cancel) -> Connection + Send + 'static) {
        self.helpers.retain(|t| !t.is_finished());
        self.probe.cancel();
        self.probe = Cancel::default();
        let (cancel, connection) = (self.probe.clone(), self.connection.clone());
        *connection.lock().unwrap() = Connection { status: "connecting…".into(), models: Vec::new() };
        self.helpers.push(std::thread::spawn(move || {
            let found = f(&cancel);
            let mut connection = connection.lock().unwrap();
            // Superseded: the result is about other settings.
            if !cancel.is_cancelled() {
                *connection = found;
            }
        }));
    }
}

fn connect(ai: &Ai, cancel: &Cancel) -> Connection {
    let only = |status: &str| Connection { status: status.into(), models: Vec::new() };
    let ready = match ai.provider {
        Provider::Off => return only(""),
        Provider::Claude => match claude(ai) {
            None => return only("Claude Code isn't installed (or not found)"),
            Some(c) if !c.logged_in(cancel) => return only("Claude Code found, not logged in: run `claude` in a terminal"),
            Some(_) => "Claude Code found, logged in",
        },
        Provider::Api => match read_key(&ai.base_url) {
            Ok(k) if k.is_empty() && !ai.base_url.starts_with("http://") => return only("no key saved for this address"),
            _ => "connected",
        },
    };
    match backend(ai).map_err(Error::Failed).and_then(|b| b.models(cancel)) {
        Ok(models) => Connection { status: format!("{ready} · {} models", models.len()), models },
        Err(Error::Cancelled) => only(""),
        Err(Error::Failed(e)) if ai.provider == Provider::Claude => only(&format!("{ready}; {e}")),
        Err(Error::Failed(e)) => only(&e),
    }
}

fn claude(ai: &Ai) -> Option<ClaudeCli> {
    let dir = ai.claude_config_dir.trim();
    Some(ClaudeCli { bin: llm::find_claude()?, model: ai.claude_model.trim().into(), config_dir: (!dir.is_empty()).then(|| dir.into()) })
}

fn backend(ai: &Ai) -> Result<Box<dyn Backend>, String> {
    match ai.provider {
        Provider::Off => Err("No AI is connected.".into()),
        Provider::Claude => Ok(Box::new(claude(ai).ok_or("Claude Code isn't installed (or not found).")?)),
        Provider::Api => {
            let key = read_key(&ai.base_url);
            // A local server needs no key, so a missing keychain doesn't matter there.
            let key = if ai.base_url.starts_with("http://") { key.unwrap_or_default() } else { key? };
            Ok(Box::new(OpenAiCompat { base_url: ai.base_url.clone(), model: ai.api_model.clone(), key }))
        }
    }
}

/// Keys are saved per address, so changing it never sends a key elsewhere.
fn entry(base_url: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYCHAIN_SERVICE, base_url.trim().trim_end_matches('/')).map_err(|e| format!("keychain: {e}"))
}

fn read_key(base_url: &str) -> Result<String, String> {
    match entry(base_url)?.get_password() {
        Ok(k) => Ok(k),
        Err(keyring::Error::NoEntry) => Ok(String::new()),
        Err(e) => Err(format!("keychain: {e}")),
    }
}

fn write_key(base_url: &str, key: &str) -> Result<(), String> {
    let e = entry(base_url)?;
    let r = if key.is_empty() { e.delete_credential() } else { e.set_password(key) };
    match r {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("keychain: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_chat_hides_what_came_before() {
        let user = |t: &str| Item::User { text: t.into(), capture: None };
        let mut a = Assistant::default();
        a.log.extend([user("old"), Item::Reply("old".into())]);
        assert_eq!(a.history().len(), 2);
        a.log.push(Item::NewChat);
        assert!(a.history().is_empty());
        a.log.push(user("new"));
        assert_eq!(a.history()[0].text, "new");
        // Nothing sent (AI off): no new chat either.
        assert!(!a.send(&Ai::default(), "x".into(), None, true));
        assert_eq!(a.history()[0].text, "new");
    }
}
