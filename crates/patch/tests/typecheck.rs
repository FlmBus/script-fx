//! Type-checks the bundled patches against `patches/.types/scriptfx.d.luau` with
//! `luau-lsp analyze` (strict mode via `patches/.luaurc`). This also keeps the
//! definitions file honest: it has to describe what the patches actually use.
//! Skipped when `luau-lsp` isn't installed.

use std::path::{Path, PathBuf};
use std::process::Command;

fn luau_lsp() -> Option<PathBuf> {
    let candidates = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .chain(std::env::var_os("HOME").map(|h| Path::new(&h).join(".local/bin")));
    candidates.map(|d| d.join("luau-lsp")).find(|p| p.is_file())
}

#[test]
fn bundled_patches_type_check() {
    let Some(bin) = luau_lsp() else {
        eprintln!("luau-lsp not installed, skipping the type check");
        return;
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
    let mut files = Vec::new();
    // Factory patches plus the DSP test fixtures (which use every block).
    let roots = [root.join("patches"), root.join("crates/dsp/tests/fixtures")];
    for dir in roots.iter().flat_map(|r| patch::discover(r)) {
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            if e.path().extension().is_some_and(|x| x == "luau") {
                files.push(e.path().strip_prefix(&root).unwrap().to_path_buf());
            }
        }
    }
    assert!(files.len() >= 7, "found {files:?}");
    let out = Command::new(bin)
        .current_dir(&root)
        .args(["analyze", "--platform=standard", "--defs=@scriptfx=patches/.types/scriptfx.d.luau"])
        .args(&files)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    let problems: Vec<&str> = text.lines().filter(|l| !l.starts_with("[INFO]")).collect();
    assert!(problems.is_empty() && out.status.success(), "luau-lsp analyze:\n{}", problems.join("\n"));
}
