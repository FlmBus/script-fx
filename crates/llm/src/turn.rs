//! One chat turn: ask, stage, check, feed errors back (at most [`REPAIRS`] times).

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::{Backend, Cancel, Error, Message, Proposal, Request, Role, check, failed, prompt, workspace};
use prompt::Action;

const REPAIRS: usize = 2;
/// Earlier messages that go along.
const HISTORY: usize = 20;

pub struct Turn {
    pub root: PathBuf,
    /// The loaded patch's folder and its current values as text.
    pub current: Option<(String, String)>,
    pub history: Vec<Message>,
    pub text: String,
    /// Analysis JSON.
    pub capture: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Progress {
    Thinking,
    Checking,
    Repairing(usize),
}

#[derive(Debug)]
pub struct Outcome {
    pub reply: String,
    pub proposal: Option<Proposal>,
}

pub fn run(turn: &Turn, backend: &dyn Backend, cancel: &Cancel, progress: &dyn Fn(Progress)) -> Result<Outcome, Error> {
    let sources = turn.current.as_ref().map(|(folder, _)| workspace::read_luau(&turn.root.join(folder)));
    let current = turn.current.as_ref().zip(sources.as_ref()).map(|((f, v), s)| (f.as_str(), s, v.as_str()));
    let mut messages: Vec<Message> = turn.history[turn.history.len().saturating_sub(HISTORY)..].to_vec();
    messages.push(Message { role: Role::User, text: prompt::user(&turn.text, current, turn.capture.as_deref()) });
    let mut req = Request { system: prompt::system(), messages, schema: prompt::schema() };

    for attempt in 0..=REPAIRS {
        progress(if attempt == 0 { Progress::Thinking } else { Progress::Repairing(attempt) });
        let text = backend.complete(&req, cancel)?;
        progress(Progress::Checking);
        let last = attempt == REPAIRS;
        let problems = match evaluate(turn, &text, cancel, last) {
            Ok(outcome) => {
                return if cancel.is_cancelled() { Err(Error::Cancelled) } else { Ok(outcome) };
            }
            Err(p) => p,
        };
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        if last {
            return Err(failed(format!("The model's patch still has errors after {REPAIRS} repairs, so nothing changed:\n\n{}", problems.join("\n"))));
        }
        req.messages.push(Message { role: Role::Assistant, text });
        req.messages.push(Message { role: Role::User, text: prompt::repair(&problems) });
    }
    unreachable!()
}

/// `last`: no repair left, so accept a patch that runs but has type errors.
fn evaluate(turn: &Turn, text: &str, cancel: &Cancel, last: bool) -> Result<Outcome, Vec<String>> {
    let a = prompt::parse(text).map_err(|e| vec![e])?;
    let (folder, is_new) = match a.action {
        Action::None => return Ok(Outcome { reply: a.reply, proposal: None }),
        Action::Create => (workspace::new_folder(&turn.root, &a.name), true),
        Action::Edit => match &turn.current {
            Some((f, _)) => (f.clone(), false),
            None => return Err(vec!["no patch is loaded: use action \"create\"".into()]),
        },
    };
    let mut files: BTreeMap<String, Option<String>> = a.delete.into_iter().map(|p| (p, None)).collect();
    files.extend(a.files.into_iter().map(|f| (f.path, Some(f.content))));
    if files.is_empty() {
        return Err(vec!["the action changes the patch, but there are no files".into()]);
    }
    workspace::valid_files(&files).map_err(|e| vec![e])?;
    let staged = workspace::Staged::new(&turn.root, &folder, is_new, &files).map_err(|e| vec![format!("writing the files failed: {}", e.kind())])?;
    let mut found = check::check(&staged.dir, cancel);
    if found.errors.is_empty() && (last || found.types.is_empty()) {
        return Ok(Outcome { reply: a.reply, proposal: Some(Proposal { folder, is_new, files }) });
    }
    found.errors.append(&mut found.types);
    Err(found.errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Answers from a script and records what it was asked.
    struct Fake(Mutex<Vec<String>>, Mutex<Vec<String>>);
    impl Backend for Fake {
        fn complete(&self, req: &Request, _: &Cancel) -> Result<String, Error> {
            self.1.lock().unwrap().push(req.messages.last().unwrap().text.clone());
            Ok(self.0.lock().unwrap().remove(0))
        }
        fn models(&self, _: &Cancel) -> Result<Vec<crate::Model>, Error> {
            Ok(vec![])
        }
    }

    fn answer(code: &str) -> String {
        serde_json::json!({ "reply": "ok", "action": "create", "name": "T",
            "files": [{ "path": "init.luau", "content": code }], "delete": [] })
        .to_string()
    }

    fn turn() -> Turn {
        Turn { root: std::env::temp_dir().join("llm-turn-none"), current: None, history: vec![], text: "x".into(), capture: None }
    }

    fn go(answers: Vec<String>) -> (Result<Outcome, Error>, Vec<String>) {
        let f = Fake(Mutex::new(answers), Mutex::default());
        let r = run(&turn(), &f, &Cancel::default(), &|_| {});
        (r, f.1.into_inner().unwrap())
    }

    const GOOD: &str = r#"return Patch.new("T", { chain = Chain.new({ Blocks.Gain.new("g", { db = -3 }) }) })"#;
    const BAD: &str = r#"return Patch.new("T", { chain = Chain.new({ Blocks.Gain.new("g", { dbb = -3 }) }) })"#;

    #[test]
    fn repairs_with_the_loader_error() {
        let (r, asked) = go(vec![answer(BAD), format!("Here:\n```json\n{}\n```", answer(GOOD))]);
        let p = r.unwrap().proposal.unwrap();
        assert!(p.is_new && p.folder == "t");
        assert!(asked[1].contains("dbb"), "{}", asked[1]);
        assert!(!asked[1].contains("staging"), "leaks a path: {}", asked[1]);
    }

    #[test]
    fn gives_up_and_rejects_escapes() {
        assert!(matches!(go(vec![answer(BAD); 3]).0, Err(Error::Failed(_))));
        let (r, asked) = go(vec![answer(GOOD).replace("init.luau", "../init.luau"), answer(GOOD)]);
        assert!(r.is_ok() && asked[1].contains("../init.luau"));
    }

    #[test]
    fn edit_needs_a_patch() {
        let (r, asked) = go(vec![answer(GOOD).replace("create", "edit"), answer(GOOD)]);
        assert!(r.is_ok() && asked[1].contains("no patch is loaded"));
    }
}
