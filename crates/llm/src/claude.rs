//! The installed Claude Code CLI as a plain completion: no tools, no settings, no MCP servers,
//! no saved session. The prompt goes through stdin (Windows limits command lines to 32 KB).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::{Backend, Cancel, Error, Model, Request, Role, failed};

const TIMEOUT: Duration = Duration::from_secs(600);

pub struct ClaudeCli {
    pub bin: PathBuf,
    /// Empty = the CLI's default.
    pub model: String,
    /// `CLAUDE_CONFIG_DIR`, for hosts started without the user's shell environment.
    pub config_dir: Option<PathBuf>,
}

/// On PATH, else where the installers put it (a DAW started from the desktop has a short PATH).
pub fn find_claude() -> Option<PathBuf> {
    let exe = if cfg!(windows) { "claude.exe" } else { "claude" };
    let path = std::env::var_os("PATH").unwrap_or_default();
    let home = dirs::home_dir().unwrap_or_default();
    std::env::split_paths(&path)
        .filter(|d| d.is_absolute())
        .chain([".local/bin", ".claude/local", ".local/share/mise/shims", ".local/share/mise/installs/claude/latest"].map(|d| home.join(d)))
        .chain(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from))
        .map(|d| d.join(exe))
        .find(|p| p.is_file())
}

impl ClaudeCli {
    fn command(&self) -> Command {
        let mut c = Command::new(&self.bin);
        // Don't look like a nested session when the host was started from Claude Code.
        for k in ["CLAUDECODE", "CLAUDE_CODE_ENTRYPOINT"] {
            c.env_remove(k);
        }
        if let Some(dir) = &self.config_dir {
            c.env("CLAUDE_CONFIG_DIR", dir);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        c
    }

    /// `claude -p` without tools, settings, MCP servers or a saved session.
    fn isolated(&self) -> Result<Command, Error> {
        let dir = crate::data_dir("llm-cwd");
        std::fs::create_dir_all(&dir).map_err(|e| failed(format!("creating a working folder: {e}")))?;
        let mut c = self.command();
        c.current_dir(&dir)
            .args(["-p", "--tools", "", "--setting-sources", "", "--strict-mcp-config"])
            .args(["--disable-slash-commands", "--no-session-persistence"]);
        Ok(c)
    }

    pub fn logged_in(&self, cancel: &Cancel) -> bool {
        let out = run(self.command().args(["auth", "status"]), String::new(), cancel, Duration::from_secs(15));
        let Ok((_, stdout, _)) = out else { return false };
        serde_json::from_str::<Value>(&stdout).is_ok_and(|v| v["loggedIn"] == true)
    }
}

impl Backend for ClaudeCli {
    fn complete(&self, req: &Request, cancel: &Cancel) -> Result<String, Error> {
        let mut c = self.isolated()?;
        c.args(["--output-format", "json", "--system-prompt"])
            .arg("You write audio effect patches for the ScriptFX plugin. The input holds your instructions, the API reference and the conversation.")
            .arg("--json-schema")
            .arg(req.schema.to_string());
        if !self.model.is_empty() {
            c.args(["--model", &self.model]);
        }
        let (ok, stdout, stderr) = run(&mut c, transcript(req), cancel, TIMEOUT)?;
        parse(ok, &stdout, &stderr)
    }

    /// The SDK protocol's handshake lists the login's models; nothing is asked of a model.
    fn models(&self, cancel: &Cancel) -> Result<Vec<Model>, Error> {
        let mut c = self.isolated()?;
        c.args(["--input-format", "stream-json", "--output-format", "stream-json", "--verbose"]);
        let hello = "{\"type\":\"control_request\",\"request_id\":\"1\",\"request\":{\"subtype\":\"initialize\"}}\n";
        let (_, stdout, _) = run(&mut c, hello.into(), cancel, Duration::from_secs(30))?;
        parse_models(&stdout).ok_or_else(|| failed("this Claude Code version doesn't list its models"))
    }
}

fn parse_models(stdout: &str) -> Option<Vec<Model>> {
    let lines = stdout.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok());
    let models: Vec<Model> = lines
        .filter(|v| v["type"] == "control_response")
        .filter_map(|v| v["response"]["response"]["models"].as_array().cloned())
        .flatten()
        .filter_map(|m| {
            let id = m["value"].as_str()?;
            let name = m["displayName"].as_str().unwrap_or(id).to_string();
            if id != "default" {
                return Some(Model { id: id.into(), name });
            }
            let what = m["description"].as_str().filter(|d| !d.is_empty());
            Some(Model { id: String::new(), name: what.map_or("Default".into(), |d| format!("Default ({d})")) })
        })
        .collect();
    (!models.is_empty()).then_some(models)
}

fn transcript(req: &Request) -> String {
    let mut s = format!("{}\n\n# Conversation\n", req.system);
    for m in &req.messages {
        let who = match m.role {
            Role::User => "User",
            Role::Assistant => "You",
        };
        s += &format!("\n## {who}\n\n{}\n", m.text);
    }
    s + "\nAnswer the last user message."
}

fn parse(ok: bool, stdout: &str, stderr: &str) -> Result<String, Error> {
    let Ok(v) = serde_json::from_str::<Value>(stdout) else {
        let text = if stderr.trim().is_empty() { stdout } else { stderr };
        return Err(failed(format!("claude: {}", crate::http::short(text))));
    };
    let result = v["result"].as_str().unwrap_or("");
    if !ok || v["is_error"] == true {
        let why = if result.is_empty() { v["subtype"].as_str().unwrap_or("no answer") } else { result };
        return Err(failed(format!("claude: {}", crate::http::short(why))));
    }
    Ok(if v["structured_output"].is_object() { v["structured_output"].to_string() } else { result.to_string() })
}

/// Runs a process to its end, killing it on cancel or timeout.
pub(crate) fn run(c: &mut Command, input: String, cancel: &Cancel, timeout: Duration) -> Result<(bool, String, String), Error> {
    let name = Path::new(c.get_program()).file_name().unwrap_or_default().to_string_lossy().to_string();
    let mut child = c
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| failed(format!("couldn't start {name}: {e}")))?;
    let (mut stdin, stdout, stderr) = (child.stdin.take().unwrap(), child.stdout.take().unwrap(), child.stderr.take().unwrap());
    // Each pipe on its own thread: nobody blocks on a full one.
    let writer = std::thread::spawn(move || drop(stdin.write_all(input.as_bytes())));
    let (out, err) = (read(stdout), read(stderr));
    let start = Instant::now();
    let status = loop {
        let stop = if cancel.is_cancelled() {
            Some(Error::Cancelled)
        } else if start.elapsed() > timeout {
            Some(failed(format!("{name} took longer than {} s", timeout.as_secs())))
        } else {
            None
        };
        if let Some(e) = stop {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(30)),
            Err(e) => return Err(failed(format!("{name}: {e}"))),
        }
    };
    let _ = writer.join();
    Ok((status.success(), out.join().unwrap_or_default(), err.join().unwrap_or_default()))
}

fn read(mut r: impl Read + Send + 'static) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = r.read_to_end(&mut b);
        String::from_utf8_lossy(&b).into_owned()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_and_errors() {
        assert_eq!(parse(true, r#"{"is_error":false,"structured_output":{"reply":"hi"}}"#, "").unwrap(), r#"{"reply":"hi"}"#);
        assert_eq!(parse(false, r#"{"is_error":true,"result":"Not logged in"}"#, ""), Err(failed("claude: Not logged in")));
        assert_eq!(parse(false, "", "boom"), Err(failed("claude: boom")));
    }

    #[test]
    fn lists_models() {
        let out = r#"{"type":"system"}
{"type":"control_response","response":{"response":{"models":[{"value":"default","displayName":"Default (recommended)","description":"X 1"},{"value":"x","displayName":"X 2"}]}}}"#;
        let m = |id: &str, name: &str| Model { id: id.into(), name: name.into() };
        assert_eq!(parse_models(out), Some(vec![m("", "Default (X 1)"), m("x", "X 2")]));
        assert_eq!(parse_models("nope"), None);
    }

    #[cfg(unix)]
    #[test]
    fn cancel_kills_the_process() {
        let cancel = Cancel::default();
        cancel.cancel();
        let t = Instant::now();
        assert_eq!(run(Command::new("sleep").arg("20"), String::new(), &cancel, TIMEOUT), Err(Error::Cancelled));
        assert!(t.elapsed() < Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn large_input_round_trips() {
        let input = "x".repeat(1 << 20);
        let (ok, out, _) = run(&mut Command::new("cat"), input.clone(), &Cancel::default(), TIMEOUT).unwrap();
        assert!(ok && out == input);
    }
}
