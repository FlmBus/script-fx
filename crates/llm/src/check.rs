//! Is a patch good? Everything here feeds back to the model, so messages carry file and line but
//! no absolute paths (they'd leak the user's name to the provider).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use patch::{MACRO_SLOTS, Patch};
use ui_components::{Bound, Inputs, Ui};

use crate::{Cancel, workspace};

const DEFS: &str = include_str!("../../../patches/.types/scriptfx.d.luau");
const LUAURC: &str = include_str!("../../../patches/.luaurc");
const SR: f32 = 48_000.0;

pub struct Problems {
    /// It doesn't run: never apply it.
    pub errors: Vec<String>,
    /// Type errors: worth a repair, but it runs.
    pub types: Vec<String>,
}

/// Checks the patch in `dir`.
pub fn check(dir: &Path, cancel: &Cancel) -> Problems {
    let mut p = Problems { errors: Vec::new(), types: Vec::new() };
    match Patch::load(dir) {
        Err(e) => p.errors.push(e.to_string()),
        Ok(patch) => {
            if patch.has_panel {
                p.errors.extend(panel(dir));
            }
            p.errors.extend(sound(&patch));
        }
    }
    p.types = types(dir, cancel);
    let mut prefixes = vec![format!("{}/", dir.display())];
    if let Ok(real) = dir.canonicalize() {
        prefixes.push(format!("{}/", real.display()));
        prefixes.push(real.display().to_string());
    }
    prefixes.push(dir.display().to_string());
    for s in p.errors.iter_mut().chain(&mut p.types) {
        for prefix in &prefixes {
            *s = s.replace(prefix.as_str(), "");
        }
    }
    p
}

fn panel(dir: &Path) -> Vec<String> {
    let mut ui = match Ui::load(dir) {
        Ok(ui) => ui,
        Err(e) => return vec![format!("panel: {e}")],
    };
    let mut out = Vec::new();
    for v in [0.0, 0.5, 1.0] {
        let bound = |_: &str| Some(Bound { value: v, display: format!("{v:.1}"), locked: false });
        ui.update(
            &Inputs { time: v, hover: None, active: None, bound: &bound, meter: &|_| Some(v), series: &|_| None },
            1.0,
        );
        for e in ui.errors() {
            let e = format!("panel: {e}");
            if !out.contains(&e) {
                out.push(e);
            }
        }
    }
    out
}

/// Runs noise through the chain: it must stay finite and not blow up.
fn sound(patch: &Patch) -> Vec<String> {
    let run = || {
        let defaults: [f32; MACRO_SLOTS] = std::array::from_fn(|i| if i < patch.macros.len() { patch.macro_default(i) } else { 0.0 });
        for (name, macros, limit) in [
            ("at its defaults", defaults, 8.0f32), // +18 dBFS
            ("with all macros at 0", [0.0; MACRO_SLOTS], 64.0),
            ("with all macros at 1", [1.0; MACRO_SLOTS], 64.0),
        ] {
            let mut graph = dsp::Graph::build(patch, SR, &patch.default_param_values());
            let mut seed = 1u32;
            let mut noise = || {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed as f32 / u32::MAX as f32 - 0.5) * 0.5
            };
            let mut peak = 0.0f32;
            for _ in 0..(SR as usize / dsp::SUB_BLOCK) {
                let mut frames = [[0.0f32; 2]; dsp::SUB_BLOCK];
                frames.iter_mut().for_each(|f| *f = [noise(), noise()]);
                graph.process(&macros, &mut frames);
                for x in frames.iter().flatten() {
                    if !x.is_finite() {
                        return vec![format!("the chain outputs invalid samples (NaN/inf) {name}")];
                    }
                    peak = peak.max(x.abs());
                }
            }
            if peak > limit {
                return vec![format!("the output is dangerously loud {name}: {:+.0} dBFS peak for -12 dBFS noise", 20.0 * peak.log10())];
            }
        }
        Vec::new()
    };
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)).unwrap_or_else(|_| vec!["the chain can't be built".into()])
}

fn types(dir: &Path, cancel: &Cancel) -> Vec<String> {
    let Some(bin) = luau_lsp() else { return Vec::new() };
    // Our own copy of the definitions and strict mode, next to the patch copy being checked.
    let Some(name) = dir.file_name().and_then(|n| n.to_str()) else { return Vec::new() };
    let copy = match workspace::Staged::new(dir.parent().unwrap_or(dir), name, false, &Default::default()) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let Some(base) = copy.dir.parent() else { return Vec::new() };
    if std::fs::write(base.join("scriptfx.d.luau"), DEFS).and(std::fs::write(base.join(".luaurc"), LUAURC)).is_err() {
        return Vec::new();
    }
    let files = workspace::read_luau(&copy.dir);
    let mut c = Command::new(bin);
    c.current_dir(base)
        .args(["analyze", "--platform=standard", "--defs=@scriptfx=scriptfx.d.luau"])
        .args(files.keys().map(|f| format!("{name}/{f}")));
    let Ok((ok, out, err)) = crate::claude::run(&mut c, String::new(), cancel, Duration::from_secs(20)) else { return Vec::new() };
    if ok {
        return Vec::new();
    }
    let base = format!("{}/", base.display());
    (out + &err)
        .lines()
        .filter(|l| !l.starts_with("[INFO]") && !l.trim().is_empty())
        .take(30)
        .map(|l| l.replace(&base, "").replace(&format!("{name}/"), ""))
        .collect()
}

fn luau_lsp() -> Option<PathBuf> {
    let exe = if cfg!(windows) { "luau-lsp.exe" } else { "luau-lsp" };
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .filter(|d| d.is_absolute())
        .chain(dirs::home_dir().map(|h| h.join(".local/bin")))
        .map(|d| d.join(exe))
        .find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_patches_pass() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../patches");
        for dir in patch::discover(&root) {
            let p = check(&dir, &Cancel::default());
            assert!(p.errors.is_empty() && p.types.is_empty(), "{}: {:?} {:?}", dir.display(), p.errors, p.types);
        }
    }
}
