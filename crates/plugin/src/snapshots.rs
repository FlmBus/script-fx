//! Snapshots: saved sounds of a patch. A snapshot holds everything the project saves for the
//! patch: the macros, every param's value, the meta knobs' settings and the user's routing edits
//! (added/removed routes, modulator settings). "Init" is the patch's defaults and has no file.
//!
//! Stored per user, apart from the patch code: `<data dir>/ScriptFX/snapshots/<patch folder>/<name>.json`
//! (`SCRIPTFX_SNAPSHOTS` overrides the root). The file name is the snapshot's name. Values are
//! keyed by id, so a snapshot survives params and macros being added, removed or reordered; what
//! it doesn't name gets the patch's default on recall.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use patch::{ModEdits, ModSettings, Patch};
use serde::{Deserialize, Serialize};

use crate::FxParams;
use nice_plug::prelude::Param;

/// The patch's defaults; always there, never a file.
pub const INIT: &str = "Init";
const FORMAT: u32 = 1;
const MAX_NAME: usize = 48;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Snapshot {
    pub format: u32,
    /// The patch folder it was saved from (an import for another patch is refused).
    pub patch: String,
    /// Normalized 0..1, by macro id.
    pub macros: BTreeMap<String, f32>,
    /// Plain values, by param id.
    pub params: BTreeMap<String, f32>,
    /// Meta-knob settings that differ from the patch's, by `Patch::mod_key`.
    pub mod_slots: BTreeMap<String, ModSettings>,
    pub mod_edits: ModEdits,
}

impl Snapshot {
    /// The current sound of `patch` (loaded, with the project's edits applied; `free` = its
    /// params' plain values).
    pub fn capture(folder: &str, params: &FxParams, patch: &Patch, free: &[f32]) -> Self {
        let macros = (0..patch.macros.len())
            .map(|i| (patch.macros[i].id.clone(), params.macros[i].value.unmodulated_normalized_value()))
            .collect();
        let values = patch.params.iter().zip(free).map(|(p, v)| (p.id.clone(), *v)).collect();
        let stored = params.mod_settings.read().unwrap();
        Self {
            format: FORMAT,
            patch: folder.to_string(),
            macros,
            params: values,
            mod_slots: changed_slots(patch, &stored),
            mod_edits: params.mod_edits.read().unwrap().clone(),
        }
    }

    /// What this snapshot sounds like on `patch`: every macro and param named (defaults for the
    /// ones it doesn't know), the ones `patch` doesn't have dropped. Comparable with [`capture`].
    pub fn resolve(&self, folder: &str, patch: &Patch) -> Self {
        let macros = patch
            .macros
            .iter()
            .enumerate()
            .map(|(i, m)| (m.id.clone(), self.macros.get(&m.id).copied().unwrap_or_else(|| patch.macro_default(i))))
            .collect();
        let values = patch
            .params
            .iter()
            .map(|p| (p.id.clone(), self.params.get(&p.id).copied().map_or_else(|| p.default_plain(), |v| p.snap(v))))
            .collect();
        let slots = self.mod_slots.iter().map(|(k, v)| (k.clone(), *v)).collect();
        Self {
            format: FORMAT,
            patch: folder.to_string(),
            macros,
            params: values,
            mod_slots: changed_slots(patch, &slots),
            mod_edits: self.mod_edits.clone(),
        }
    }

    /// Same sound, give or take float noise (values that went through a knob and back).
    pub fn same_sound(&self, other: &Self) -> bool {
        let close = |a: &BTreeMap<String, f32>, b: &BTreeMap<String, f32>| {
            a.len() == b.len()
                && a.iter().all(|(k, x)| b.get(k).is_some_and(|y| (x - y).abs() <= 1e-4 * x.abs().max(y.abs()).max(1.0)))
        };
        close(&self.macros, &other.macros)
            && close(&self.params, &other.params)
            && self.mod_slots == other.mod_slots
            && self.mod_edits == other.mod_edits
    }

    /// Puts it into the project (`FxParams`), except the macros: those are host parameters and
    /// are set by the caller (with gestures), after the patch has been reloaded.
    pub fn store(&self, params: &FxParams) {
        *params.param_values.write().unwrap() = self.params.iter().map(|(k, v)| (k.clone(), *v)).collect();
        *params.mod_settings.write().unwrap() = self.mod_slots.iter().map(|(k, v)| (k.clone(), *v)).collect();
        *params.mod_edits.write().unwrap() = self.mod_edits.clone();
    }
}

/// The meta-knob settings that differ from what the patch defines, by key. A knob turned back to
/// the patch's value counts as unchanged (saving and comparing stay stable).
fn changed_slots(patch: &Patch, stored: &std::collections::HashMap<String, ModSettings>) -> BTreeMap<String, ModSettings> {
    let mut out = BTreeMap::new();
    for i in 0..patch.modulations.len() {
        for k in 0..patch.modulations[i].len() {
            let key = patch.mod_key(i, k);
            if let Some(s) = stored.get(&key).filter(|s| **s != patch.modulations[i][k].patch) {
                out.insert(key, *s);
            }
        }
    }
    out
}

/// `<data dir>/ScriptFX/snapshots` (or `SCRIPTFX_SNAPSHOTS`).
pub fn root() -> PathBuf {
    if let Some(p) = std::env::var_os("SCRIPTFX_SNAPSHOTS") {
        return PathBuf::from(p);
    }
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("ScriptFX").join("snapshots")
}

/// A patch's snapshot folder. `None` for a folder name that isn't a plain name.
fn dir(root: &Path, folder: &str) -> Option<PathBuf> {
    let mut parts = Path::new(folder).components();
    matches!((parts.next(), parts.next()), (Some(std::path::Component::Normal(_)), None)).then(|| root.join(folder))
}

/// The patch's snapshot names, sorted (case-insensitive). Init isn't listed.
pub fn list(root: &Path, folder: &str) -> Vec<String> {
    let Some(dir) = dir(root, folder) else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            (path.extension()? == "json").then_some(())?;
            let name = path.file_stem()?.to_str()?.to_string();
            check_name(&name).ok()
        })
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names
}

/// A usable name: trimmed, not empty, not "Init", no characters a file name can't hold on any
/// platform. Returns the trimmed name or why not.
pub fn check_name(name: &str) -> Result<String, &'static str> {
    let name = name.trim();
    if name.is_empty() {
        return Err("type a name");
    }
    if name.eq_ignore_ascii_case(INIT) {
        return Err("“Init” is the patch's defaults");
    }
    if name.chars().count() > MAX_NAME {
        return Err("at most 48 characters");
    }
    if name.starts_with('.') || name.ends_with('.') || name.chars().any(|c| c.is_control() || r#"<>:"/\|?*"#.contains(c)) {
        return Err(r#"no < > : " / \ | ? * and no dot at the ends"#);
    }
    Ok(name.to_string())
}

fn path(root: &Path, folder: &str, name: &str) -> io::Result<PathBuf> {
    let name = check_name(name).map_err(io::Error::other)?;
    let dir = dir(root, folder).ok_or_else(|| io::Error::other("not a patch folder"))?;
    Ok(dir.join(format!("{name}.json")))
}

pub fn exists(root: &Path, folder: &str, name: &str) -> bool {
    // Case-insensitive: "Warm" and "warm" would be one file on macOS and Windows.
    list(root, folder).iter().any(|n| n.eq_ignore_ascii_case(name.trim()))
}

pub fn read(root: &Path, folder: &str, name: &str) -> io::Result<Snapshot> {
    read_file(&path(root, folder, name)?)
}

/// Writes atomically (a temporary file, then a rename), so a crash never leaves half a snapshot.
pub fn write(root: &Path, folder: &str, name: &str, snap: &Snapshot) -> io::Result<()> {
    let path = path(root, folder, name)?;
    std::fs::create_dir_all(path.parent().unwrap())?;
    write_file(&path, snap)
}

pub fn delete(root: &Path, folder: &str, name: &str) -> io::Result<()> {
    std::fs::remove_file(path(root, folder, name)?)
}

/// Fails if `to` exists (other than `from` in another case).
pub fn rename(root: &Path, folder: &str, from: &str, to: &str) -> io::Result<()> {
    let (a, b) = (path(root, folder, from)?, path(root, folder, to)?);
    if !from.trim().eq_ignore_ascii_case(to.trim()) && exists(root, folder, to) {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, "a snapshot with that name exists"));
    }
    std::fs::rename(a, b)
}

pub fn read_file(path: &Path) -> io::Result<Snapshot> {
    let text = std::fs::read_to_string(path)?;
    let snap: Snapshot = serde_json::from_str(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("not a snapshot: {e}")))?;
    if snap.format > FORMAT {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "made by a newer ScriptFX"));
    }
    Ok(snap)
}

pub fn write_file(path: &Path, snap: &Snapshot) -> io::Result<()> {
    let text = serde_json::to_string_pretty(snap).map_err(io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// A free name based on `base` ("Warm", "Warm 2", …), for imports.
pub fn free_name(root: &Path, folder: &str, base: &str) -> String {
    let base = check_name(base).unwrap_or_else(|_| "Imported".to_string());
    if !exists(root, folder, &base) {
        return base;
    }
    (2..).map(|n| format!("{base} {n}")).find(|n| !exists(root, folder, n)).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(name: &str) -> PathBuf {
        let r = std::env::temp_dir().join(format!("scriptfx-snapshots-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&r);
        r
    }

    fn patch(src: &str) -> Patch {
        let dir = std::env::temp_dir().join(format!("scriptfx-snap-patch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("init.luau"), src).unwrap();
        Patch::load(&dir).unwrap()
    }

    const SRC: &str = "local g = Blocks.Gain.new('g', { db = 0 })\nlocal l = Blocks.LowPass.new('l', { freq = 8000 })\n\
        return Patch.new('T', { chain = Chain.new({ g, l }), macros = { Macro.new('m', 'M', { default = 0.5 }):drives(l.params.q) } })";

    #[test]
    fn names() {
        assert_eq!(check_name("  Warm bus "), Ok("Warm bus".into()));
        assert!(check_name("init").is_err());
        assert!(check_name("").is_err());
        assert!(check_name("a/b").is_err());
        assert!(check_name("..").is_err());
        assert!(check_name(&"x".repeat(49)).is_err());
    }

    #[test]
    fn write_list_rename_delete() {
        let r = root("crud");
        let s = Snapshot { format: FORMAT, patch: "p".into(), ..Default::default() };
        write(&r, "p", "warm", &s).unwrap();
        write(&r, "p", "Bright", &s).unwrap();
        assert_eq!(list(&r, "p"), vec!["Bright", "warm"]);
        assert_eq!(read(&r, "p", "warm").unwrap(), s);
        assert!(rename(&r, "p", "warm", "BRIGHT").is_err());
        rename(&r, "p", "warm", "Warm").unwrap();
        assert_eq!(list(&r, "p"), vec!["Bright", "Warm"]);
        assert_eq!(free_name(&r, "p", "Warm"), "Warm 2");
        delete(&r, "p", "Bright").unwrap();
        assert_eq!(list(&r, "p"), vec!["Warm"]);
        assert!(list(&r, "../p").is_empty());
        assert!(write(&r, "../p", "x", &s).is_err());
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn resolve_fills_defaults_and_drops_strangers() {
        let p = patch(SRC);
        let mut s = Snapshot::default();
        s.params.insert("g.db".into(), -6.0);
        s.params.insert("gone.db".into(), 3.0);
        s.macros.insert("m".into(), 0.75);
        let r = s.resolve("t", &p);
        assert_eq!(r.params["g.db"], -6.0);
        assert_eq!(r.params["l.freq"], 8000.0);
        assert!(!r.params.contains_key("gone.db"));
        assert_eq!(r.macros["m"], 0.75);
        // Init: everything at its default.
        let init = Snapshot::default().resolve("t", &p);
        assert_eq!(init.macros["m"], 0.5);
        assert!(!init.same_sound(&r));
    }

    #[test]
    fn capture_matches_what_it_saved() {
        let p = patch(SRC);
        let params = FxParams::default();
        let s = Snapshot { macros: [("m".to_string(), 0.0)].into(), params: [("g.db".to_string(), -3.0)].into(), ..Default::default() };
        let resolved = s.resolve("t", &p);
        resolved.store(&params);
        let free = params.free_values(&p);
        // Macros are set by the shell; FxParams::default() has them at 0 like the snapshot.
        assert!(Snapshot::capture("t", &params, &p, &free).same_sound(&resolved));
    }
}
