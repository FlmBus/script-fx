//! Native file dialogs (snapshot import/export) without stalling the editor: on Linux (XDG
//! portal) and Windows the dialog runs on its own thread and the shell polls for the answer; on
//! macOS AppKit wants the main thread, which is the editor's, so it runs modally there.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// What the user picked.
#[derive(Debug)]
pub enum Picked {
    Import(PathBuf),
    Export(PathBuf),
}

type Slot = Arc<Mutex<Option<Option<Picked>>>>;

#[derive(Default)]
pub struct Dialogs {
    /// A dialog is open; filled with its answer (`None` = cancelled) when it closes.
    pending: Option<Slot>,
}

impl Dialogs {
    /// "Open" for a snapshot file. Ignored while another dialog is open.
    pub fn import(&mut self) {
        self.run(|| {
            rfd::FileDialog::new().set_title("Import a snapshot").add_filter("ScriptFX snapshot", &["json"]).pick_file().map(Picked::Import)
        });
    }

    /// "Save as" for the current sound, suggesting `name`.json.
    pub fn export(&mut self, name: &str) {
        let file = format!("{name}.json");
        self.run(move || {
            rfd::FileDialog::new()
                .set_title("Export the snapshot")
                .add_filter("ScriptFX snapshot", &["json"])
                .set_file_name(file)
                .save_file()
                .map(Picked::Export)
        });
    }

    /// The answer, once (call per frame).
    pub fn poll(&mut self) -> Option<Picked> {
        let answer = self.pending.as_ref()?.lock().unwrap().take()?;
        self.pending = None;
        answer
    }

    fn run(&mut self, dialog: impl FnOnce() -> Option<Picked> + Send + 'static) {
        if self.pending.is_some() {
            return;
        }
        let slot: Slot = Arc::default();
        self.pending = Some(slot.clone());
        #[cfg(target_os = "macos")]
        {
            *slot.lock().unwrap() = Some(dialog());
        }
        #[cfg(not(target_os = "macos"))]
        std::thread::spawn(move || *slot.lock().unwrap() = Some(dialog()));
    }
}
