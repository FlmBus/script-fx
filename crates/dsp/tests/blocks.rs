//! Behaviour of every block from `dsp::fx`: one or two measurable properties
//! each (levels, spectra, timing), not golden files.

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use dsp::{Frame, Graph};
use patch::{MACRO_SLOTS, Patch};

const SR: f32 = 48_000.0;

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        use std::sync::atomic::AtomicU32;
        static N: AtomicU32 = AtomicU32::new(0);
        let p = std::env::temp_dir().join(format!("dsp-blocks-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn load(chain: &str, setup: &str) -> Patch {
    let d = TempDir::new();
    std::fs::write(d.0.join("init.luau"), format!("{setup}\nreturn Patch.new('T', {{ chain = Chain.new {{ {chain} }} }})"))
        .unwrap();
    Patch::load(&d.0).unwrap_or_else(|e| panic!("{chain}: {e}"))
}

fn graph(chain: &str) -> Graph {
    let p = load(chain, "");
    Graph::build(&p, SR, &p.default_param_values())
}

fn through(chain: &str, mut x: Vec<Frame>) -> Vec<Frame> {
    graph(chain).process_all(&[0.0; MACRO_SLOTS], &mut x);
    x
}

fn sine(freq: f32, db: f32, secs: f32) -> Vec<Frame> {
    let a = 10f32.powf(db / 20.0);
    (0..(secs * SR) as usize)
        .map(|i| {
            let x = a * (std::f32::consts::TAU * freq * i as f32 / SR).sin();
            [x, x]
        })
        .collect()
}

fn impulse(len: usize) -> Vec<Frame> {
    let mut x = vec![[0.0; 2]; len];
    x[0] = [1.0, 1.0];
    x
}

fn peak(x: &[Frame], ch: usize) -> f32 {
    x.iter().fold(0.0f32, |p, f| p.max(f[ch].abs()))
}

fn rms_db(x: &[Frame], ch: usize) -> f32 {
    let ms = x.iter().map(|f| (f[ch] as f64).powi(2)).sum::<f64>() / x.len() as f64;
    (10.0 * ms.max(1e-30).log10()) as f32
}

/// Level (dB re a full-scale sine) of `freq` in channel `ch` (Goertzel, Hann window).
fn tone_db(x: &[Frame], freq: f32, ch: usize) -> f32 {
    let w = std::f64::consts::TAU * freq as f64 / SR as f64;
    let c = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    let n = x.len();
    for (i, f) in x.iter().enumerate() {
        let win = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos();
        let s0 = f[ch] as f64 * win + c * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    let power = s1 * s1 + s2 * s2 - c * s1 * s2;
    (20.0 * (power.sqrt() / (n as f64 / 4.0)).max(1e-12).log10()) as f32
}

fn all_finite(x: &[Frame]) -> bool {
    x.iter().all(|f| f[0].is_finite() && f[1].is_finite())
}

// ---------------------------------------------------------------- distortion

#[test]
fn waveshaper_follows_the_curve() {
    let x = sine(1000.0, -6.0, 0.1);
    let inv = through("Blocks.Waveshaper.new('w', { curve = function(x) return -x end, oversampling = 1 })", x.clone());
    assert!(inv.iter().zip(&x).all(|(a, b)| (a[0] + b[0]).abs() < 1e-3), "function curve inverts");
    let list = through("Blocks.Waveshaper.new('w', { curve = { 1, 0, 1 }, oversampling = 1 })", x.clone());
    assert!(list.iter().zip(&x).all(|(a, b)| (a[0] - b[0].abs()).abs() < 1e-3), "list curve = |x|");
    let squash = through("Blocks.Waveshaper.new('w', { curve = function(x) return math.tanh(4 * x) end, drive = 6 })", x);
    assert!(tone_db(&squash[2400..], 3000.0, 0) > -30.0, "tanh adds a 3rd harmonic");
}

#[test]
fn wavefolder_and_clipper_add_harmonics_or_limit() {
    let x = sine(500.0, -6.0, 0.2);
    let folded = through("Blocks.Wavefolder.new('f', { amount = 18 })", x.clone());
    assert!(all_finite(&folded) && tone_db(&folded[4800..], 1500.0, 0) > -30.0);
    let clipped = through("Blocks.Clipper.new('c', { ceiling = -12, oversampling = 1 })", x.clone());
    assert!(peak(&clipped, 0) <= 10f32.powf(-12.0 / 20.0) + 1e-6);
    let soft = through("Blocks.Clipper.new('c', { ceiling = -12, softness = 100, oversampling = 1 })", x);
    assert!(peak(&soft, 0) <= 10f32.powf(-12.0 / 20.0) + 1e-6);
}

#[test]
fn rectifier_doubles_the_frequency() {
    let y = through("Blocks.Rectifier.new('r')", sine(200.0, -6.0, 0.5));
    let t = &y[12_000..];
    assert!(tone_db(t, 400.0, 0) > tone_db(t, 200.0, 0) + 30.0);
}

#[test]
fn bitcrusher_quantizes() {
    let x = sine(1000.0, -6.0, 0.1);
    let coarse = through("Blocks.Bitcrusher.new('b', { bits = 2 })", x.clone());
    let levels: std::collections::BTreeSet<i32> = coarse.iter().map(|f| (f[0] * 1000.0).round() as i32).collect();
    assert!(levels.len() <= 5, "{levels:?}");
    let fine = through("Blocks.Bitcrusher.new('b', { bits = 24 })", x.clone());
    assert!(fine.iter().zip(&x).all(|(a, b)| (a[0] - b[0]).abs() < 1e-5));
    let held = through("Blocks.Bitcrusher.new('b', { bits = 24, rate = 4800 })", x);
    assert!(held.windows(2).filter(|w| w[0] == w[1]).count() > 4000, "sample and hold repeats values");
}

#[test]
fn ring_mod_makes_sum_and_difference() {
    let y = through("Blocks.RingMod.new('r', { freq = 300 })", sine(1000.0, -6.0, 0.5));
    let t = &y[4800..];
    assert!(tone_db(t, 700.0, 0) > -20.0 && tone_db(t, 1300.0, 0) > -20.0);
    assert!(tone_db(t, 1000.0, 0) < -60.0, "{}", tone_db(t, 1000.0, 0));
}

#[test]
fn tape_saturates_wobbles_and_reports_its_delay() {
    let g = graph("Blocks.Tape.new('t', { drive = 12, wow = 100, flutter = 100, oversampling = 1 })");
    assert_eq!(g.latency(), (0.004 * SR).ceil() as usize);
    let y = through("Blocks.Tape.new('t', { drive = 12, wow = 100, flutter = 100 })", sine(1000.0, -6.0, 1.0));
    assert!(all_finite(&y));
    let level = rms_db(&y[24_000..], 0) - rms_db(&sine(1000.0, -6.0, 0.5), 0);
    assert!(level.abs() < 8.0, "{level}");
    assert!(tone_db(&y[24_000..], 2000.0, 0) > -50.0, "asymmetric: even harmonics");
}

// ---------------------------------------------------------------- filter

fn gain_through(chain: &str, freq: f32) -> f32 {
    let x = sine(freq, -24.0, 0.5);
    let y = through(chain, x.clone());
    rms_db(&y[12_000..], 0) - rms_db(&x[12_000..], 0)
}

#[test]
fn morph_filter_morphs() {
    let lp = "Blocks.MorphFilter.new('m', { freq = 1000, morph = 0 })";
    let bp = "Blocks.MorphFilter.new('m', { freq = 1000, morph = 50, resonance = 50 })";
    let hp = "Blocks.MorphFilter.new('m', { freq = 1000, morph = 100 })";
    assert!(gain_through(lp, 100.0).abs() < 0.5 && gain_through(lp, 10_000.0) < -30.0);
    assert!(gain_through(hp, 10_000.0).abs() < 0.5 && gain_through(hp, 100.0) < -30.0);
    assert!(gain_through(bp, 1000.0).abs() < 0.5 && gain_through(bp, 100.0) < -15.0 && gain_through(bp, 10_000.0) < -15.0);
}

#[test]
fn ladder_is_steep() {
    let c = "Blocks.LadderFilter.new('l', { freq = 500 })";
    assert!(gain_through(c, 50.0) > -3.0);
    assert!(gain_through(c, 5000.0) < -60.0, "{}", gain_through(c, 5000.0));
    let res = through("Blocks.LadderFilter.new('l', { freq = 1000, resonance = 90 })", sine(1000.0, -24.0, 0.5));
    assert!(all_finite(&res));
}

#[test]
fn comb_peaks_at_its_harmonics() {
    let c = "Blocks.CombFilter.new('c', { freq = 500, feedback = 90, mix = 100, oversampling = 1 })";
    assert!(gain_through(c, 1000.0) > gain_through(c, 750.0) + 20.0);
}

#[test]
fn formant_follows_the_vowel() {
    // "a" has F1 ≈ 730 Hz, "i" ≈ 270 Hz.
    let a = "Blocks.Formant.new('f', { vowel = 0 })";
    let i = "Blocks.Formant.new('f', { vowel = 2 })";
    assert!(gain_through(a, 730.0) > gain_through(a, 270.0) + 6.0);
    assert!(gain_through(i, 270.0) > gain_through(i, 730.0) + 6.0);
}

#[test]
fn resonator_rings_at_its_notes() {
    let y = through("Blocks.Resonator.new('r', { notes = { 'A4' }, decay = 1000, mix = 100 })", impulse(24_000));
    let t = &y[2400..];
    assert!(tone_db(t, 440.0, 0) > tone_db(t, 300.0, 0) + 30.0);
    assert!(rms_db(&y[19_200..], 0) < rms_db(&y[..4800], 0) - 20.0, "decays");
}

#[test]
fn dynamic_eq_cuts_only_when_loud() {
    let c = "Blocks.DynamicEq.new('d', { freq = 1000, threshold = -20, ratio = 4, range = 12 })";
    let mut g = graph(c);
    let mut loud = sine(1000.0, -6.0, 0.5);
    let reference = rms_db(&loud[12_000..], 0);
    g.process_all(&[0.0; MACRO_SLOTS], &mut loud);
    let cut = reference - rms_db(&loud[12_000..], 0);
    // Band at about -9 dB (sine envelope), 11 dB over: 11 × 0.75 ≈ 8 dB.
    assert!(cut > 5.0 && cut < 13.0, "cut {cut}");
    assert!(g.meters()[0].load(Ordering::Relaxed) > 0.2);
    assert!(gain_through(c, 1000.0).abs() < 0.5, "quiet (-24 dB, below the threshold) passes");
}

// ---------------------------------------------------------------- modulation

#[test]
fn modulation_blocks_move_but_keep_the_level() {
    for c in [
        "Blocks.Chorus.new('c')",
        "Blocks.Flanger.new('f', { feedback = 80 })",
        "Blocks.Phaser.new('p', { stages = 8, feedback = 50 })",
        "Blocks.Chorus.new('c', { sync = '1/4' })",
    ] {
        let x = sine(1000.0, -12.0, 1.0);
        let y = through(c, x.clone());
        assert!(all_finite(&y), "{c}");
        let level = rms_db(&y[12_000..], 0) - rms_db(&x[12_000..], 0);
        assert!(level.abs() < 9.0, "{c}: {level}");
        assert!(y.iter().zip(&x).any(|(a, b)| (a[0] - b[0]).abs() > 0.01), "{c} changes the signal");
        assert!(y[12_000..].iter().any(|f| (f[0] - f[1]).abs() > 1e-3), "{c} is stereo");
    }
}

#[test]
fn tremolo_and_auto_pan_modulate_level() {
    let y = through("Blocks.Tremolo.new('t', { rate = 10, depth = 100 })", vec![[0.5, 0.5]; 24_000]);
    let lo = y.iter().map(|f| f[0]).fold(1.0f32, f32::min);
    assert!(lo < 0.02 && peak(&y, 0) > 0.49, "{lo}");
    let y = through("Blocks.AutoPan.new('a', { rate = 10, shape = 'square' })", vec![[0.5, 0.5]; 24_000]);
    assert!(y.iter().any(|f| f[0] < 0.01 && f[1] > 0.49) && y.iter().any(|f| f[1] < 0.01 && f[0] > 0.49));
}

#[test]
fn synced_tremolo_locks_to_the_beat_grid_and_phase_offset_shifts_it() {
    // 1/4 square tremolo at 120 BPM: loud in the first half of every beat (250 ms).
    let level_at = |offset: f32, start_beat: f64, ms: f32| {
        let mut g = graph(&format!("Blocks.Tremolo.new('t', {{ sync = '1/4', shape = 'square', depth = 100, phase_offset = {offset} }})"));
        g.set_transport(Some(120.0), Some(start_beat));
        let mut x = vec![[1.0f32, 1.0]; 48_000];
        g.process_all(&[0.0; MACRO_SLOTS], &mut x);
        x[(ms * 0.001 * SR) as usize][0]
    };
    // Beat 0 starts at frame 0: loud at 100 ms (first half), silent at 350 ms (second half).
    assert!(level_at(0.0, 0.0, 100.0) > 0.99 && level_at(0.0, 0.0, 350.0) < 0.01);
    // 180° flips it; 360° is a full cycle (no change).
    assert!(level_at(180.0, 0.0, 100.0) < 0.01 && level_at(180.0, 0.0, 350.0) > 0.99);
    assert!(level_at(360.0, 0.0, 100.0) > 0.99 && level_at(360.0, 0.0, 350.0) < 0.01);
    // Starting mid-beat (host at beat 2.5): the grid, not the start, decides.
    assert!(level_at(0.0, 2.5, 100.0) < 0.01 && level_at(0.0, 2.5, 350.0) > 0.99);
    // AutoPan follows the same grid.
    let mut g = graph("Blocks.AutoPan.new('a', { sync = '1/4', shape = 'square', phase_offset = 90 })");
    g.set_transport(Some(120.0), Some(0.0));
    let mut x = vec![[1.0f32, 1.0]; 24_000];
    g.process_all(&[0.0; MACRO_SLOTS], &mut x);
    // Square + 90°: right (pan +1 → left silent) for 0..125 ms, left for 125..375 ms.
    assert!(x[2400][0] < 0.01 && x[2400][1] > 0.99, "{:?}", x[2400]);
    assert!(x[12_000][1] < 0.01 && x[12_000][0] > 0.99, "{:?}", x[12_000]);
}

// ---------------------------------------------------------------- space

#[test]
fn delay_repeats_with_feedback_and_chain() {
    let y = through("Blocks.Delay.new('d', { time = 10, feedback = 50, mix = 100 })", impulse(2000));
    assert!((y[480][0] - 1.0).abs() < 1e-3 && (y[960][0] - 0.5).abs() < 1e-3, "{} {}", y[480][0], y[960][0]);
    let y = through(
        "Blocks.Delay.new('d', { time = 10, feedback = 50, mix = 100, feedback_chain = Chain.new { Blocks.Gain.new('g', { db = -6.0206 }) } })",
        impulse(2000),
    );
    assert!((y[960][0] - 0.25).abs() < 1e-3, "{}", y[960][0]);
    let y = through("Blocks.Delay.new('d', { time = 10, feedback = 50, mix = 100, pingpong = true })", impulse(2000));
    assert!(y[480][0] > 0.9 && y[480][1].abs() < 1e-3 && y[960][1] > 0.4 && y[960][0].abs() < 1e-3);
}

#[test]
fn delay_syncs_to_the_host_tempo() {
    let mut g = graph("Blocks.Delay.new('d', { sync = '1/8', feedback = 0, mix = 100 })");
    g.set_transport(Some(150.0), None); // 1/8 at 150 BPM = 200 ms
    let mut x = impulse(20_000);
    g.process_all(&[0.0; MACRO_SLOTS], &mut x);
    let at = x.iter().position(|f| f[0].abs() > 0.5).unwrap();
    assert_eq!(at, 9600);
}

#[test]
fn delay_compensates_an_oversampled_feedback_chain() {
    let y = through(
        "Blocks.Delay.new('d', { time = 10, feedback = 50, mix = 100, feedback_chain = Chain.new { Blocks.Gain.new('g', { db = 0, oversampling = 2 }) } })",
        impulse(2000),
    );
    let second = y[600..].iter().enumerate().max_by(|a, b| a.1[0].abs().total_cmp(&b.1[0].abs())).unwrap().0 + 600;
    assert_eq!(second, 960);
}

#[test]
fn reverb_decays_in_about_its_rt60() {
    let y = through("Blocks.Reverb.new('r', { decay = 1, mix = 100 })", impulse((2.0 * SR) as usize));
    assert!(all_finite(&y));
    let early = rms_db(&y[(0.1 * SR) as usize..(0.2 * SR) as usize], 0);
    let late = rms_db(&y[(1.1 * SR) as usize..(1.2 * SR) as usize], 0);
    let drop = early - late;
    assert!((45.0..75.0).contains(&drop), "decay over 1 s: {drop} dB");
    assert!(y[(0.1 * SR) as usize..].iter().any(|f| (f[0] - f[1]).abs() > 1e-4), "stereo");
}

#[test]
fn frozen_reverb_sustains() {
    let p = load(
        "Blocks.Reverb.new('r', { decay = 0.5, mix = 100, freeze = 0 })",
        "",
    );
    let mut g = Graph::build(&p, SR, &p.default_param_values());
    let mut x = sine(300.0, -12.0, 0.3);
    g.process_all(&[0.0; MACRO_SLOTS], &mut x);
    g.free_values()[p.param_index("r.freeze").unwrap()].store(1.0, Ordering::Relaxed);
    let mut tail = vec![[0.0; 2]; (2.0 * SR) as usize];
    g.process_all(&[0.0; MACRO_SLOTS], &mut tail);
    let a = rms_db(&tail[(0.2 * SR) as usize..(0.4 * SR) as usize], 0);
    let b = rms_db(&tail[(1.6 * SR) as usize..(1.8 * SR) as usize], 0);
    assert!(a > -60.0 && (a - b).abs() < 3.0, "frozen: {a} → {b}");
}

#[test]
fn shimmer_adds_the_octave() {
    let y = through("Blocks.Shimmer.new('s', { decay = 4, mix = 100, amount = 100 })", sine(400.0, -12.0, 2.0));
    assert!(all_finite(&y) && peak(&y, 0) < 4.0);
    let t = &y[(1.0 * SR) as usize..];
    assert!(tone_db(t, 800.0, 0) > -50.0, "{}", tone_db(t, 800.0, 0));
}

// ---------------------------------------------------------------- pitch

#[test]
fn pitch_shift_moves_the_tone() {
    let y = through("Blocks.PitchShift.new('p', { semitones = 12 })", sine(500.0, -12.0, 1.0));
    let t = &y[4800..];
    assert!(tone_db(t, 1000.0, 0) > tone_db(t, 500.0, 0) + 15.0, "{} vs {}", tone_db(t, 1000.0, 0), tone_db(t, 500.0, 0));
    let y = through("Blocks.PitchShift.new('p', { semitones = -12 })", sine(1000.0, -12.0, 1.0));
    let t = &y[4800..];
    assert!(tone_db(t, 500.0, 0) > tone_db(t, 1000.0, 0) + 15.0);
}

#[test]
fn granular_makes_a_cloud() {
    let y = through(
        "Blocks.Granular.new('g', { density = 40, pitch = 12, reverse = 50, mix = 100 })",
        sine(500.0, -12.0, 2.0),
    );
    assert!(all_finite(&y));
    let t = &y[SR as usize..];
    assert!(rms_db(t, 0) > -40.0 && peak(t, 0) < 2.0, "{}", rms_db(t, 0));
    assert!(tone_db(t, 1000.0, 0) > tone_db(t, 500.0, 0), "pitched up");
}

// ---------------------------------------------------------------- rhythm

#[test]
fn stutter_repeats_the_last_slice() {
    let p = load(
        "Blocks.Stutter.new('s', { active = 0, length = 50 })",
        "",
    );
    let mut g = Graph::build(&p, SR, &p.default_param_values());
    // A rising ramp, so every sample is different.
    let mut x: Vec<Frame> = (0..24_000).map(|i| [i as f32 / 24_000.0; 2]).collect();
    g.process_all(&[0.0; MACRO_SLOTS], &mut x[..12_000]);
    g.free_values()[p.param_index("s.active").unwrap()].store(1.0, Ordering::Relaxed);
    g.process_all(&[0.0; MACRO_SLOTS], &mut x[12_000..]);
    // After the switch (smoothed, one sub-block + fade), output repeats every 2400 samples.
    let slice = 2400;
    for i in 15_000..19_000 {
        assert!((x[i][0] - x[i + slice][0]).abs() < 1e-4, "frame {i}: {} vs {}", x[i][0], x[i + slice][0]);
    }
    assert!(x[16_000][0] < 0.5, "plays the past (values from before the trigger)");
}

// ---------------------------------------------------------------- dynamics

#[test]
fn limiter_holds_the_ceiling_with_lookahead_latency() {
    let g = graph("Blocks.Limiter.new('l', { ceiling = -1, lookahead = 2 })");
    assert_eq!(g.latency(), 96);
    let mut x: Vec<Frame> = sine(100.0, 0.0, 1.0).into_iter().map(|f| [f[0] * 4.0, f[1] * 4.0]).collect();
    let mut g = g;
    g.process_all(&[0.0; MACRO_SLOTS], &mut x);
    let ceil = 10f32.powf(-1.0 / 20.0);
    assert!(peak(&x, 0) <= ceil + 1e-6, "{}", peak(&x, 0));
    assert!(peak(&x[24_000..], 0) > ceil * 0.95, "reaches the ceiling");
    assert!(g.meters()[0].load(Ordering::Relaxed) > 0.4);
    // Distortion stays low: no clipping, the gain moves slowly on a 100 Hz sine.
    let t = &x[24_000..];
    assert!(tone_db(t, 300.0, 0) < tone_db(t, 100.0, 0) - 25.0);
}

#[test]
fn gate_closes_below_the_threshold() {
    let c = "Blocks.Gate.new('g', { threshold = -40 })";
    assert!(gain_through(c, 1000.0).abs() < 0.5, "-24 dB passes");
    let quiet = through(c, sine(1000.0, -60.0, 0.5));
    assert!(rms_db(&quiet[12_000..], 0) < -110.0, "{}", rms_db(&quiet[12_000..], 0));
    let mut g = graph(c);
    let mut x = sine(1000.0, -60.0, 0.5);
    g.process_all(&[0.0; MACRO_SLOTS], &mut x);
    assert!(g.meters()[0].load(Ordering::Relaxed) > 0.9);
}

#[test]
fn de_esser_cuts_only_the_highs() {
    let c = "Blocks.DeEsser.new('d', { threshold = -30, range = 12 })";
    let hi = {
        let x = sine(8000.0, -45.0, 0.5);
        let y = through(c, x.clone());
        rms_db(&y[12_000..], 0) - rms_db(&x[12_000..], 0)
    };
    let loud_hi = {
        let x = sine(8000.0, -6.0, 0.5);
        let y = through(c, x.clone());
        rms_db(&y[12_000..], 0) - rms_db(&x[12_000..], 0)
    };
    assert!(loud_hi < -6.0, "loud sibilance cut: {loud_hi}");
    assert!(hi.abs() < 1.0, "quiet highs pass: {hi}");
    let lo = {
        let x = sine(200.0, -6.0, 0.5);
        let y = through(c, x.clone());
        rms_db(&y[12_000..], 0) - rms_db(&x[12_000..], 0)
    };
    assert!(lo.abs() < 0.5, "lows untouched: {lo}");
}

// ---- the versatile compressor -----------------------------------------------------------------

/// Gain reduction (dB) on a 1 kHz sine at `db` dBFS once settled.
fn comp_gr(chain: &str, db: f32) -> f32 {
    let x = sine(1000.0, db, 1.0);
    let y = through(chain, x.clone());
    rms_db(&x[36_000..], 0) - rms_db(&y[36_000..], 0)
}

#[test]
fn compressor_feedback_keeps_the_dialled_ratio() {
    // -6 dBFS, threshold -20, 4:1, hard knee: 14 dB over → 10.5 dB reduction, either topology.
    let ff = comp_gr("Blocks.Compressor.new('c', { threshold = -20, ratio = 4, knee = 0 })", -6.0);
    let fb = comp_gr("Blocks.Compressor.new('c', { threshold = -20, ratio = 4, knee = 0, feedback = 100 })", -6.0);
    assert!((ff - 10.5).abs() < 1.0, "feed-forward {ff}");
    assert!((fb - 10.5).abs() < 1.0, "feedback {fb}");
    // A fast, hard feedback compressor stays stable (no blow-up, no NaN).
    let y = through("Blocks.Compressor.new('c', { threshold = -40, ratio = 100, attack = 0.05, feedback = 100 })", sine(100.0, 0.0, 0.5));
    assert!(all_finite(&y) && peak(&y, 0) < 1.0);
}

#[test]
fn compressor_ratio_curve_compresses_gently_first() {
    // Vari-mu: a little over the threshold it barely compresses, far over it reaches the ratio.
    let c = |curve: f32| format!("Blocks.Compressor.new('c', {{ threshold = -30, ratio = 6, knee = 0, ratio_curve = {curve} }})");
    let (fixed_low, vari_low) = (comp_gr(&c(0.0), -24.0), comp_gr(&c(100.0), -24.0));
    assert!(vari_low < fixed_low * 0.4, "6 dB over: fixed {fixed_low}, vari {vari_low}");
    // 30 dB over, the local ratio has reached 6:1; the reduction so far is its integral,
    // 15 · (1 − 1/6) = 12.5 dB. The peak detector follows the waveform, and with a rising ratio
    // the troughs of each cycle count for less: a little under that.
    let vari_high = comp_gr(&c(100.0), 0.0);
    assert!((10.5..=13.0).contains(&vari_high), "30 dB over: {vari_high}");
    // The output never falls as the input rises.
    let curve = |over: f32| over - dsp::comp_static(over, 6.0, 0.0, 1.0);
    assert!((0..60).all(|o| curve(o as f32 + 1.0) >= curve(o as f32)));
}

#[test]
fn compressor_link_decides_whether_a_loud_channel_ducks_the_other() {
    let loud_left = |link: f32| {
        let x: Vec<Frame> = sine(1000.0, -6.0, 1.0).into_iter().map(|f| [f[0], f[1] * 0.01]).collect();
        let y = through(&format!("Blocks.Compressor.new('c', {{ threshold = -30, ratio = 10, link = {link} }})"), x.clone());
        rms_db(&x[36_000..], 1) - rms_db(&y[36_000..], 1)
    };
    assert!(loud_left(0.0).abs() < 0.5, "unlinked: the quiet right channel is untouched");
    assert!(loud_left(100.0) > 15.0, "linked: it follows the left");
}

#[test]
fn compressor_lookahead_catches_the_onset_and_reports_its_latency() {
    let onset_peak = |extra: &str| {
        let mut x = vec![[0.0; 2]; 4800];
        x.extend(sine(1000.0, -6.0, 0.2));
        let g = graph(&format!("Blocks.Compressor.new('c', {{ threshold = -30, ratio = 20, attack = 2, knee = 0{extra} }})"));
        let lat = g.latency();
        let mut g = g;
        let mut y = x.clone();
        g.process_all(&[0.0; MACRO_SLOTS], &mut y);
        // The first 2 ms after the onset, wherever the latency put it.
        (peak(&y[4800 + lat..4800 + lat + 96], 0), lat)
    };
    let (without, lat0) = onset_peak("");
    let (with, lat) = onset_peak(", lookahead = 5");
    assert_eq!((lat0, lat), (0, 240));
    assert!(20.0 * (without / with).log10() > 6.0, "without {without}, with {with}");
}

#[test]
fn compressor_saturation_adds_even_harmonics() {
    let h2 = |sat: f32| {
        let y = through(&format!("Blocks.Compressor.new('c', {{ threshold = 0, saturation = {sat} }})"), sine(1000.0, -3.0, 0.5));
        tone_db(&y[12_000..], 2000.0, 0) - tone_db(&y[12_000..], 1000.0, 0)
    };
    assert!(h2(0.0) < -80.0 && h2(100.0) > -40.0, "clean {}, saturated {}", h2(0.0), h2(100.0));
}

#[test]
fn compressor_presets_load_and_compress() {
    for b in ["Compressor", "FETCompressor", "OptoCompressor", "VariMuCompressor"] {
        let gr = comp_gr(&format!("Blocks.{b}.new('c', {{ threshold = -30 }})"), -6.0);
        assert!(gr > 6.0, "{b}: {gr}");
    }
}

#[test]
fn dynamics_lookahead_is_latency() {
    for b in ["Gate", "DeEsser", "Transient", "FETCompressor"] {
        assert_eq!(graph(&format!("Blocks.{b}.new('x', {{ lookahead = 2 }})")).latency(), 96, "{b}");
    }
}

// ---- linear phase -----------------------------------------------------------------------------

/// `y` against `x` delayed by `lat` and scaled by `g`: the largest difference after `from`.
fn delayed_error(x: &[Frame], y: &[Frame], lat: usize, g: f32, from: usize) -> f32 {
    (from..x.len()).map(|i| (y[i][0] - g * x[i - lat][0]).abs()).fold(0.0, f32::max)
}

#[test]
fn linear_phase_eq_matches_the_magnitude_without_phase_shift() {
    let lin = "Blocks.PeakEq.new('e', { freq = 1000, q = 1, gain = 6, phase = 'linear' })";
    let min = "Blocks.PeakEq.new('e', { freq = 1000, q = 1, gain = 6 })";
    let lat = graph(lin).latency();
    assert_eq!(lat, 4096 + 256);
    for f in [100.0, 700.0, 1000.0, 5000.0] {
        // The same gain as the minimum-phase EQ…
        let (gl, gm) = (gain_through(lin, f), gain_through(min, f));
        assert!((gl - gm).abs() < 0.1, "{f} Hz: linear {gl}, minimum {gm}");
        // …and the output is the input, delayed by the latency and scaled: no phase shift.
        let x = sine(f, -12.0, 0.5);
        let y = through(lin, x.clone());
        let g = 10f32.powf(gl / 20.0);
        let err = delayed_error(&x, &y, lat, g, 12_000);
        assert!(err < 0.003, "{f} Hz: {err}");
    }
}

#[test]
fn linear_phase_high_pass_keeps_its_slope() {
    let c = |slope: f32| format!("Blocks.HighPass.new('h', {{ freq = 1000, slope = {slope}, phase = 'linear' }})");
    assert!((gain_through(&c(24.0), 1000.0) + 3.0).abs() < 0.3);
    assert!((gain_through(&c(24.0), 250.0) + 48.0).abs() < 3.0, "{}", gain_through(&c(24.0), 250.0));
}

#[test]
fn linear_phase_band_split_reconstructs_exactly() {
    let split = "Blocks.BandSplit.new('s', { crossovers = { 120, 800, 4000 }, phase = 'linear',
        bands = { Chain.new(), Chain.new(), Chain.new(), Chain.new() } })";
    let lat = graph(split).latency();
    let x: Vec<Frame> = (0..24_000).map(|i| {
        let t = i as f32 / SR;
        let s = 0.3 * (6.28 * 90.0 * t).sin() + 0.2 * (6.28 * 1300.0 * t).sin() + 0.1 * (6.28 * 7000.0 * t).sin();
        [s, s]
    }).collect();
    let y = through(split, x.clone());
    let err = delayed_error(&x, &y, lat, 1.0, lat + 1);
    assert!(err < 1e-3, "{err}");
    // Muting the middle bands takes out 800 Hz–4 kHz and keeps the outside.
    let muted = "Blocks.BandSplit.new('s', { crossovers = { 200, 3000 }, phase = 'linear',
        bands = { Chain.new(), Chain.new { Blocks.Gain.new('m', { db = -120 }) }, Chain.new() } })";
    assert!(gain_through(muted, 800.0) < -20.0);
    assert!(gain_through(muted, 50.0).abs() < 0.5 && gain_through(muted, 12000.0).abs() < 0.5);
}
