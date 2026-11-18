//! Every block in the prelude's `Blocks` is documented in the definitions file
//! (`patches/.types/scriptfx.d.luau`) with a doc comment that starts with its
//! category: `--- <Category> · <what it does>`. The doc comments show up on hover
//! and in completion (luau-lsp), and an LLM can search the file by category.

use std::path::Path;

const CATEGORIES: &[&str] =
    &["Utility", "EQ", "Filter", "Dynamics", "Distortion", "Modulation", "Space", "Pitch", "Rhythm", "Routing"];

fn prelude_blocks() -> Vec<String> {
    let dir = std::env::temp_dir().join(format!("block-docs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let rt = patch::runtime::Runtime::new(&dir).unwrap();
    let mut names: Vec<String> = rt
        .lua
        .load("local t = {} for k in pairs(Blocks) do t[#t + 1] = k end return t")
        .eval()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    names.sort();
    names
}

/// `name → first doc line` of the entries in `declare Blocks: { ... }`.
fn documented_blocks() -> Vec<(String, Option<String>)> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../patches/.types/scriptfx.d.luau");
    let src = std::fs::read_to_string(path).unwrap();
    let body = src.split("declare Blocks: {").nth(1).expect("declare Blocks").split("\n}").next().unwrap();
    let lines: Vec<&str> = body.lines().collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let Some((name, _)) = line.trim().split_once(": { new:") else { continue };
        let mut first_doc = None;
        for prev in lines[..i].iter().rev() {
            match prev.trim().strip_prefix("---") {
                Some(doc) => first_doc = Some(doc.trim().to_string()),
                None => break,
            }
        }
        out.push((name.to_string(), first_doc));
    }
    out.sort();
    out
}

#[test]
fn every_block_is_documented_with_a_category() {
    let blocks = prelude_blocks();
    let docs = documented_blocks();
    let doc_names: Vec<&String> = docs.iter().map(|(n, _)| n).collect();
    assert_eq!(doc_names, blocks.iter().collect::<Vec<_>>(), "blocks in the prelude vs. scriptfx.d.luau");
    for (name, doc) in &docs {
        let doc = doc.as_deref().unwrap_or_else(|| panic!("Blocks.{name} has no `---` doc comment"));
        let category = doc.split(" · ").next().unwrap();
        assert!(
            doc.contains(" · ") && CATEGORIES.contains(&category),
            "Blocks.{name}: doc must start with `<Category> · ` ({CATEGORIES:?}), got {doc:?}"
        );
    }
}
