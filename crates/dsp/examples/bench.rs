//! Offline benchmark: synthetic bass line through each bundled patch.
//! cargo run --release -p dsp --example bench
use dsp::{Frame, Graph};
use patch::{MACRO_SLOTS, Patch};
use std::time::Instant;

const SR: f32 = 48_000.0;

/// Plucked, slightly detuned saw bass: notes every 250 ms with decaying envelope.
fn bass_line(secs: f32) -> Vec<Frame> {
    let notes = [41.2, 41.2, 55.0, 49.0, 41.2, 61.7, 55.0, 36.7];
    let n = (secs * SR) as usize;
    let mut phase = 0.0f32;
    (0..n)
        .map(|i| {
            let t = i as f32 / SR;
            let step = (t / 0.25) as usize;
            let local = t - step as f32 * 0.25;
            let f = notes[step % notes.len()];
            phase = (phase + f / SR).fract();
            let saw = 2.0 * phase - 1.0;
            let env = (-local * 6.0).exp() * (1.0 - (-local * 800.0).exp());
            let x = 0.5 * saw * env;
            [x, x]
        })
        .collect()
}

fn peak_db(frames: &[Frame]) -> f32 {
    20.0 * frames.iter().map(|f| f[0].abs()).fold(0.0, f32::max).max(1e-9).log10()
}

fn main() {
    let root = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../patches"));
    let input = bass_line(20.0);
    println!("input: 20 s synthetic bass, peak {:.1} dBFS\n", peak_db(&input));
    for dir in patch::discover(&root) {
        let p = Patch::load(&dir).unwrap();
        let mut macros = [0.0; MACRO_SLOTS];
        for i in 0..p.macros.len() {
            macros[i] = p.macro_default(i);
        }
        let mut g = Graph::build(&p, SR, &p.default_param_values());
        let mut buf = input.clone();
        let t = Instant::now();
        g.process_all(&macros, &mut buf);
        let el = t.elapsed().as_secs_f64();
        let meters: Vec<String> = p
            .meters
            .iter()
            .zip(g.meters().iter())
            .map(|(id, m)| format!("{id}={:.2}", m.load(std::sync::atomic::Ordering::Relaxed)))
            .collect();
        println!(
            "{:<12} {:>2} blocks  {:>7.1}x realtime  ({:.2}% of one core)  out peak {:>5.1} dBFS  {}",
            p.name,
            count(&p.chain),
            20.0 / el,
            el / 20.0 * 100.0,
            peak_db(&buf),
            meters.join(" ")
        );
    }
    sweep(&input);
}

/// Peak, RMS (dBFS), crest factor and high-band energy share of the last 10 s.
fn stats(frames: &[Frame]) -> (f32, f32, f32, f32) {
    let tail = &frames[frames.len() / 2..];
    let peak = tail.iter().map(|f| f[0].abs()).fold(0.0, f32::max);
    let rms = (tail.iter().map(|f| f[0] * f[0]).sum::<f32>() / tail.len() as f32).sqrt();
    // crude "brightness": energy of the first difference vs. the signal
    let diff = (tail.windows(2).map(|w| (w[1][0] - w[0][0]).powi(2)).sum::<f32>() / tail.len() as f32).sqrt();
    let db = |x: f32| 20.0 * x.max(1e-9).log10();
    (db(peak), db(rms), db(peak) - db(rms), db(diff / rms))
}

fn sweep(input: &[Frame]) {
    let root = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../patches"));
    let p = Patch::load(root.join("bass-helper")).unwrap();
    println!("\nBass Helper macro sweeps (others at default):");
    println!("{:<12} {:>5}  {:>7} {:>7} {:>6} {:>10}", "macro", "value", "peak", "rms", "crest", "brightness");
    for name in ["transients", "stabilize", "drive", "boom", "click"] {
        let slot = p.macro_index(name).unwrap();
        for v in [0.0, 0.5, 1.0] {
            let mut macros = [0.0; MACRO_SLOTS];
            for i in 0..p.macros.len() {
                macros[i] = p.macro_default(i);
            }
            macros[slot] = v;
            let mut g = Graph::build(&p, SR, &p.default_param_values());
            let mut buf = input.to_vec();
            g.process_all(&macros, &mut buf);
            let (pk, rms, crest, bright) = stats(&buf);
            println!("{name:<12} {v:>5.1}  {pk:>6.1}  {rms:>6.1}  {crest:>5.1}  {bright:>9.1}");
        }
    }
}

fn count(chain: &[patch::BlockSpec]) -> usize {
    let mut n = 0;
    patch::chain::walk(chain, &mut |_| n += 1);
    n
}
