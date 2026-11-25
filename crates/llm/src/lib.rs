//! The LLM behind the chat: a message (plus the loaded patch, its values and an attached capture)
//! becomes a reply and, if the model changes a patch, a **validated** [`Proposal`].
//!
//! - `claude`     backend: the user's installed, logged-in `claude` CLI (we never see credentials)
//! - `http`       backend: any OpenAI-compatible API
//! - `prompt`     what the model is told, and the shape of its answer
//! - `workspace`  patch files: reading, staging, committing with a backup, undo
//! - `check`      is a staged patch good? (loader, panel render, DSP, luau-lsp)
//! - `turn`       ask → stage → check → feed errors back
//!
//! Everything blocks: run a turn on a worker thread. Nothing here writes to the patches folder
//! except [`Proposal::commit`] and [`Applied::undo`].

mod check;
mod claude;
mod http;
mod prompt;
mod turn;
mod workspace;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub use check::check;
pub use claude::{ClaudeCli, find_claude};
pub use http::OpenAiCompat;
pub use turn::{Outcome, Progress, Turn, run};
pub use workspace::{Applied, Proposal};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone)]
pub struct Message {
    pub role: Role,
    pub text: String,
}

/// A model the user can pick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    /// Empty = the backend's default.
    pub id: String,
    pub name: String,
}

pub struct Request {
    pub system: String,
    pub messages: Vec<Message>,
    pub schema: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    Cancelled,
    /// Shown to the user as is.
    Failed(String),
}

pub(crate) fn failed(msg: impl Into<String>) -> Error {
    Error::Failed(msg.into())
}

/// One request → the model's answer as JSON text.
pub trait Backend: Send {
    fn complete(&self, req: &Request, cancel: &Cancel) -> Result<String, Error>;
    /// The models this login or key can use. Costs no tokens.
    fn models(&self, cancel: &Cancel) -> Result<Vec<Model>, Error>;
}

#[derive(Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// `<user data dir>/ScriptFX/<name>`.
pub(crate) fn data_dir(name: &str) -> PathBuf {
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("ScriptFX").join(name)
}
