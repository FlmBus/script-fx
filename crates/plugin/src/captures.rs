//! Captures on disk: where a Listen's audio and analyses are stored, and how they are read
//! back. The LLM is meant to *fetch* analysis data instead of carrying it in the conversation
//! (decided 2026-11-25, Flemming): the chat keeps a one-line digest per stream and the model
//! pulls the sections it needs through a tool, which also lets it compare against older
//! captures. [`read`] is the query that tool will call; it has no LLM dependency.
//!
//! Layout (`<data dir>/ScriptFX/captures/`, or `$SCRIPTFX_CAPTURES`):
//!
//! ```text
//! index.json                  every capture, newest first (rebuilt from the folders if lost)
//! capture-<unix secs>[-n]/
//!   capture.json              the manifest: when, how long, patch, tracks, digests
//!   <track slug>/
//!     analysis.json           { "input": Analysis, "output": Analysis }
//!     input.wav               f32 stereo at the host's rate
//!     output.wav
//! ```
//!
//! One folder per capture with a subfolder per track, so a multi-track Listen writes one
//! capture (and concurrent captures can't collide, which the old flat `capture-<secs>` layout
//! allowed).

// The read side ([`list`], [`manifest`], [`read`], [`Stream`], [`Section`]) exists for the
// `get_analysis` tool, which arrives with `crates/llm`. It is covered by the tests here, so it
// is written and verified before the caller exists rather than after.
#![allow(dead_code)]

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use analysis::{Analysis, Frame};
use serde::{Deserialize, Serialize};

/// How many captures to keep. Audio is the expensive part: 30 s of f32 stereo at 48 kHz is
/// ~11.5 MB per stream, so one 4-track capture can be ~92 MB. The oldest go first.
pub const KEEP: usize = 20;

/// `<data dir>/ScriptFX/captures`, or `$SCRIPTFX_CAPTURES` (tests, dev).
pub fn root() -> PathBuf {
    if let Some(p) = std::env::var_os("SCRIPTFX_CAPTURES") {
        return PathBuf::from(p);
    }
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("ScriptFX").join("captures")
}

/// One capture: what a single Listen recorded, across one or more tracks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub id: String,
    /// Seconds since the epoch, so the model can say "the capture before that one".
    pub unix_time: u64,
    /// Seconds of signal recorded (silence is skipped while recording).
    pub seconds: f32,
    pub sample_rate_hz: f32,
    /// The patch the focus track ran, if any.
    pub patch: Option<String>,
    pub tracks: Vec<Track>,
}

/// One track inside a capture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    /// Folder name inside the capture, derived from the track name.
    pub slug: String,
    /// What the host calls the track, if it told us.
    pub name: Option<String>,
    /// The instance that started the Listen (the patch being worked on).
    pub focus: bool,
    /// One line per stream for the chat and the LLM's context: enough to decide what to fetch.
    pub input_digest: String,
    pub output_digest: String,
}

/// A stream of one track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Input,
    Output,
}

impl Stream {
    pub const ALL: [Stream; 2] = [Stream::Input, Stream::Output];

    pub fn key(self) -> &'static str {
        match self {
            Stream::Input => "input",
            Stream::Output => "output",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Stream::ALL.into_iter().find(|v| v.key() == s)
    }
}

/// Which part of an analysis to read. `All` is the whole thing (~1.1 k tokens per stream);
/// the single sections are what the tool offers so the model can ask for less.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    All,
    Loudness,
    Spectrum,
    Dynamics,
    Stereo,
    Pitch,
}

impl Section {
    pub const ALL: [Section; 6] =
        [Section::All, Section::Loudness, Section::Spectrum, Section::Dynamics, Section::Stereo, Section::Pitch];

    pub fn key(self) -> &'static str {
        match self {
            Section::All => "all",
            Section::Loudness => "loudness",
            Section::Spectrum => "spectrum",
            Section::Dynamics => "dynamics",
            Section::Stereo => "stereo",
            Section::Pitch => "pitch",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Section::ALL.into_iter().find(|v| v.key() == s)
    }
}

/// What one instance recorded, ready to be written.
pub struct Recording {
    /// `None` = the host didn't name the track.
    pub name: Option<String>,
    pub focus: bool,
    pub input: Vec<Frame>,
    pub output: Vec<Frame>,
    pub input_analysis: Analysis,
    pub output_analysis: Analysis,
}

/// Writes one capture (all tracks), updates the index and prunes old captures. Returns the
/// capture's folder.
pub fn save(recordings: &[Recording], sample_rate: f32, patch: Option<String>) -> io::Result<PathBuf> {
    let root = root();
    fs::create_dir_all(&root)?;
    let unix_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // A second-resolution name can repeat (two Listens in one second, or two hosts): suffix it.
    let (id, dir) = (0..)
        .map(|n| if n == 0 { format!("capture-{unix_time}") } else { format!("capture-{unix_time}-{n}") })
        .map(|id| (id.clone(), root.join(id)))
        .find(|(_, dir)| !dir.exists())
        .expect("a free capture id");
    fs::create_dir_all(&dir)?;

    let seconds = recordings.first().map_or(0.0, |r| r.input.len() as f32 / sample_rate.max(1.0));
    let mut tracks = Vec::with_capacity(recordings.len());
    for (i, rec) in recordings.iter().enumerate() {
        let slug = slug(rec.name.as_deref(), i, &tracks);
        let track_dir = dir.join(&slug);
        fs::create_dir_all(&track_dir)?;
        let json = serde_json::json!({ "input": rec.input_analysis, "output": rec.output_analysis });
        fs::write(track_dir.join("analysis.json"), serde_json::to_vec_pretty(&json)?)?;
        write_wav(&track_dir.join("input.wav"), &rec.input, sample_rate)?;
        write_wav(&track_dir.join("output.wav"), &rec.output, sample_rate)?;
        tracks.push(Track {
            slug,
            name: rec.name.clone(),
            focus: rec.focus,
            input_digest: analysis::digest(&rec.input_analysis),
            output_digest: analysis::digest(&rec.output_analysis),
        });
    }

    let manifest = Manifest { id, unix_time, seconds, sample_rate_hz: sample_rate, patch, tracks };
    fs::write(dir.join("capture.json"), serde_json::to_vec_pretty(&manifest)?)?;
    write_index(&root, &manifest)?;
    prune(&root);
    Ok(dir)
}

/// Every capture, newest first. Reads `index.json`; if it's missing or unreadable (hand-deleted,
/// interrupted write) the folders are the truth and it's rebuilt from them.
pub fn list() -> Vec<Manifest> {
    let root = root();
    let from_index = fs::read(root.join("index.json")).ok().and_then(|b| serde_json::from_slice::<Vec<Manifest>>(&b).ok());
    let mut all = match from_index {
        Some(all) if !all.is_empty() => all,
        _ => scan(&root),
    };
    all.sort_by(|a, b| b.unix_time.cmp(&a.unix_time));
    all
}

/// What the captures folder holds right now, for the settings sheet.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub captures: usize,
    pub bytes: u64,
}

impl Usage {
    /// "12 captures · 430 MB", or "empty".
    pub fn label(self) -> String {
        if self.captures == 0 {
            return "empty".to_owned();
        }
        let n = self.captures;
        let plural = if n == 1 { "capture" } else { "captures" };
        format!("{n} {plural} · {}", size(self.bytes))
    }
}

/// Counts the captures and adds up everything in the folder (the WAVs are nearly all of it).
pub fn usage() -> Usage {
    let root = root();
    Usage { captures: scan(&root).len(), bytes: dir_size(&root) }
}

/// Deletes every capture and the index: the folder itself stays. Returns what was freed.
pub fn clear() -> io::Result<Usage> {
    let root = root();
    let freed = usage();
    if !root.is_dir() {
        return Ok(Usage::default());
    }
    for entry in fs::read_dir(&root)?.flatten() {
        let path = entry.path();
        let removed = if path.is_dir() { fs::remove_dir_all(&path) } else { fs::remove_file(&path) };
        // One stubborn file (open elsewhere, no permission) must not stop the rest.
        if let Err(e) = removed {
            nice_plug::nice_log!("clearing captures: {} ({e})", path.display());
        }
    }
    Ok(freed)
}

fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(path) else { return 0 };
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            _ => e.metadata().map(|m| m.len()).unwrap_or(0),
        })
        .sum()
}

/// Bytes as a short human-readable string (MB from a megabyte up: the unit people think in
/// for audio).
fn size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    match bytes {
        b if b >= GB => format!("{:.1} GB", b as f64 / GB as f64),
        b if b >= MB => format!("{} MB", b / MB),
        b if b >= KB => format!("{} kB", b / KB),
        b => format!("{b} B"),
    }
}

/// One capture's manifest.
pub fn manifest(id: &str) -> io::Result<Manifest> {
    let bytes = fs::read(capture_dir(id)?.join("capture.json"))?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

/// One section of one stream of one track — the query behind the planned `get_analysis` tool.
pub fn read(id: &str, track: &str, stream: Stream, section: Section) -> io::Result<serde_json::Value> {
    let path = capture_dir(id)?.join(sane(track)).join("analysis.json");
    let bytes = fs::read(path)?;
    let json: serde_json::Value = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    let stream_value = json.get(stream.key()).ok_or_else(|| io::Error::other(format!("no {} stream", stream.key())))?;
    match section {
        Section::All => Ok(stream_value.clone()),
        s => stream_value
            .get(s.key())
            .cloned()
            .ok_or_else(|| io::Error::other(format!("no section {}", s.key()))),
    }
}

/// Rejects ids and slugs that could walk out of the captures folder (they reach us from the
/// LLM, so they are untrusted input).
fn sane(name: &str) -> String {
    name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect()
}

fn capture_dir(id: &str) -> io::Result<PathBuf> {
    let dir = root().join(sane(id));
    dir.is_dir().then_some(dir).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("no capture {id}")))
}

/// `"Bass DI"` → `"bass-di"`; unnamed tracks become `track-<n>`, and duplicates get a suffix.
fn slug(name: Option<&str>, index: usize, taken: &[Track]) -> String {
    let base = name
        .map(|n| {
            n.trim()
                .to_lowercase()
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect::<String>()
                .trim_matches('-')
                .to_owned()
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("track-{}", index + 1));
    let base = sane(&base);
    (0..)
        .map(|n| if n == 0 { base.clone() } else { format!("{base}-{n}") })
        .find(|cand| !taken.iter().any(|t| t.slug == *cand))
        .expect("a free slug")
}

fn write_wav(path: &Path, frames: &[Frame], sample_rate: f32) -> io::Result<()> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: sample_rate as u32,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(path, spec).map_err(io::Error::other)?;
    for f in frames {
        w.write_sample(f[0]).map_err(io::Error::other)?;
        w.write_sample(f[1]).map_err(io::Error::other)?;
    }
    w.finalize().map_err(io::Error::other)
}

/// Prepends the new capture to the index (rebuilding it if it was lost).
fn write_index(root: &Path, manifest: &Manifest) -> io::Result<()> {
    let mut all = fs::read(root.join("index.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<Vec<Manifest>>(&b).ok())
        .unwrap_or_else(|| scan(root));
    all.retain(|m| m.id != manifest.id);
    all.insert(0, manifest.clone());
    all.sort_by(|a, b| b.unix_time.cmp(&a.unix_time));
    fs::write(root.join("index.json"), serde_json::to_vec_pretty(&all)?)
}

/// Every `capture.json` under `root`.
fn scan(root: &Path) -> Vec<Manifest> {
    let Ok(entries) = fs::read_dir(root) else { return Vec::new() };
    entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| fs::read(e.path().join("capture.json")).ok())
        .filter_map(|b| serde_json::from_slice::<Manifest>(&b).ok())
        .collect()
}

/// Deletes the oldest captures beyond [`KEEP`], and rewrites the index.
fn prune(root: &Path) {
    let mut all = scan(root);
    all.sort_by(|a, b| b.unix_time.cmp(&a.unix_time));
    for old in all.iter().skip(KEEP) {
        let _ = fs::remove_dir_all(root.join(sane(&old.id)));
    }
    all.truncate(KEEP);
    let _ = fs::write(root.join("index.json"), serde_json::to_vec_pretty(&all).unwrap_or_default());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unique captures root per test (the env var is process-wide, so tests that use it
    /// must not run in parallel with each other — hence one test for the whole round trip).
    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("scriptfx-captures-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        unsafe { std::env::set_var("SCRIPTFX_CAPTURES", &dir) };
        dir
    }

    fn recording(name: Option<&str>, focus: bool) -> Recording {
        let frames: Vec<Frame> = (0..4800).map(|i| { let v = (i as f32 * 0.01).sin() * 0.5; [v, v] }).collect();
        let a = analysis::analyze(&frames, 48_000.0);
        Recording {
            name: name.map(str::to_owned),
            focus,
            input: frames.clone(),
            output: frames,
            input_analysis: a.clone(),
            output_analysis: a,
        }
    }

    #[test]
    fn saves_reads_and_prunes() {
        let root = temp_root("roundtrip");

        // Two tracks in one capture: the focus track and an unnamed one.
        let dir = save(&[recording(Some("Bass DI"), true), recording(None, false)], 48_000.0, Some("tape-glow".into())).unwrap();
        assert!(dir.join("bass-di/analysis.json").is_file(), "named track → slug folder");
        assert!(dir.join("bass-di/input.wav").is_file() && dir.join("bass-di/output.wav").is_file());
        assert!(dir.join("track-2/analysis.json").is_file(), "unnamed track → track-<n>");

        let all = list();
        assert_eq!(all.len(), 1);
        let m = &all[0];
        assert_eq!(m.patch.as_deref(), Some("tape-glow"));
        assert_eq!(m.tracks.len(), 2);
        assert!(m.tracks[0].focus && !m.tracks[1].focus);
        assert!(!m.tracks[0].output_digest.is_empty(), "a digest for the context");
        assert_eq!(manifest(&m.id).unwrap().id, m.id);

        // Sections: the tool's query path.
        let whole = read(&m.id, "bass-di", Stream::Output, Section::All).unwrap();
        assert!(whole.get("spectrum").is_some() && whole.get("loudness").is_some());
        let spectrum = read(&m.id, "bass-di", Stream::Input, Section::Spectrum).unwrap();
        assert!(spectrum.get("third_octave").is_some(), "the band curve is kept");
        assert!(spectrum.get("loudness").is_none(), "one section only");
        assert!(read(&m.id, "bass-di", Stream::Output, Section::Pitch).unwrap().get("pitched_percent").is_some());

        // Unknown things are errors, not panics; traversal can't leave the folder.
        assert!(read("nope", "bass-di", Stream::Output, Section::All).is_err());
        assert!(read(&m.id, "../..", Stream::Output, Section::All).is_err());
        assert!(Section::parse("spectrum") == Some(Section::Spectrum) && Section::parse("nope").is_none());
        assert!(Stream::parse("input") == Some(Stream::Input));

        // A second capture in the same second gets its own folder.
        let second = save(&[recording(Some("Bass DI"), true)], 48_000.0, None).unwrap();
        assert_ne!(second, dir);
        assert_eq!(list().len(), 2);

        // Retention: KEEP captures survive, the oldest are deleted.
        for _ in 0..KEEP {
            save(&[recording(Some("x"), true)], 48_000.0, None).unwrap();
        }
        let kept = list();
        assert_eq!(kept.len(), KEEP, "pruned to KEEP");
        assert!(fs::read_dir(&root).unwrap().flatten().filter(|e| e.path().is_dir()).count() == KEEP, "folders too");

        // The index is only a cache: without it the folders still answer.
        fs::remove_file(root.join("index.json")).unwrap();
        assert_eq!(list().len(), KEEP, "rebuilt from the folders");

        // What the settings sheet shows, and the button behind it.
        let before = usage();
        assert_eq!(before.captures, KEEP);
        assert!(before.bytes > 0 && before.label().contains(&format!("{KEEP} captures")));
        let freed = clear().unwrap();
        assert_eq!(freed.captures, KEEP);
        assert_eq!(usage(), Usage::default(), "nothing left");
        assert_eq!(usage().label(), "empty");
        assert!(list().is_empty());
        assert!(root.is_dir(), "the folder itself stays");
        clear().unwrap();
    }

    #[test]
    fn sizes_read_as_people_expect() {
        assert_eq!(size(512), "512 B");
        assert_eq!(size(2 * 1024), "2 kB");
        assert_eq!(size(11_534_336), "11 MB");
        assert_eq!(size(2_147_483_648), "2.0 GB");
    }
}
