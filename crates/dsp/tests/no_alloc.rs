#![cfg(debug_assertions)] // assert_no_alloc only checks in debug builds
//! The audio path must not allocate or free: run every bundled patch under
//! assert_no_alloc (aborts the test on any allocation inside the closure), plus
//! the fixtures in `tests/fixtures` (e.g. every utility/routing block, oversampled).
use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use dsp::{Frame, Graph};
use patch::{MACRO_SLOTS, Patch};

#[global_allocator]
static A: AllocDisabler = AllocDisabler;

#[test]
fn process_is_allocation_free() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let dirs = [manifest.join("../../patches"), manifest.join("tests/fixtures")];
    for dir in dirs.iter().flat_map(|d| patch::discover(d)) {
        let p = Patch::load(&dir).unwrap();
        let mut g = Graph::build(&p, 48_000.0, &p.default_param_values());
        let mut frames: Vec<Frame> = (0..4800).map(|i| [((i as f32) * 0.05).sin() * 0.5; 2]).collect();
        let mut macros = [0.3; MACRO_SLOTS];
        assert_no_alloc(|| {
            for step in 0..10 {
                macros[0] = step as f32 / 10.0;
                g.process_all(&macros, &mut frames);
            }
        });
    }

    // Also verify allocation-free execution with all 5 modulators and active param + macro assignments.
    let tmp = std::env::temp_dir().join(format!("dsp-no-alloc-mods-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("init.luau"),
        "local g = Blocks.Gain.new('g', { db = 0 })
         local m = Macro.new('m', 'M', { default = 0.5 }):drives(g.params.db, { from = -12, to = 12 })
         return Patch.new('Mods', {
           chain = Chain.new { g },
           macros = { m },
           modulators = {
             { id = 'follow', attack_ms = 5, release_ms = 100 },
             { id = 'lfo1', rate_hz = 2, shape = 'sine' },
             { id = 'lfo2', rate_hz = 1.5, shape = 'triangle' },
             { id = 'lfo3', rate_hz = 0.5, shape = 'square' },
             { id = 'step', rate_hz = 4, values = { 0, 0.5, 1, 0.25 } },
           },
           assignments = {
             { source = 'follow', destination = g.params.db, amount = 0.25 },
             { source = 'lfo1', destination = g.params.db, amount = 0.1, bipolar = true },
             { source = 'step', destination = m, amount = 0.3 },
           },
         })",
    )
    .unwrap();
    let p = Patch::load(&tmp).unwrap();
    let _ = std::fs::remove_dir_all(&tmp);
    let mut g = Graph::build(&p, 48_000.0, &p.default_param_values());
    // A rebuilt graph (routing changed) takes over on the audio thread.
    let mut next = Graph::build(&p, 48_000.0, &p.default_param_values());
    let mut frames: Vec<Frame> = (0..4800).map(|i| [((i as f32) * 0.05).sin() * 0.5; 2]).collect();
    let macros = [0.3; MACRO_SLOTS];
    assert_no_alloc(|| {
        g.process_all(&macros, &mut frames);
        next.adopt(&mut g);
        next.process_all(&macros, &mut frames);
    });
}
