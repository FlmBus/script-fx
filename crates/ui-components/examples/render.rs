//! Render a component UI to PNG: cargo run --release -p ui-components --example render -- <patch dir> <out.png> [scale] [bind=value ...]
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use resvg::tiny_skia::{Pixmap, PixmapPaint, Transform};
use ui_components::{Bound, Inputs, Ui};

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().expect("patch dir"));
    let out = args.next().expect("out.png");
    let scale: f32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let mut binds: HashMap<String, f32> = HashMap::new();
    let mut hover_at: Option<(f32, f32)> = None;
    for a in args {
        if let Some(rest) = a.strip_prefix("hover=") {
            let (x, y) = rest.split_once(',').unwrap();
            hover_at = Some((x.parse().unwrap(), y.parse().unwrap()));
        } else if let Some((k, v)) = a.split_once('=') {
            binds.insert(k.into(), v.parse().unwrap());
        }
    }
    let t = Instant::now();
    let mut ui = Ui::load(&dir).unwrap_or_else(|e| panic!("{e}"));
    let load = t.elapsed();
    let bound = |b: &str| Some(Bound { value: *binds.get(b).unwrap_or(&0.5), display: format!("{:.1}", binds.get(b).unwrap_or(&0.5) * 10.0), locked: false });
    let meter = |_: &str| Some(0.7);
    let series = |_: &str| None;
    let mut hover = None;
    for pass in 0..2 {
        let inputs = Inputs { time: 0.3, hover, active: hover, bound: &bound, meter: &meter, series: &series };
        ui.update(&inputs, scale);
        if pass == 0 {
            hover = hover_at.and_then(|(x, y)| ui.hit(x, y)).map(|h| h.id);
        }
        if hover.is_none() {
            break;
        }
    }
    let s = ui.stats;
    println!("load {:.1} ms · frame {:.1} ms · {} nodes · {} renders · lua {:.1} ms · svg {:.1} ms",
        load.as_secs_f32() * 1000.0, s.total_us / 1000.0, s.nodes, s.renders, s.lua_us / 1000.0, (s.parse_us + s.raster_us) / 1000.0);
    let mut costs: Vec<(String, f32)> = ui.layers().iter().map(|l| (l.name.clone(), l.raster_us)).collect();
    costs.sort_by(|a, b| b.1.total_cmp(&a.1));
    println!("slowest: {}", costs.iter().take(6).map(|(n, us)| format!("{n} {:.1}ms", us / 1000.0)).collect::<Vec<_>>().join(", "));
    for e in ui.errors() {
        println!("ERROR: {e}");
    }
    let mut pm = Pixmap::new((ui_components::WIDTH * scale) as u32, (ui_components::HEIGHT * scale) as u32).unwrap();
    for l in ui.layers() {
        if let Some(p) = l.pixmap {
            pm.draw_pixmap((l.rect.x * scale) as i32, (l.rect.y * scale) as i32, p.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
        }
    }
    pm.save_png(&out).unwrap();

    // Optional: BENCH_DRAG=<bind> drags that knob for 120 frames.
    if let Ok(target) = std::env::var("BENCH_DRAG") {
        let mut total = 0.0;
        let mut max: f32 = 0.0;
        for i in 0..120 {
            let v = (i as f32 / 120.0).fract();
            let bound = |b: &str| Some(Bound { value: if b == target { v } else { *binds.get(b).unwrap_or(&0.5) }, display: format!("{:.1}", v * 10.0), locked: false });
            let inputs = Inputs { time: i as f32 / 60.0, hover: None, active: None, bound: &bound, meter: &meter, series: &series };
            ui.update(&inputs, scale);
            total += ui.stats.total_us;
            max = max.max(ui.stats.total_us);
        }
        println!("drag {target} @{scale}x: {:.2} ms avg, {:.2} ms max", total / 120.0 / 1000.0, max / 1000.0);
    }
}
