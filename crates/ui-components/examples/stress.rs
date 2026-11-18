//! Stress test for the component system.
//! cargo run --release -p ui-components --example stress -- <out dir for PNGs>
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use resvg::tiny_skia::{Pixmap, PixmapPaint, Transform};
use ui_components::{Bound, FrameStats, Inputs, Ui};

#[derive(Default, Clone)]
struct World {
    binds: HashMap<String, f32>,
    meters: HashMap<String, f32>,
    spectrum: Vec<f32>,
    time: f32,
    hover: Option<u64>,
    active: Option<u64>,
}

fn frame(ui: &mut Ui, w: &World, scale: f32) -> FrameStats {
    let bound = |b: &str| {
        w.binds.get(b).map(|v| Bound { value: *v, display: format!("{:.0}%", v * 100.0), locked: false })
    };
    let meter = |m: &str| w.meters.get(m).copied();
    let series = |s: &str| (s == "spectrum").then(|| w.spectrum.clone());
    let inputs = Inputs { time: w.time, hover: w.hover, active: w.active, bound: &bound, meter: &meter, series: &series };
    ui.update(&inputs, scale);
    ui.stats
}

struct Summary {
    avg_us: f32,
    max_us: f32,
    renders: f32,
    rasters: f32,
    lua_us: f32,
    raster_us: f32,
    px: f32,
}

fn run(ui: &mut Ui, w: &mut World, frames: usize, scale: f32, mut step: impl FnMut(&mut World, usize)) -> Summary {
    let mut s = Summary { avg_us: 0.0, max_us: 0.0, renders: 0.0, rasters: 0.0, lua_us: 0.0, raster_us: 0.0, px: 0.0 };
    for i in 0..frames {
        step(w, i);
        w.time += 1.0 / 60.0;
        let st = frame(ui, w, scale);
        s.avg_us += st.total_us;
        s.max_us = s.max_us.max(st.total_us);
        s.renders += st.renders as f32;
        s.rasters += st.rasters as f32;
        s.lua_us += st.lua_us;
        s.raster_us += st.raster_us + st.parse_us;
        s.px += st.raster_px as f32;
    }
    let n = frames as f32;
    Summary { avg_us: s.avg_us / n, max_us: s.max_us, renders: s.renders / n, rasters: s.rasters / n, lua_us: s.lua_us / n, raster_us: s.raster_us / n, px: s.px / n }
}

fn print(name: &str, s: &Summary) {
    println!(
        "  {name:<34} {:>7.3} ms avg {:>7.3} ms max | {:>5.1} renders {:>5.1} rasters/frame | lua {:>6.0}µs  svg {:>6.0}µs  {:>7.0} px",
        s.avg_us / 1000.0, s.max_us / 1000.0, s.renders, s.rasters, s.lua_us, s.raster_us, s.px
    );
}

fn composite(ui: &Ui, scale: f32) -> Pixmap {
    let mut out = Pixmap::new((ui_components::WIDTH * scale) as u32, (ui_components::HEIGHT * scale) as u32).unwrap();
    for l in ui.layers() {
        if let Some(pm) = l.pixmap {
            out.draw_pixmap((l.rect.x * scale) as i32, (l.rect.y * scale) as i32, pm.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
        }
    }
    out
}

fn timed<T>(f: impl FnOnce() -> T) -> (T, f32) {
    let t = Instant::now();
    let r = f();
    (r, t.elapsed().as_secs_f32() * 1000.0)
}

fn main() {
    let out_dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "/tmp".into()));
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let neon_dir = root.join("../../patches/neon-saturator");

    // ------------------------------------------------------------ Neon
    println!("Neon Saturator (840x560, ~17 components)");
    let (ui, load_ms) = timed(|| Ui::load(&neon_dir).unwrap());
    let mut ui = ui;
    let mut w = World::default();
    for (k, v) in [("macro.tape", 0.5), ("macro.tube", 0.38), ("macro.diode", 0.24), ("macro.soft_clip", 0.32)] {
        w.binds.insert(k.into(), v);
    }
    w.meters.insert("meter.in".into(), 0.6);
    w.meters.insert("meter.out".into(), 0.7);
    w.spectrum = (0..64).map(|i| 0.8 - i as f32 / 90.0).collect();
    let (st, cold_ms) = timed(|| frame(&mut ui, &w, 1.0));
    println!("  load ui.lua {load_ms:.1} ms · first frame @1x {cold_ms:.1} ms ({} renders, {} rasters)", st.renders, st.rasters);
    composite(&ui, 1.0).save_png(out_dir.join("neon.png")).unwrap();
    let mut costs: Vec<(String, f32)> = ui.layers().iter().map(|l| (l.name.clone(), l.raster_us)).collect();
    costs.sort_by(|a, b| b.1.total_cmp(&a.1));
    println!("  slowest layers @1x: {}", costs.iter().take(6).map(|(n, us)| format!("{n} {:.1}ms", us / 1000.0)).collect::<Vec<_>>().join(", "));

    print("idle (only the animated LED)", &run(&mut ui, &mut w, 120, 1.0, |_, _| {}));
    print("drag one knob", &run(&mut ui, &mut w, 120, 1.0, |w, i| { w.binds.insert("macro.tape".into(), (i as f32 / 120.0).fract()); }));
    print("meters moving", &run(&mut ui, &mut w, 120, 1.0, |w, i| {
        w.meters.insert("meter.in".into(), 0.5 + 0.3 * (i as f32 * 0.3).sin());
        w.meters.insert("meter.out".into(), 0.6 + 0.3 * (i as f32 * 0.2).cos());
    }));
    let playing = |w: &mut World, i: usize| {
        w.meters.insert("meter.in".into(), 0.5 + 0.3 * (i as f32 * 0.3).sin());
        w.meters.insert("meter.out".into(), 0.6 + 0.3 * (i as f32 * 0.2).cos());
        w.binds.insert("macro.tape".into(), (i as f32 / 120.0).fract());
    };
    print("playing + dragging", &run(&mut ui, &mut w, 120, 1.0, playing));

    let (_, cold2) = timed(|| frame(&mut ui, &w, 2.0));
    println!("  rescale to 2x (HiDPI): {cold2:.1} ms once");
    let mut costs: Vec<(String, f32)> = ui.layers().iter().map(|l| (l.name.clone(), l.raster_us)).collect();
    costs.sort_by(|a, b| b.1.total_cmp(&a.1));
    println!("  slowest layers @2x: {}", costs.iter().take(6).map(|(n, us)| format!("{n} {:.1}ms", us / 1000.0)).collect::<Vec<_>>().join(", "));
    print("@2x idle", &run(&mut ui, &mut w, 120, 2.0, |_, _| {}));
    print("@2x drag one knob", &run(&mut ui, &mut w, 120, 2.0, |w, i| { w.binds.insert("macro.tape".into(), (i as f32 / 120.0).fract()); }));
    print("@2x playing + dragging", &run(&mut ui, &mut w, 120, 2.0, playing));

    // back to 1x (rescale), then hover over a knob
    frame(&mut ui, &w, 1.0);
    let knob = ui.hit(165.0, 390.0).expect("knob at (165, 390)");
    w.hover = Some(knob.id);
    let st = frame(&mut ui, &w, 1.0);
    println!("  hover a knob: {:.3} ms, {} renders (kind={}, bind={:?})", st.total_us / 1000.0, st.renders, knob.kind, knob.bind);
    w.hover = None;
    frame(&mut ui, &w, 1.0);

    // tabs: click ADVANCED
    let tab = ui.hit(465.0, 304.0).expect("tab button");
    ui.click(tab.id).unwrap();
    let st = frame(&mut ui, &w, 1.0);
    println!("  click tab ADVANCED: {:.2} ms, {} renders, {} rasters", st.total_us / 1000.0, st.renders, st.rasters);
    assert!(ui.hit(165.0, 390.0).is_none(), "knobs should be gone on the ADVANCED page");
    composite(&ui, 1.0).save_png(out_dir.join("neon_tab2.png")).unwrap();
    print("spectrum 64 pts, every frame", &run(&mut ui, &mut w, 120, 1.0, |w, i| {
        w.spectrum = (0..64).map(|k| (0.75 - k as f32 / 90.0 + 0.1 * ((i + k) as f32 * 0.4).sin()).clamp(0.0, 1.0)).collect();
    }));
    w.spectrum = (0..256).map(|k| (0.75 - k as f32 / 360.0).clamp(0.0, 1.0)).collect();
    print("spectrum 256 pts, every frame", &run(&mut ui, &mut w, 120, 1.0, |w, i| {
        w.spectrum = (0..256).map(|k| (0.75 - k as f32 / 360.0 + 0.1 * ((i + k) as f32 * 0.1).sin()).clamp(0.0, 1.0)).collect();
    }));

    // toggle via local state
    let toggle = ui.hit(340.0, 510.0).expect("toggle");
    ui.click(toggle.id).unwrap();
    let st = frame(&mut ui, &w, 1.0);
    println!("  click toggle (local state): {:.3} ms, {} renders", st.total_us / 1000.0, st.renders);

    // hot reload keeps state (still on ADVANCED)
    let (_, ms) = timed(|| ui.reload().unwrap());
    let st = frame(&mut ui, &w, 1.0);
    println!("  hot reload: eval {ms:.1} ms + frame {:.1} ms ({} renders) · tab kept: {}", st.total_us / 1000.0, st.renders, ui.hit(165.0, 390.0).is_none());

    // ------------------------------------------------------------ 120 knobs
    println!("\n120 knobs (grid)");
    let mut ui = Ui::load(&root.join("fixtures/grid")).unwrap();
    let mut w = World::default();
    for i in 0..120 {
        w.binds.insert(format!("param.p{i}"), (i as f32 * 0.137).fract());
    }
    let (st, ms) = timed(|| frame(&mut ui, &w, 1.0));
    println!("  first frame {ms:.1} ms ({} nodes)", st.nodes);
    print("idle", &run(&mut ui, &mut w, 60, 1.0, |_, _| {}));
    print("drag one knob", &run(&mut ui, &mut w, 60, 1.0, |w, i| { w.binds.insert("param.p5".into(), (i as f32 / 60.0).fract()); }));
    print("all 120 automated every frame", &run(&mut ui, &mut w, 60, 1.0, |w, i| {
        for k in 0..120 {
            w.binds.insert(format!("param.p{k}"), ((k + i) as f32 * 0.013).fract());
        }
    }));
    composite(&ui, 1.0).save_png(out_dir.join("grid.png")).unwrap();

    // ------------------------------------------------------------ nesting
    println!("\nnesting Panel > Section > Strip > Knob");
    let mut ui = Ui::load(&root.join("fixtures/nested")).unwrap();
    let mut w = World::default();
    for k in ["low1", "low2", "low3", "low4", "high1", "high2", "high3", "high4"] {
        w.binds.insert(format!("param.{k}"), 0.4);
    }
    frame(&mut ui, &w, 1.0);
    w.binds.insert("param.low3".into(), 0.9);
    let st = frame(&mut ui, &w, 1.0);
    println!("  drag low3: {} renders (expect 1: only that knob), {} rasters", st.renders, st.rasters);
    composite(&ui, 1.0).save_png(out_dir.join("nested.png")).unwrap();

    // ------------------------------------------------------------ animated background
    println!("\nanimated full-size background (worst case)");
    let mut ui = Ui::load(&root.join("fixtures/animated_bg")).unwrap();
    let mut w = World::default();
    frame(&mut ui, &w, 1.0);
    print("@1x", &run(&mut ui, &mut w, 60, 1.0, |_, _| {}));
    print("@2x", &run(&mut ui, &mut w, 60, 2.0, |_, _| {}));

    // ------------------------------------------------------------ broken components
    println!("\nbroken components");
    let mut ui = Ui::load(&root.join("fixtures/broken")).unwrap();
    let w = World::default();
    let (st, ms) = timed(|| frame(&mut ui, &w, 1.0));
    let ok = ui.layers().iter().filter(|l| l.pixmap.is_some() && l.error.is_none()).count();
    println!("  frame {ms:.1} ms, {} renders, healthy layers with an image: {ok}", st.renders);
    for e in ui.errors() {
        println!("  error box: {e}");
    }
    let (_, ms) = timed(|| frame(&mut ui, &w, 1.0));
    println!("  next frame (nothing changed, broken ones not retried): {ms:.3} ms");
    composite(&ui, 1.0).save_png(out_dir.join("broken.png")).unwrap();
}
