use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ui_components::{Bound, Inputs, Ui};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures").join(name)
}

fn frame(ui: &mut Ui, binds: &HashMap<String, f32>, hover: Option<u64>) {
    let bound = |b: &str| binds.get(b).map(|v| Bound { value: *v, display: format!("{v:.2}"), locked: false });
    let meter = |_: &str| None;
    let series = |_: &str| None;
    ui.update(&Inputs { time: 0.0, hover, active: None, bound: &bound, meter: &meter, series: &series }, 1.0);
}

#[test]
fn only_the_changed_leaf_rerenders() {
    let mut ui = Ui::load(&fixture("nested")).unwrap();
    let mut binds: HashMap<String, f32> =
        ["low1", "low2", "low3", "low4", "high1", "high2", "high3", "high4"].iter().map(|k| (format!("param.{k}.db"), 0.4)).collect();
    frame(&mut ui, &binds, None);
    assert!(ui.stats.renders >= 15, "first frame renders everything");

    frame(&mut ui, &binds, None);
    assert_eq!(ui.stats.renders, 0, "nothing changed");

    binds.insert("param.low3.db".into(), 0.9);
    frame(&mut ui, &binds, None);
    assert_eq!(ui.stats.renders, 1, "only the knob bound to low3");
    assert_eq!(ui.stats.rasters, 1);
}

#[test]
fn hover_only_affects_the_hovered_component() {
    let mut ui = Ui::load(&fixture("nested")).unwrap();
    let binds = HashMap::new();
    frame(&mut ui, &binds, None);
    let knob = ui.hit(60.0, 100.0).expect("a knob in the LOW section");
    assert_eq!(knob.kind, "knob");
    frame(&mut ui, &binds, Some(knob.id));
    assert_eq!(ui.stats.renders, 1);
}

#[test]
fn broken_components_are_isolated() {
    let mut ui = Ui::load(&fixture("broken")).unwrap();
    frame(&mut ui, &HashMap::new(), None);
    let errors = ui.errors();
    assert_eq!(errors.len(), 4, "{errors:?}");
    assert!(errors.iter().any(|e| e.contains("budget")));
    assert!(errors.iter().any(|e| e.contains("memory")));
    assert!(errors.iter().any(|e| e.contains("invalid SVG")));
    let healthy = ui.layers().iter().filter(|l| l.pixmap.is_some() && l.error.is_none()).count();
    assert_eq!(healthy, 3, "panel + both 'fine' components");
}

#[test]
fn local_state_and_reload() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../patches/neon-saturator");
    let mut ui = Ui::load(&dir).unwrap();
    let binds = HashMap::new();
    frame(&mut ui, &binds, None);
    assert!(ui.hit(165.0, 390.0).is_some(), "knob on the MAIN page");
    let tab = ui.hit(465.0, 304.0).expect("ADVANCED tab");
    ui.click(tab.id).unwrap();
    frame(&mut ui, &binds, None);
    assert!(ui.hit(165.0, 390.0).is_none(), "ADVANCED page has no knob there");
    ui.reload().unwrap();
    frame(&mut ui, &binds, None);
    assert!(ui.hit(165.0, 390.0).is_none(), "tab state survives a reload");
}

/// Layers may overflow their component (glows), never the panel: Junkyard's filtered full-size
/// background once smeared outside the panel over the shell.
#[test]
fn no_layer_leaves_the_panel() {
    let patches = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../patches");
    for name in ["junkyard-amp", "neon-saturator", "tape-glow", "clean-gain", "bass-helper"] {
        let mut ui = Ui::load(&patches.join(name)).unwrap();
        for scale in [1.0, 1.5, 2.0] {
            let bound = |_: &str| Some(Bound { value: 0.5, display: String::new(), locked: false });
            let (meter, series) = (|_: &str| Some(0.5), |_: &str| None);
            ui.update(&Inputs { time: 0.0, hover: None, active: None, bound: &bound, meter: &meter, series: &series }, scale);
            for l in ui.layers().iter().filter(|l| l.pixmap.is_some()) {
                let r = l.rect;
                assert!(
                    r.x >= 0.0 && r.y >= 0.0 && r.x + r.w <= ui_components::WIDTH && r.y + r.h <= ui_components::HEIGHT,
                    "{name} @{scale}x: layer {:?} at {r:?} leaves the 840×560 panel",
                    l.kind
                );
            }
        }
    }
}

#[test]
fn lua_knows_the_panel_size() {
    let rt = patch::runtime::Runtime::new(&fixture("numbers")).unwrap();
    let ui: mlua::Table = rt.lua.globals().get("UI").unwrap();
    assert_eq!(ui.get::<f32>("WIDTH").unwrap(), ui_components::WIDTH);
    assert_eq!(ui.get::<f32>("HEIGHT").unwrap(), ui_components::HEIGHT);
}
