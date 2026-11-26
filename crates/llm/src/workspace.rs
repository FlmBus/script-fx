//! Patch files. The model only ever touches `.luau` files inside one patch folder; changes are
//! staged and checked in a copy first, and every commit keeps a backup in
//! `<data dir>/ScriptFX/history/`.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

const MAX_FILE: usize = 256 * 1024;
const MAX_FILES: usize = 40;
const MAX_DEPTH: usize = 4;

/// A change to one patch that passed the checks but isn't written yet.
#[derive(Debug, Clone, PartialEq)]
pub struct Proposal {
    pub folder: String,
    pub is_new: bool,
    /// Relative path (`/`) → content; `None` = delete.
    pub files: BTreeMap<String, Option<String>>,
}

/// A committed change, for undo.
#[derive(Debug, Clone)]
pub struct Applied {
    pub folder: String,
    pub is_new: bool,
    /// What each touched file held before.
    before: BTreeMap<String, Option<String>>,
    /// What we wrote: undo refuses if the user has changed it since.
    after: BTreeMap<String, Option<String>>,
}

/// `init.luau`, `ui/knob.luau`: plain names, `/`-separated, no dot files, no drive letters.
pub fn valid_path(rel: &str) -> bool {
    let parts: Vec<&str> = rel.split('/').collect();
    let plain = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    let (Some(file), dirs) = (parts.last(), &parts[..parts.len() - 1]) else { return false };
    parts.len() <= MAX_DEPTH && file.strip_suffix(".luau").is_some_and(plain) && dirs.iter().all(|d| plain(d))
}

pub fn valid_files(files: &BTreeMap<String, Option<String>>) -> Result<(), String> {
    if files.len() > MAX_FILES {
        return Err(format!("too many files (max {MAX_FILES})"));
    }
    for (path, content) in files {
        if !valid_path(path) {
            return Err(format!("'{path}': only .luau files inside the patch folder, e.g. init.luau or ui/knob.luau"));
        }
        if content.as_ref().is_some_and(|c| c.len() > MAX_FILE) {
            return Err(format!("'{path}' is too large"));
        }
    }
    Ok(())
}

/// A folder name for a new patch that isn't taken: "Warm Bass!" → `warm-bass`, `warm-bass-2`.
pub fn new_folder(root: &Path, name: &str) -> String {
    let mut slug = String::new();
    for c in name.chars().take(40) {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = match slug.trim_end_matches('-') {
        "" => "new-patch",
        s => s,
    };
    let taken = |s: &str| root.join(s).symlink_metadata().is_ok();
    (1..).map(|i| if i == 1 { slug.to_string() } else { format!("{slug}-{i}") }).find(|s| !taken(s)).unwrap()
}

/// Every `.luau` file of a patch (no symlinks).
pub fn read_luau(dir: &Path) -> BTreeMap<String, String> {
    fn walk(dir: &Path, prefix: &str, depth: usize, out: &mut BTreeMap<String, String>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let (Ok(kind), name) = (e.file_type(), e.file_name().to_string_lossy().into_owned()) else { continue };
            if kind.is_dir() && depth < MAX_DEPTH {
                walk(&e.path(), &format!("{prefix}{name}/"), depth + 1, out);
            } else if kind.is_file() && name.ends_with(".luau") {
                if let Ok(text) = std::fs::read_to_string(e.path()) {
                    out.insert(format!("{prefix}{name}"), text);
                }
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, "", 1, &mut out);
    out
}

/// A copy of a patch with changes on top; removed when dropped.
pub struct Staged {
    base: PathBuf,
    pub dir: PathBuf,
}

impl Staged {
    pub fn new(root: &Path, folder: &str, is_new: bool, files: &BTreeMap<String, Option<String>>) -> io::Result<Self> {
        let base = crate::data_dir("staging").join(unique());
        let staged = Staged { dir: base.join(folder), base };
        std::fs::create_dir_all(&staged.dir)?;
        if !is_new {
            copy_dir(&root.join(folder), &staged.dir, 0)?;
        }
        write_all(&staged.dir, files)?;
        Ok(staged)
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

impl Proposal {
    /// Writes the change (fast: call it on the UI thread, so hot reload never sees half of it).
    pub fn commit(&self, root: &Path) -> io::Result<Applied> {
        let dir = root.join(&self.folder);
        if self.is_new {
            std::fs::create_dir(&dir)?; // fails if somebody took the name meanwhile
        } else {
            copy_dir(&dir, &history(&self.folder), 0)?;
        }
        let before = current(&dir, self.files.keys())?;
        if let Err(e) = write_all(&dir, &self.files) {
            let _ = write_all(&dir, &before);
            if self.is_new {
                let _ = std::fs::remove_dir_all(&dir);
            }
            return Err(e);
        }
        Ok(Applied { folder: self.folder.clone(), is_new: self.is_new, before, after: self.files.clone() })
    }
}

impl Applied {
    /// Puts the files back; a new patch moves to the history folder.
    pub fn undo(&self, root: &Path) -> io::Result<()> {
        let dir = root.join(&self.folder);
        if current(&dir, self.after.keys())? != self.after {
            return Err(io::Error::other("the patch was changed since"));
        }
        if self.is_new {
            // Copy, then remove: the history may be on another file system.
            copy_dir(&dir, &history(&self.folder), 0)?;
            return std::fs::remove_dir_all(&dir);
        }
        write_all(&dir, &self.before)
    }
}

fn current<'a>(dir: &Path, paths: impl Iterator<Item = &'a String>) -> io::Result<BTreeMap<String, Option<String>>> {
    paths
        .map(|p| {
            let text = match std::fs::read_to_string(dir.join(p)) {
                Ok(t) => Some(t),
                Err(e) if e.kind() == io::ErrorKind::NotFound => None,
                Err(e) => return Err(e),
            };
            Ok((p.clone(), text))
        })
        .collect()
}

fn write_all(dir: &Path, files: &BTreeMap<String, Option<String>>) -> io::Result<()> {
    let base = dir.canonicalize()?;
    for (rel, content) in files {
        let path = base.join(rel);
        let parent = path.parent().unwrap_or(&base);
        // A symlinked subfolder must not lead out of the patch. Check before creating anything in it.
        let mut existing = parent;
        while !existing.exists() {
            existing = existing.parent().unwrap_or(&base);
        }
        if !existing.canonicalize()?.starts_with(&base) {
            return Err(io::Error::other(format!("'{rel}' is outside the patch folder")));
        }
        match content {
            Some(text) => {
                std::fs::create_dir_all(parent)?;
                let tmp = path.with_extension("luau.tmp");
                std::fs::write(&tmp, text)?;
                std::fs::rename(&tmp, &path)?;
            }
            None => match std::fs::remove_file(&path) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                _ => {}
            },
        }
    }
    Ok(())
}

/// Regular files and folders only (no symlinks).
fn copy_dir(from: &Path, to: &Path, depth: usize) -> io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let kind = e.file_type()?;
        if kind.is_dir() && depth < 8 {
            copy_dir(&e.path(), &to.join(e.file_name()), depth + 1)?;
        } else if kind.is_file() {
            std::fs::copy(e.path(), to.join(e.file_name()))?;
        }
    }
    Ok(())
}

fn history(folder: &str) -> PathBuf {
    crate::data_dir("history").join(format!("{folder}-{}", unique()))
}

fn unique() -> String {
    static N: AtomicU32 = AtomicU32::new(0);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    format!("{t}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        let p = std::env::temp_dir().join(format!("llm-test-{}", unique()));
        std::fs::create_dir_all(p.join("a/ui")).unwrap();
        std::fs::write(p.join("a/init.luau"), "old").unwrap();
        p
    }

    fn files(list: &[(&str, Option<&str>)]) -> BTreeMap<String, Option<String>> {
        list.iter().map(|(p, c)| (p.to_string(), c.map(String::from))).collect()
    }

    #[test]
    fn paths() {
        for ok in ["init.luau", "ui/knob.luau", "ui/a-b_c.luau"] {
            assert!(valid_path(ok), "{ok}");
        }
        for bad in ["", "../x.luau", "/x.luau", "a//b.luau", "x.lua", ".luau", "C:/x.luau", "a\\b.luau", "ui/../../x.luau", ".git/x.luau", "a/b/c/d/e.luau"] {
            assert!(!valid_path(bad), "{bad}");
        }
    }

    #[test]
    fn folder_names() {
        let r = root();
        assert_eq!(new_folder(&r, "Warm Bass!"), "warm-bass");
        assert_eq!(new_folder(&r, "A"), "a-2");
        assert_eq!(new_folder(&r, "../.."), "new-patch");
    }

    #[test]
    fn commit_and_undo() {
        let r = root();
        let p = Proposal { folder: "a".into(), is_new: false, files: files(&[("init.luau", Some("new")), ("ui/x.luau", Some("x"))]) };
        let applied = p.commit(&r).unwrap();
        assert_eq!(std::fs::read_to_string(r.join("a/init.luau")).unwrap(), "new");
        std::fs::write(r.join("a/init.luau"), "mine").unwrap();
        assert!(applied.undo(&r).is_err(), "keeps the user's edit");
        std::fs::write(r.join("a/init.luau"), "new").unwrap();
        applied.undo(&r).unwrap();
        assert_eq!(std::fs::read_to_string(r.join("a/init.luau")).unwrap(), "old");
        assert!(!r.join("a/ui/x.luau").exists());

        let p = Proposal { folder: "b".into(), is_new: true, files: files(&[("init.luau", Some("b"))]) };
        let applied = p.commit(&r).unwrap();
        assert!(p.commit(&r).is_err(), "never overwrites");
        applied.undo(&r).unwrap();
        assert!(!r.join("b").exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_dont_lead_out() {
        let (r, outside) = (root(), root());
        std::os::unix::fs::symlink(&outside, r.join("a/link")).unwrap();
        let p = Proposal { folder: "a".into(), is_new: false, files: files(&[("link/x.luau", Some("x"))]) };
        assert!(p.commit(&r).is_err());
        assert!(!outside.join("x.luau").exists());
    }
}
