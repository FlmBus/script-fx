//! Native DSP for declarative patch chains.
//!
//! [`Graph::build`] compiles a patch's `chain` (off the audio thread) into a tree
//! of nodes. [`Graph::process`] runs it on the audio thread: no allocations, no
//! locks, no Lua. Settings are recomputed once per sub-block of at most
//! [`SUB_BLOCK`] frames (from macros + free param values) and smoothed.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use atomic_float::AtomicF32;
use patch::chain::{BlockSpec, CompStyle, EqShape, Value, ValueRef, resolve_value};
use patch::{Control, MACRO_SLOTS, ParamSpec, Patch};

/// Maximum frames per [`Graph::process`] call.
pub const SUB_BLOCK: usize = 32;
/// Time constant for smoothing block settings.
const SETTING_SMOOTHING_MS: f32 = 20.0;

pub type Frame = [f32; 2];

/// Shared, lock-free value arrays (editor ↔ audio thread).
pub type Atomics = Arc<[AtomicF32]>;

fn atomics(values: impl IntoIterator<Item = f32>) -> Atomics {
    values.into_iter().map(AtomicF32::new).collect::<Vec<_>>().into()
}

#[inline]
fn db_to_gain(db: f32) -> f32 {
    (db * (std::f32::consts::LN_10 / 20.0)).exp()
}

#[inline]
fn gain_to_db(g: f32) -> f32 {
    20.0 * g.max(1e-9).log10()
}

/// One-pole coefficient for a time constant in ms at `rate` Hz.
#[inline]
fn coef(ms: f32, rate: f32) -> f32 {
    (-1.0 / (ms.max(0.01) * 0.001 * rate)).exp()
}

/// Per-sub-block context shared by all nodes.
struct Ctx<'a> {
    values: &'a [f32],
    sr: f32,
    /// Setting smoothing factor per sub-block.
    k: f32,
    meters: &'a [AtomicF32],
}

/// A smoothed block setting.
#[derive(Clone, Copy)]
struct P {
    src: ValueRef,
    cur: f32,
    init: bool,
}

impl P {
    fn new(v: Option<&Value>, default: f32, params: &[ParamSpec]) -> Self {
        let src = match v {
            Some(v) => resolve_value(v, params).expect("validated by patch"),
            None => ValueRef::Const(default),
        };
        P { src, cur: default, init: false }
    }

    fn update(&mut self, ctx: &Ctx) -> f32 {
        let target = match self.src {
            ValueRef::Const(c) => c,
            ValueRef::Param(i) => ctx.values[i],
        };
        if self.init {
            self.cur += ctx.k * (target - self.cur);
        } else {
            self.cur = target;
            self.init = true;
        }
        self.cur
    }
}

#[derive(Clone, Copy, Default)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: [f32; 2],
    z2: [f32; 2],
}

impl Biquad {
    /// RBJ cookbook coefficients.
    fn set(&mut self, shape: EqShape, freq: f32, q: f32, gain_db: f32, sr: f32) {
        let w0 = std::f32::consts::TAU * freq.clamp(10.0, sr * 0.49) / sr;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q.max(0.05));
        let a = 10f32.powf(gain_db / 40.0);
        let sa = 2.0 * a.sqrt() * alpha;
        let (b0, b1, b2, a0, a1, a2) = match shape {
            EqShape::Peak => (1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a),
            EqShape::LowShelf => (
                a * ((a + 1.0) - (a - 1.0) * cos + sa),
                2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                a * ((a + 1.0) - (a - 1.0) * cos - sa),
                (a + 1.0) + (a - 1.0) * cos + sa,
                -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                (a + 1.0) + (a - 1.0) * cos - sa,
            ),
            EqShape::HighShelf => (
                a * ((a + 1.0) + (a - 1.0) * cos + sa),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                a * ((a + 1.0) + (a - 1.0) * cos - sa),
                (a + 1.0) - (a - 1.0) * cos + sa,
                2.0 * ((a - 1.0) - (a + 1.0) * cos),
                (a + 1.0) - (a - 1.0) * cos - sa,
            ),
            EqShape::LowPass => ((1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
            EqShape::HighPass => ((1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
        };
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    #[inline]
    fn tick(&mut self, x: f32, ch: usize) -> f32 {
        let y = self.b0 * x + self.z1[ch];
        self.z1[ch] = self.b1 * x - self.a1 * y + self.z2[ch];
        self.z2[ch] = self.b2 * x - self.a2 * y;
        y
    }
}

struct Comp {
    style: CompStyle,
    threshold: P,
    ratio: P,
    attack: P,
    release: P,
    knee: P,
    makeup: P,
    mix: P,
    auto_makeup: bool,
    /// Current gain reduction, dB (≥ 0).
    env: f32,
    rms: f32,
    /// Opto: how long/much it has been compressing (0..1), slows the release.
    sustain: f32,
    /// Slow average of the gain reduction (dB), for adaptive auto makeup.
    avg_gr: f32,
    meter: usize,
}

struct Rider {
    target: P,
    range: P,
    speed: P,
    rms: f32,
    gain_db: f32,
    meter: usize,
}

struct Transient {
    attack: P,
    sustain: P,
    fast: f32,
    slow: f32,
    gain_db: f32,
    meter: usize,
}

enum Node {
    Gain { db: P, gain: f32 },
    Eq { shape: EqShape, freq: P, q: P, gain: P, last: [f32; 3], bq: Biquad },
    Drive { amount: P, mix: P },
    Comp(Box<Comp>),
    Rider(Box<Rider>),
    Transient(Box<Transient>),
    Split { freq: P, last: f32, lp: [Biquad; 2], hp: [Biquad; 2], low: Vec<Node>, high: Vec<Node> },
}

fn build_nodes(chain: &[BlockSpec], params: &[ParamSpec], meter: &mut usize) -> Vec<Node> {
    let p = |v: &Option<Value>, d: f32| P::new(v.as_ref(), d, params);
    fn next_meter(meter: &mut usize) -> usize {
        let m = *meter;
        *meter += 1;
        m
    }
    let mut out = Vec::with_capacity(chain.len());
    for b in chain {
        out.push(match b {
            BlockSpec::Gain { db, .. } => Node::Gain { db: P::new(Some(db), 0.0, params), gain: 1.0 },
            BlockSpec::Eq { shape, freq, q, gain, .. } => Node::Eq {
                shape: *shape,
                freq: P::new(Some(freq), 1000.0, params),
                q: p(q, std::f32::consts::FRAC_1_SQRT_2),
                gain: p(gain, 0.0),
                last: [f32::NAN; 3],
                bq: Biquad::default(),
            },
            BlockSpec::Drive { amount, mix, .. } => Node::Drive { amount: P::new(Some(amount), 0.0, params), mix: p(mix, 100.0) },
            BlockSpec::Comp { style, threshold, ratio, attack, release, knee, makeup, auto_makeup, mix, .. } => {
                let (r, a, rel, k) = match style {
                    CompStyle::Fet => (4.0, 0.3, 80.0, 2.0),
                    CompStyle::Opto => (3.0, 10.0, 250.0, 8.0),
                    CompStyle::Clean => (2.0, 5.0, 100.0, 4.0),
                };
                Node::Comp(Box::new(Comp {
                    style: *style,
                    threshold: P::new(Some(threshold), 0.0, params),
                    ratio: p(ratio, r),
                    attack: p(attack, a),
                    release: p(release, rel),
                    knee: p(knee, k),
                    makeup: p(makeup, 0.0),
                    mix: p(mix, 100.0),
                    auto_makeup: *auto_makeup,
                    env: 0.0,
                    rms: 0.0,
                    sustain: 0.0,
                    avg_gr: 0.0,
                    meter: next_meter(meter),
                }))
            }
            BlockSpec::Rider { target, range, speed, .. } => Node::Rider(Box::new(Rider {
                target: P::new(Some(target), -18.0, params),
                range: P::new(Some(range), 6.0, params),
                speed: p(speed, 300.0),
                rms: 0.0,
                gain_db: 0.0,
                meter: next_meter(meter),
            })),
            BlockSpec::Transient { attack, sustain, .. } => Node::Transient(Box::new(Transient {
                attack: P::new(Some(attack), 0.0, params),
                sustain: p(sustain, 0.0),
                fast: 0.0,
                slow: 0.0,
                gain_db: 0.0,
                meter: next_meter(meter),
            })),
            BlockSpec::Split { freq, low, high, .. } => {
                // Same depth-first order as `patch::chain::walk`: low, then high.
                let low = build_nodes(low, params, meter);
                let high = build_nodes(high, params, meter);
                Node::Split {
                    freq: P::new(Some(freq), 250.0, params),
                    last: f32::NAN,
                    lp: [Biquad::default(); 2],
                    hp: [Biquad::default(); 2],
                    low,
                    high,
                }
            }
        });
    }
    out
}

fn process_nodes(nodes: &mut [Node], frames: &mut [Frame], ctx: &Ctx) {
    for n in nodes {
        n.process(frames, ctx);
    }
}

impl Node {
    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let sr = ctx.sr;
        match self {
            Node::Gain { db, gain } => {
                let target = db_to_gain(db.update(ctx));
                let step = (target - *gain) / frames.len() as f32;
                for f in frames {
                    *gain += step;
                    f[0] *= *gain;
                    f[1] *= *gain;
                }
            }
            Node::Eq { shape, freq, q, gain, last, bq } => {
                let cur = [freq.update(ctx), q.update(ctx), gain.update(ctx)];
                if cur != *last {
                    bq.set(*shape, cur[0], cur[1], cur[2], sr);
                    *last = cur;
                }
                for f in frames {
                    f[0] = bq.tick(f[0], 0);
                    f[1] = bq.tick(f[1], 1);
                }
            }
            Node::Drive { amount, mix } => {
                let g = db_to_gain(amount.update(ctx));
                let comp = 1.0 / g.sqrt();
                let wet = (mix.update(ctx) / 100.0).clamp(0.0, 1.0);
                for f in frames {
                    for x in f.iter_mut() {
                        let y = (*x * g).tanh() * comp;
                        *x += wet * (y - *x);
                    }
                }
            }
            Node::Comp(c) => c.process(frames, ctx),
            Node::Rider(r) => r.process(frames, ctx),
            Node::Transient(t) => t.process(frames, ctx),
            Node::Split { freq, last, lp, hp, low, high } => {
                let f0 = freq.update(ctx);
                if f0 != *last {
                    for (l, h) in lp.iter_mut().zip(hp.iter_mut()) {
                        l.set(EqShape::LowPass, f0, std::f32::consts::FRAC_1_SQRT_2, 0.0, sr);
                        h.set(EqShape::HighPass, f0, std::f32::consts::FRAC_1_SQRT_2, 0.0, sr);
                    }
                    *last = f0;
                }
                let n = frames.len();
                let mut lo = [[0.0f32; 2]; SUB_BLOCK];
                let mut hi = [[0.0f32; 2]; SUB_BLOCK];
                for i in 0..n {
                    for ch in 0..2 {
                        let x = frames[i][ch];
                        let l = lp[0].tick(x, ch);
                        lo[i][ch] = lp[1].tick(l, ch);
                        let h = hp[0].tick(x, ch);
                        hi[i][ch] = hp[1].tick(h, ch);
                    }
                }
                process_nodes(low, &mut lo[..n], ctx);
                process_nodes(high, &mut hi[..n], ctx);
                for i in 0..n {
                    frames[i] = [lo[i][0] + hi[i][0], lo[i][1] + hi[i][1]];
                }
            }
        }
    }
}

impl Comp {
    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let sr = ctx.sr;
        let thr = self.threshold.update(ctx);
        let ratio = self.ratio.update(ctx).max(1.0);
        let knee = self.knee.update(ctx).max(0.0);
        let makeup_db = self.makeup.update(ctx);
        let wet = (self.mix.update(ctx) / 100.0).clamp(0.0, 1.0);
        let att = coef(self.attack.update(ctx), sr);
        let mut release_ms = self.release.update(ctx);
        if self.style == CompStyle::Opto {
            // Program-dependent: the longer/harder it compressed, the slower it lets go.
            release_ms *= 1.0 + 4.0 * self.sustain;
        }
        let rel = coef(release_ms, sr);
        let rms_c = coef(10.0, sr);
        let sus_c = coef(1500.0, sr);
        // Adaptive auto makeup: give back most of the *average* gain reduction
        // (level-independent, unlike the static threshold/ratio formula).
        let avg_c = coef(1500.0, sr);
        let auto = if self.auto_makeup { 0.8 } else { 0.0 };
        let slope = 1.0 - 1.0 / ratio;

        for f in frames {
            let x = f[0].abs().max(f[1].abs());
            let level = match self.style {
                CompStyle::Opto => {
                    self.rms = rms_c * self.rms + (1.0 - rms_c) * x * x;
                    10.0 * self.rms.max(1e-18).log10()
                }
                _ => gain_to_db(x),
            };
            let over = level - thr;
            let gr = if over <= -knee / 2.0 {
                0.0
            } else if over >= knee / 2.0 || knee == 0.0 {
                over * slope
            } else {
                slope * (over + knee / 2.0).powi(2) / (2.0 * knee)
            };
            let c = if gr > self.env { att } else { rel };
            self.env = c * self.env + (1.0 - c) * gr;
            if self.style == CompStyle::Opto {
                self.sustain = sus_c * self.sustain + (1.0 - sus_c) * (self.env / 12.0).min(1.0);
            }
            self.avg_gr = avg_c * self.avg_gr + (1.0 - avg_c) * self.env;
            let g = db_to_gain(-self.env + makeup_db + auto * self.avg_gr);
            for x in f.iter_mut() {
                *x += wet * (*x * g - *x);
            }
        }
        ctx.meters[self.meter].store((self.env / 24.0).clamp(0.0, 1.0), Ordering::Relaxed);
    }
}

impl Rider {
    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let target = self.target.update(ctx);
        let range = self.range.update(ctx).max(0.0);
        let speed = self.speed.update(ctx);
        let det = coef(speed, ctx.sr);
        let move_c = coef(speed * 2.0, ctx.sr);
        for f in frames {
            let x = 0.5 * (f[0] * f[0] + f[1] * f[1]);
            self.rms = det * self.rms + (1.0 - det) * x;
            let level = 10.0 * self.rms.max(1e-18).log10();
            // Don't ride up silence/noise: hold the current gain below the gate.
            if level > target - 40.0 {
                let desired = (target - level).clamp(-range, range);
                self.gain_db = move_c * self.gain_db + (1.0 - move_c) * desired;
            }
            let g = db_to_gain(self.gain_db.clamp(-range, range));
            f[0] *= g;
            f[1] *= g;
        }
        ctx.meters[self.meter].store((0.5 + self.gain_db / 24.0).clamp(0.0, 1.0), Ordering::Relaxed);
    }
}

impl Transient {
    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let sr = ctx.sr;
        let attack = self.attack.update(ctx);
        let sustain = self.sustain.update(ctx);
        let (fa, fr) = (coef(0.5, sr), coef(30.0, sr));
        let (sa, sr_) = (coef(25.0, sr), coef(200.0, sr));
        let gs = coef(1.0, sr);
        for f in frames {
            let x = f[0].abs().max(f[1].abs());
            let c = if x > self.fast { fa } else { fr };
            self.fast = c * self.fast + (1.0 - c) * x;
            let c = if x > self.slow { sa } else { sr_ };
            self.slow = c * self.slow + (1.0 - c) * x;
            let diff = gain_to_db(self.fast) - gain_to_db(self.slow);
            let target = attack * (diff / 6.0).clamp(0.0, 1.0) + sustain * (-diff / 6.0).clamp(0.0, 1.0);
            self.gain_db = gs * self.gain_db + (1.0 - gs) * target;
            let g = db_to_gain(self.gain_db);
            f[0] *= g;
            f[1] *= g;
        }
        ctx.meters[self.meter].store((0.5 + self.gain_db / 24.0).clamp(0.0, 1.0), Ordering::Relaxed);
    }
}

/// A compiled chain. Build off the audio thread, then move it to the audio thread.
pub struct Graph {
    nodes: Vec<Node>,
    controls: Vec<Control>,
    free: Atomics,
    meters: Atomics,
    values: Vec<f32>,
    sr: f32,
    k: f32,
}

impl Graph {
    /// `free` = plain values of all params (only free ones are used).
    pub fn build(patch: &Patch, sample_rate: f32, free: &[f32]) -> Self {
        let mut meter = 0;
        let nodes = build_nodes(&patch.chain, &patch.params, &mut meter);
        debug_assert_eq!(meter, patch.meters.len(), "meter order must match patch::chain::walk");
        // Idle meter values: comps show 0 dB reduction, gain meters sit at unity (0.5).
        let mut idle = Vec::with_capacity(meter);
        patch::chain::walk(&patch.chain, &mut |b| {
            if b.has_meter() {
                idle.push(if matches!(b, BlockSpec::Comp { .. }) { 0.0 } else { 0.5 });
            }
        });
        let sub_rate = sample_rate / SUB_BLOCK as f32;
        Graph {
            nodes,
            controls: patch.controls.clone(),
            free: atomics(free.iter().copied()),
            meters: atomics(idle),
            values: vec![0.0; patch.params.len()],
            sr: sample_rate,
            k: 1.0 - coef(SETTING_SMOOTHING_MS, sub_rate),
        }
    }

    /// Free param values (plain), writable from the editor.
    pub fn free_values(&self) -> Atomics {
        self.free.clone()
    }

    /// Block meters (index = `Patch::meters`), readable from the editor.
    pub fn meters(&self) -> Atomics {
        self.meters.clone()
    }

    /// Processes up to [`SUB_BLOCK`] frames in place. Real-time safe.
    pub fn process(&mut self, macros: &[f32; MACRO_SLOTS], frames: &mut [Frame]) {
        debug_assert!(frames.len() <= SUB_BLOCK);
        for (i, c) in self.controls.iter().enumerate() {
            self.values[i] = match *c {
                Control::Free => self.free[i].load(Ordering::Relaxed),
                Control::Macro { slot, from, to, curve } => curve.apply(from, to, macros[slot]),
            };
        }
        let ctx = Ctx { values: &self.values, sr: self.sr, k: self.k, meters: &self.meters };
        process_nodes(&mut self.nodes, frames, &ctx);
    }

    /// Convenience for offline use/tests: processes any number of frames.
    pub fn process_all(&mut self, macros: &[f32; MACRO_SLOTS], frames: &mut [Frame]) {
        for chunk in frames.chunks_mut(SUB_BLOCK) {
            self.process(macros, chunk);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const SR: f32 = 48_000.0;

    fn patch_with(chain: &str, params: &str) -> (Patch, TempDir) {
        let d = TempDir::new();
        std::fs::write(
            d.0.join("patch.lua"),
            format!("return {{ name = 'T', params = {{ {params} }}, chain = {{ {chain} }} }}"),
        )
        .unwrap();
        (Patch::load(&d.0).unwrap(), d)
    }

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            use std::sync::atomic::AtomicU32;
            static N: AtomicU32 = AtomicU32::new(0);
            let p = std::env::temp_dir()
                .join(format!("dsp-test-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir_all(&p).unwrap();
            TempDir(p)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn sine(freq: f32, db: f32, secs: f32) -> Vec<Frame> {
        let a = db_to_gain(db);
        (0..(secs * SR) as usize)
            .map(|i| {
                let x = a * (std::f32::consts::TAU * freq * i as f32 / SR).sin();
                [x, x]
            })
            .collect()
    }

    /// RMS in dB of the last `secs` seconds (left channel).
    fn tail_db(frames: &[Frame], secs: f32) -> f32 {
        let n = (secs * SR) as usize;
        let tail = &frames[frames.len() - n..];
        let ms = tail.iter().map(|f| f[0] * f[0]).sum::<f32>() / n as f32;
        10.0 * ms.log10()
    }

    fn run(chain: &str, params: &str, input: &mut [Frame]) -> Graph {
        let (p, _d) = patch_with(chain, params);
        let mut g = Graph::build(&p, SR, &p.default_param_values());
        g.process_all(&[0.0; MACRO_SLOTS], input);
        g
    }

    #[test]
    fn crossover_sums_flat() {
        for freq in [60.0, 250.0, 1000.0, 5000.0] {
            let mut x = sine(freq, -12.0, 0.5);
            let reference = tail_db(&x, 0.2);
            run("split('s', { freq = 250, low = {}, high = {} })", "", &mut x);
            let diff = tail_db(&x, 0.2) - reference;
            assert!(diff.abs() < 0.05, "{freq} Hz: {diff} dB");
        }
    }

    #[test]
    fn peak_eq_boosts_at_center() {
        let mut x = sine(1000.0, -20.0, 0.5);
        let reference = tail_db(&x, 0.2);
        run("eq('e', { type = 'peak', freq = 1000, q = 1, gain = 6 })", "", &mut x);
        assert!((tail_db(&x, 0.2) - reference - 6.0).abs() < 0.1);
    }

    #[test]
    fn fet_comp_reduces_by_ratio() {
        // Sine peak at -6 dBFS, threshold -20, ratio 4, hard knee → ~10.5 dB GR.
        let mut x = sine(1000.0, -6.0, 1.0);
        let reference = tail_db(&x, 0.3);
        let g = run("comp('c', { style = 'fet', threshold = -20, ratio = 4, knee = 0 })", "", &mut x);
        let gr = reference - tail_db(&x, 0.3);
        assert!((gr - 10.5).abs() < 1.0, "gr {gr}");
        let meter = g.meters()[0].load(Ordering::Relaxed) * 24.0;
        assert!((meter - 10.5).abs() < 1.0, "meter {meter}");
    }

    #[test]
    fn rider_lifts_quiet_signal_within_range() {
        let mut x = sine(100.0, -30.0, 4.0);
        let reference = tail_db(&x, 0.5);
        run("rider('r', { target = -18, range = 6, speed = 100 })", "", &mut x);
        let lift = tail_db(&x, 0.5) - reference;
        assert!((lift - 6.0).abs() < 0.5, "lift {lift}");
    }

    #[test]
    fn param_refs_follow_macros() {
        let (p, _d) = {
            let d = TempDir::new();
            std::fs::write(
                d.0.join("patch.lua"),
                "return { name = 'T', params = { { id = 'g', name = 'G', min = -24, max = 0 } },
                  macros = { { id = 'm', name = 'M', targets = { { param = 'g' } } } },
                  chain = { gain('out', { db = 'param.g' }) } }",
            )
            .unwrap();
            (Patch::load(&d.0).unwrap(), d)
        };
        let mut g = Graph::build(&p, SR, &p.default_param_values());
        let mut macros = [0.0; MACRO_SLOTS];
        macros[0] = 0.5; // -12 dB
        let mut x = sine(1000.0, -6.0, 1.0);
        let reference = tail_db(&x, 0.2);
        g.process_all(&macros, &mut x);
        assert!((reference - tail_db(&x, 0.2) - 12.0).abs() < 0.1);
    }

    #[test]
    fn bundled_patches_build() {
        let root = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../patches"));
        for dir in patch::discover(&root) {
            let p = Patch::load(&dir).unwrap();
            let mut g = Graph::build(&p, SR, &p.default_param_values());
            let mut x = sine(110.0, -12.0, 0.2);
            g.process_all(&[0.5; MACRO_SLOTS], &mut x);
            assert!(x.iter().all(|f| f[0].is_finite()), "{}", dir.display());
        }
    }
}
