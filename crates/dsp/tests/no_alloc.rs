#![cfg(debug_assertions)] // assert_no_alloc only checks in debug builds
//! The audio path must not allocate or free: run every bundled preset under
//! assert_no_alloc (aborts the test on any allocation inside the closure).
use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use dsp::{Frame, Graph};
use preset::{MACRO_SLOTS, Preset};

#[global_allocator]
static A: AllocDisabler = AllocDisabler;

#[test]
fn process_is_allocation_free() {
    let root = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../presets"));
    for dir in preset::discover(&root) {
        let p = Preset::load(&dir).unwrap();
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
}
