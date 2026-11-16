//! Headless check: render a preset to PNG and benchmark animated frames.
//! cargo run --release -p ui-svg --example render -- presets/tape-glow out.png
use preset::{Preset, Values};
use std::time::Instant;
use ui_svg::SvgPanel;

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().expect("preset dir");
    let out = args.next().unwrap_or_else(|| "out.png".into());
    let t = Instant::now();
    let preset = Preset::load(&dir).unwrap_or_else(|e| panic!("{e}"));
    let t_preset = t.elapsed();
    let t = Instant::now();
    let mut panel = SvgPanel::load(&preset).unwrap_or_else(|e| panic!("{e}"));
    println!("load: preset {:?}, panel {:?} (incl. fonts)", t_preset, t.elapsed());
    for w in &panel.warnings {
        println!("warning: {w}");
    }

    let mut v = Values::default();
    for i in 0..preset.macros.len() {
        v.macros[i] = preset.macro_default(i);
    }
    let free = preset.default_param_values();
    v.meter_in = 0.6;
    v.meter_out = 0.75;
    v.time = 1.0;
    let m = v.macros;
    preset.compute_params(&m, &free, &mut v.params);
    preset.animate(&mut v, 1.0 / 60.0).unwrap();
    let (w, h) = panel.size();
    panel.update(&v, (w * 2.0) as u32, (h * 2.0) as u32);
    panel.pixmap().unwrap().save_png(&out).unwrap();
    println!("wrote {out}");

    // Benchmark: 300 animated frames at 1x and 2x (HiDPI).
    for scale in [1.0f32, 2.0] {
        let mut anim_us = 0.0;
        let t = Instant::now();
        for i in 0..300 {
            v.time += 1.0 / 60.0;
            v.meter_out = 0.5 + 0.4 * (i as f32 * 0.1).sin();
            v.macros[0] = (i as f32 / 300.0).fract();
            let m = v.macros;
            preset.compute_params(&m, &free, &mut v.params);
            let ta = Instant::now();
            preset.animate(&mut v, 1.0 / 60.0).unwrap();
            anim_us += ta.elapsed().as_secs_f32() * 1e6;
            panel.update(&v, (w * scale) as u32, (h * scale) as u32);
        }
        let s = panel.stats;
        println!(
            "{scale}x: {:.2} ms/frame total | lua animate {:.0}us patch {:.0}us parse {:.0}us render {:.0}us",
            t.elapsed().as_secs_f32() * 1000.0 / 300.0,
            anim_us / 300.0,
            s.patch_us,
            s.parse_us,
            s.render_us
        );
    }
}
