//! Native DSP for declarative patch chains.
//!
//! [`Graph::build`] compiles a patch's `chain` (off the audio thread) into a tree
//! of nodes. [`Graph::process`] runs it on the audio thread: no allocations, no
//! locks, no Lua. Settings are recomputed once per sub-block of at most
//! [`SUB_BLOCK`] frames (from macros + free param values) and smoothed.
//!
//! Oversampling is generic: any block can be wrapped in an oversampled node
//! (`oversampling = n` on the block; nonlinear blocks automatically). The block
//! just sees a higher `Ctx::sr`; none of them oversamples on its own. The filters
//! are linear-phase, so oversampled blocks have latency: every node reports it,
//! routing blocks delay their shorter branches to line them up, and the graph
//! reports the total ([`Graph::latency`]) for the host's delay compensation.

mod fx;
pub mod modulators;
mod oversample;
pub mod preview;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use atomic_float::AtomicF32;
use patch::chain::{BlockKind, BlockSpec, Chain, Channels, CompSpec, EqSpec, FilterSpec, ParamRef, Side};

use oversample::{Delay, Oversampler};
use patch::{MACRO_SLOTS, ModDestination, Modulation, ModulatorId, ModulatorLfoShape, ModulatorSettings, ParamSpec, Patch};

/// Maximum frames per [`Graph::process`] call.
pub const SUB_BLOCK: usize = 32;
/// Highest oversampling factor (also for nested oversampled blocks multiplied).
pub const MAX_OVERSAMPLING: usize = patch::chain::MAX_OVERSAMPLING as usize;
/// Automatic oversampling of nonlinear blocks aims for at least this rate (Hz).
const AUTO_OVERSAMPLED_RATE: f32 = 88_000.0;
/// Longest Haas delay, ms.
const HAAS_MAX_MS: f32 = 40.0;
/// Peak meter fall: to a quarter in this time (same as the shell's in/out meters).
const PEAK_DECAY_MS: f32 = 300.0;

/// Time constant for smoothing block settings.
const SETTING_SMOOTHING_MS: f32 = 20.0;

pub type Frame = [f32; 2];

/// Shared, lock-free value arrays (editor ↔ audio thread).
pub type Atomics = Arc<[AtomicF32]>;
/// Packed modulation settings, shared with the editor.
pub type ModAtomics = Arc<[AtomicU64]>;

fn atomics(values: impl IntoIterator<Item = f32>) -> Atomics {
    values.into_iter().map(AtomicF32::new).collect::<Vec<_>>().into()
}

#[inline]
pub(crate) fn db_to_gain(db: f32) -> f32 {
    (db * (std::f32::consts::LN_10 / 20.0)).exp()
}

#[inline]
pub(crate) fn gain_to_db(g: f32) -> f32 {
    20.0 * g.max(1e-9).log10()
}

/// One-pole coefficient for a time constant in ms at `rate` Hz.
#[inline]
pub(crate) fn coef(ms: f32, rate: f32) -> f32 {
    (-1.0 / (ms.max(0.01) * 0.001 * rate)).exp()
}

/// Per-sub-block context shared by all nodes.
#[derive(Clone, Copy)]
pub(crate) struct Ctx<'a> {
    pub(crate) values: &'a [f32],
    pub(crate) sr: f32,
    /// Setting smoothing factor per sub-block.
    pub(crate) k: f32,
    pub(crate) meters: &'a [AtomicF32],
    /// Host tempo, BPM (120 when the host doesn't say).
    pub(crate) tempo: f32,
    /// Host position in beats (quarter notes) at the first frame of this call;
    /// `None` when the host isn't playing (or doesn't say).
    pub(crate) beat: Option<f64>,
}

/// A smoothed block setting: a param's value, or a fixed one the DSP sets itself.
#[derive(Clone, Copy)]
pub(crate) struct P {
    src: Src,
    cur: f32,
    init: bool,
}

#[derive(Clone, Copy)]
enum Src {
    Const(f32),
    Param(usize),
}

impl P {
    /// Follows a param (a block setting).
    pub(crate) fn param(r: ParamRef) -> Self {
        P { src: Src::Param(r.0), cur: 0.0, init: false }
    }

    /// A fixed value (e.g. the 0 dB gain of a plain filter).
    pub(crate) fn constant(c: f32) -> Self {
        P { src: Src::Const(c), cur: c, init: false }
    }

    pub(crate) fn update(&mut self, ctx: &Ctx) -> f32 {
        let target = match self.src {
            Src::Const(c) => c,
            Src::Param(i) => ctx.values[i],
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum EqShape {
    Peak,
    LowShelf,
    HighShelf,
    LowPass,
    HighPass,
    /// Constant 0 dB peak gain band-pass.
    BandPass,
    /// 2nd-order allpass; equals LR4 low + high at the same frequency
    /// (used to phase-align the bands of a [`BandSplit`]).
    AllPass,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
enum CompStyle {
    Fet,
    Opto,
    #[default]
    Clean,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct Biquad {
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
    pub(crate) fn set(&mut self, shape: EqShape, freq: f32, q: f32, gain_db: f32, sr: f32) {
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
            EqShape::AllPass => (1.0 - alpha, -2.0 * cos, 1.0 + alpha, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
            EqShape::BandPass => (alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
        };
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    /// Forgets the signal (a stage that was off and comes back on).
    pub(crate) fn clear(&mut self) {
        self.z1 = [0.0; 2];
        self.z2 = [0.0; 2];
    }

    #[inline]
    pub(crate) fn tick(&mut self, x: f32, ch: usize) -> f32 {
        let y = self.b0 * x + self.z1[ch];
        self.z1[ch] = self.b1 * x - self.a1 * y + self.z2[ch];
        self.z2[ch] = self.b2 * x - self.a2 * y;
        y
    }
}

/// Most biquad stages of a low/high-pass: 48 dB/oct.
pub(crate) const FILTER_STAGES: usize = 4;

/// A low/high-pass of `slope` dB/oct (12, 24, 36 or 48) as cascaded biquads: how many stages,
/// and each stage's Q. The stages get Butterworth Q's (flat passband); `q` is the resonance,
/// applied to the steepest stage (0.707 = Butterworth, more = a peak at the cutoff).
pub(crate) fn filter_stages(slope: f32, q: f32) -> (usize, [f32; FILTER_STAGES]) {
    let n = ((slope / 12.0).round() as usize).clamp(1, FILTER_STAGES);
    let order = 2 * n;
    let mut qs = [0.0; FILTER_STAGES];
    for (k, s) in qs.iter_mut().take(n).enumerate() {
        let theta = (2 * k + 1) as f32 * std::f32::consts::PI / (2 * order) as f32;
        *s = 1.0 / (2.0 * theta.cos());
    }
    // The last stage has the highest Q: it carries the resonance.
    qs[n - 1] *= q.max(0.05) / std::f32::consts::FRAC_1_SQRT_2;
    (n, qs)
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

/// LR4 = two cascaded Butterworth (q 0.707) biquads.
#[derive(Clone, Copy, Default)]
struct Lr4([Biquad; 2]);

impl Lr4 {
    fn set(&mut self, shape: EqShape, freq: f32, sr: f32) {
        for b in &mut self.0 {
            b.set(shape, freq, std::f32::consts::FRAC_1_SQRT_2, 0.0, sr);
        }
    }
    #[inline]
    fn tick(&mut self, x: f32, ch: usize) -> f32 {
        let y = self.0[0].tick(x, ch);
        self.0[1].tick(y, ch)
    }
}

/// N-band Linkwitz-Riley split. Band 0 = LP(f0); band i = LP(fi) of what passed
/// HP(f0..fi-1); the last band = everything above the last crossover. Each band
/// also passes the allpasses of the crossovers it didn't go through, so all bands
/// share the same phase and their sum is flat.
struct BandSplit {
    freqs: Vec<P>,
    last: Vec<f32>,
    lp: Vec<Lr4>,
    hp: Vec<Lr4>,
    /// `ap[i]` = allpasses of band i at crossovers i+1..
    ap: Vec<Vec<Biquad>>,
    bands: Vec<Vec<Node>>,
    bufs: Vec<Vec<Frame>>,
    /// Latency compensation per band.
    align: Vec<Delay>,
    latency: usize,
}

struct Parallel {
    branches: Vec<Vec<Node>>,
    bufs: Vec<Vec<Frame>>,
    align: Vec<Delay>,
    latency: usize,
    /// Crossfade between exactly two branches, %.
    mix: Option<P>,
}

fn frame_buf(max_frames: usize) -> Vec<Frame> {
    vec![[0.0; 2]; max_frames]
}

pub(crate) fn chain_latency(nodes: &[Node]) -> usize {
    nodes.iter().map(Node::latency).sum()
}

/// Delays that line up chains with different latencies; returns them and the
/// common latency (the longest).
fn align(chains: &[Vec<Node>]) -> (Vec<Delay>, usize) {
    let lat: Vec<usize> = chains.iter().map(|c| chain_latency(c)).collect();
    let max = lat.iter().copied().max().unwrap_or(0);
    (lat.iter().map(|l| Delay::new(max - l)).collect(), max)
}

/// A run of blocks at `factor` × the rate around them (one up/down pair).
struct Oversampled {
    os: Oversampler,
    inner: Vec<Node>,
}

struct Haas {
    delay: P,
    /// Delayed channel.
    ch: usize,
    buf: Vec<f32>,
    pos: usize,
    /// Current delay, samples.
    cur: f32,
}

#[derive(Clone, Copy, PartialEq)]
enum SplitMode {
    MidSide,
    LeftRight,
}

/// Two mono sub-chains: mid/side or left/right. Each chain sees its signal on
/// both channels (so channel-linked blocks like compressors behave as mono);
/// the left channel of the result is used.
struct Split2 {
    mode: SplitMode,
    chains: [Vec<Node>; 2],
    bufs: [Vec<Frame>; 2],
    align: Vec<Delay>,
    latency: usize,
}

pub(crate) enum Node {
    Gain { db: P, gain: f32 },
    Pan { pan: P, gains: [f32; 2] },
    Width { width: P, w: f32 },
    /// Side → LR4 high-pass; mid → the matching allpass (= LR4 low + high), so
    /// mid and side keep the same phase above the cutoff and the image stays put.
    MonoBass { freq: P, last: f32, hp: Lr4, ap: Biquad },
    Haas(Box<Haas>),
    Invert { sign: [f32; 2] },
    DcBlock { x1: [f32; 2], y1: [f32; 2] },
    Meter { peak: f32, meter: usize },
    Split2(Box<Split2>),
    Oversampled(Box<Oversampled>),
    /// Blocks from `fx` (distortion, filters, modulation, space, pitch, rhythm, dynamics).
    Fx(Box<dyn fx::Fx>),
    Eq { shape: EqShape, freq: P, q: P, gain: P, last: [f32; 3], bq: Biquad },
    /// Low/high-pass with a slope: `stages` of `bqs` run ([`filter_stages`]).
    Filter { shape: EqShape, freq: P, q: P, slope: P, last: [f32; 3], bqs: [Biquad; FILTER_STAGES], stages: usize },
    Drive { amount: P, mix: P },
    Comp(Box<Comp>),
    Rider(Box<Rider>),
    Transient(Box<Transient>),
    BandSplit(Box<BandSplit>),
    Parallel(Box<Parallel>),
}

/// `sr` = the rate the nodes run at, `max_frames` = the most frames per call there.
///
/// Neighbouring blocks with the same oversampling share one up/down pair (less
/// latency and CPU than one pair each). Blocks without their own setting that sit
/// between two such blocks join them (e.g. a filter between two drives).
pub(crate) fn build_nodes(chain: &[BlockSpec], params: &[ParamSpec], sr: f32, max_frames: usize, meter: &mut usize) -> Vec<Node> {
    let mut factors: Vec<usize> = chain.iter().map(|b| oversampling_of(b, sr)).collect();
    let anchors: Vec<usize> = (0..chain.len()).filter(|&i| factors[i] > 1).collect();
    for w in anchors.windows(2) {
        let (a, b) = (w[0], w[1]);
        if factors[a] == factors[b] && chain[a + 1..b].iter().all(|x| x.oversampling.is_none()) {
            let f = factors[a];
            factors[a + 1..b].fill(f);
        }
    }
    let mut nodes = Vec::with_capacity(chain.len());
    let mut i = 0;
    while i < chain.len() {
        let f = factors[i];
        let end = i + factors[i..].iter().take_while(|&&x| x == f).count();
        if f == 1 {
            for b in &chain[i..end] {
                nodes.push(build_kind(&b.kind, params, sr, max_frames, meter));
            }
        } else {
            let inner: Vec<Node> = chain[i..end]
                .iter()
                .map(|b| build_kind(&b.kind, params, sr * f as f32, max_frames * f, meter))
                .collect();
            let os = Oversampler::new(f, max_frames, chain_latency(&inner));
            nodes.push(Node::Oversampled(Box::new(Oversampled { os, inner })));
        }
        i = end;
    }
    nodes
}

/// Oversampling of a block: its own setting, else automatic for nonlinear blocks
/// (the smallest factor that reaches [`AUTO_OVERSAMPLED_RATE`]), else none.
fn oversampling_of(b: &BlockSpec, sr: f32) -> usize {
    match b.oversampling {
        Some(f) => (f as usize).clamp(1, MAX_OVERSAMPLING),
        None if b.kind.is_nonlinear() => {
            let mut f = 1;
            while sr * (f as f32) < AUTO_OVERSAMPLED_RATE && f < MAX_OVERSAMPLING {
                f *= 2;
            }
            f
        }
        None => 1,
    }
}

fn build_kind(kind: &BlockKind, params: &[ParamSpec], sr: f32, max_frames: usize, meter: &mut usize) -> Node {
    let p = |r: &ParamRef| P::param(*r);
    fn next_meter(meter: &mut usize) -> usize {
        let m = *meter;
        *meter += 1;
        m
    }
    let eq = |shape: EqShape, e: &EqSpec| Node::Eq {
        shape,
        freq: p(&e.freq),
        q: p(&e.q),
        gain: p(&e.gain),
        last: [f32::NAN; 3],
        bq: Biquad::default(),
    };
    let filter = |shape: EqShape, f: &FilterSpec| Node::Filter {
        shape,
        freq: p(&f.freq),
        q: p(&f.q),
        slope: p(&f.slope),
        last: [f32::NAN; 3],
        bqs: [Biquad::default(); FILTER_STAGES],
        stages: 0,
    };
    // Sub-chains in the same depth-first order as `patch::chain::walk`.
    let chains = |cs: &[&Chain], meter: &mut usize| -> Vec<Vec<Node>> {
        cs.iter().map(|c| build_nodes(&c.0, params, sr, max_frames, meter)).collect()
    };
    if let Some(f) = fx::build(kind, &fx::Args { params, sr, max_frames }, meter) {
        return Node::Fx(f);
    }
    match kind {
        BlockKind::Gain { db } => Node::Gain { db: p(db), gain: 1.0 },
        BlockKind::PeakEq(e) => eq(EqShape::Peak, e),
        BlockKind::LowShelf(e) => eq(EqShape::LowShelf, e),
        BlockKind::HighShelf(e) => eq(EqShape::HighShelf, e),
        BlockKind::LowPass(f) => filter(EqShape::LowPass, f),
        BlockKind::HighPass(f) => filter(EqShape::HighPass, f),
        BlockKind::Drive { amount, mix } => Node::Drive { amount: p(amount), mix: p(mix) },
        BlockKind::Compressor(c) => comp(CompStyle::Clean, c, next_meter(meter)),
        BlockKind::FetCompressor(c) => comp(CompStyle::Fet, c, next_meter(meter)),
        BlockKind::OptoCompressor(c) => comp(CompStyle::Opto, c, next_meter(meter)),
        BlockKind::Rider { target, range, speed } => Node::Rider(Box::new(Rider {
            target: p(target),
            range: p(range),
            speed: p(speed),
            rms: 0.0,
            gain_db: 0.0,
            meter: next_meter(meter),
        })),
        BlockKind::Transient { attack, sustain } => Node::Transient(Box::new(Transient {
            attack: p(attack),
            sustain: p(sustain),
            fast: 0.0,
            slow: 0.0,
            gain_db: 0.0,
            meter: next_meter(meter),
        })),
        BlockKind::Pan { pan } => Node::Pan { pan: p(pan), gains: [1.0; 2] },
        BlockKind::Width { width } => Node::Width { width: p(width), w: 1.0 },
        BlockKind::MonoBass { freq } => {
            Node::MonoBass { freq: p(freq), last: f32::NAN, hp: Lr4::default(), ap: Biquad::default() }
        }
        BlockKind::Haas { delay, channel } => Node::Haas(Box::new(Haas {
            delay: p(delay),
            ch: match channel {
                Side::Left => 0,
                Side::Right => 1,
            },
            buf: vec![0.0; (HAAS_MAX_MS * 0.001 * sr) as usize + 2],
            pos: 0,
            cur: 0.0,
        })),
        BlockKind::Invert { channels } => Node::Invert {
            sign: match channels {
                Channels::Both => [-1.0, -1.0],
                Channels::Left => [-1.0, 1.0],
                Channels::Right => [1.0, -1.0],
            },
        },
        BlockKind::DcBlock => Node::DcBlock { x1: [0.0; 2], y1: [0.0; 2] },
        BlockKind::Meter => Node::Meter { peak: 0.0, meter: next_meter(meter) },
        BlockKind::MidSide { .. } | BlockKind::LeftRight { .. } => {
            let mode = if matches!(kind, BlockKind::MidSide { .. }) { SplitMode::MidSide } else { SplitMode::LeftRight };
            let built = chains(&kind.sub_chains(), meter);
            let (align, latency) = align(&built);
            let [a, b]: [Vec<Node>; 2] = built.try_into().ok().expect("two sub-chains");
            Node::Split2(Box::new(Split2 {
                mode,
                chains: [a, b],
                bufs: [frame_buf(max_frames), frame_buf(max_frames)],
                align,
                latency,
            }))
        }
        BlockKind::BandSplit { crossovers, bands } => {
            let n = crossovers.len();
            let bands = chains(&bands.iter().collect::<Vec<_>>(), meter);
            let (align, latency) = align(&bands);
            Node::BandSplit(Box::new(BandSplit {
                freqs: crossovers.iter().map(p).collect(),
                last: vec![f32::NAN; n],
                lp: vec![Lr4::default(); n],
                hp: vec![Lr4::default(); n],
                ap: (0..=n).map(|i| vec![Biquad::default(); n.saturating_sub(i + 1)]).collect(),
                bands,
                bufs: (0..=n).map(|_| frame_buf(max_frames)).collect(),
                align,
                latency,
            }))
        }
        BlockKind::Parallel { branches, mix } => {
            let branches = chains(&branches.iter().collect::<Vec<_>>(), meter);
            let (align, latency) = align(&branches);
            Node::Parallel(Box::new(Parallel {
                bufs: branches.iter().map(|_| frame_buf(max_frames)).collect(),
                branches,
                mix: mix.as_ref().map(p),
                align,
                latency,
            }))
        }
        _ => unreachable!("{kind:?} is built by fx::build"),
    }
}

fn comp(style: CompStyle, c: &CompSpec, meter: usize) -> Node {
    // The defaults per style (FET 4:1, 0.3/80 ms, knee 2; opto 3:1, 10/250, 8; clean 2:1, 5/100,
    // 4) are the prelude's: every setting arrives as a param.
    let p = |r: &ParamRef| P::param(*r);
    Node::Comp(Box::new(Comp {
        style,
        threshold: p(&c.threshold),
        ratio: p(&c.ratio),
        attack: p(&c.attack),
        release: p(&c.release),
        knee: p(&c.knee),
        makeup: p(&c.makeup),
        mix: p(&c.mix),
        auto_makeup: c.auto_makeup,
        env: 0.0,
        rms: 0.0,
        sustain: 0.0,
        avg_gr: 0.0,
        meter,
    }))
}

pub(crate) fn process_nodes(nodes: &mut [Node], frames: &mut [Frame], ctx: &Ctx) {
    for n in nodes {
        n.process(frames, ctx);
    }
}

impl Node {
    /// Latency in samples at the rate this node runs at.
    fn latency(&self) -> usize {
        match self {
            Node::Oversampled(o) => o.os.latency(),
            Node::BandSplit(b) => b.latency,
            Node::Parallel(p) => p.latency,
            Node::Split2(s) => s.latency,
            Node::Fx(f) => f.latency(),
            _ => 0,
        }
    }

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
            Node::Pan { pan, gains } => {
                // Balance: the side panned away from is attenuated, the other stays.
                let p = (pan.update(ctx) / 100.0).clamp(-1.0, 1.0);
                let target = [1.0 - p.max(0.0), 1.0 + p.min(0.0)];
                let n = frames.len() as f32;
                let step = [(target[0] - gains[0]) / n, (target[1] - gains[1]) / n];
                for f in frames {
                    gains[0] += step[0];
                    gains[1] += step[1];
                    f[0] *= gains[0];
                    f[1] *= gains[1];
                }
            }
            Node::Width { width, w } => {
                let target = (width.update(ctx) / 100.0).max(0.0);
                let step = (target - *w) / frames.len() as f32;
                for f in frames {
                    *w += step;
                    let m = 0.5 * (f[0] + f[1]);
                    let s = 0.5 * (f[0] - f[1]) * *w;
                    *f = [m + s, m - s];
                }
            }
            Node::MonoBass { freq, last, hp, ap } => {
                let fr = freq.update(ctx);
                if fr != *last {
                    hp.set(EqShape::HighPass, fr, sr);
                    ap.set(EqShape::AllPass, fr, std::f32::consts::FRAC_1_SQRT_2, 0.0, sr);
                    *last = fr;
                }
                for f in frames {
                    let m = ap.tick(0.5 * (f[0] + f[1]), 0);
                    let s = hp.tick(0.5 * (f[0] - f[1]), 0);
                    *f = [m + s, m - s];
                }
            }
            Node::Haas(h) => h.process(frames, ctx),
            Node::Invert { sign } => {
                for f in frames {
                    f[0] *= sign[0];
                    f[1] *= sign[1];
                }
            }
            Node::DcBlock { x1, y1 } => {
                let r = (-std::f32::consts::TAU * 5.0 / sr).exp();
                for f in frames {
                    for ch in 0..2 {
                        let x = f[ch];
                        y1[ch] = x - x1[ch] + r * y1[ch];
                        x1[ch] = x;
                        f[ch] = y1[ch];
                    }
                }
            }
            Node::Meter { peak, meter } => {
                let d = 0.25f32.powf((sr * PEAK_DECAY_MS * 0.001).recip());
                for f in frames.iter() {
                    *peak = (*peak * d).max(f[0].abs()).max(f[1].abs());
                }
                let level = ((gain_to_db(*peak) + 60.0) / 60.0).clamp(0.0, 1.0);
                ctx.meters[*meter].store(level, Ordering::Relaxed);
            }
            Node::Split2(s) => s.process(frames, ctx),
            Node::Fx(f) => f.process(frames, ctx),
            Node::Oversampled(o) => {
                let inner = Ctx { sr: sr * o.os.factor() as f32, ..*ctx };
                let nodes = &mut o.inner;
                o.os.run(frames, |buf| process_nodes(nodes, buf, &inner));
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
            Node::Filter { shape, freq, q, slope, last, bqs, stages } => {
                let cur = [freq.update(ctx), q.update(ctx), slope.update(ctx)];
                if cur != *last {
                    let (n, qs) = filter_stages(cur[2], cur[1]);
                    for (bq, &sq) in bqs.iter_mut().zip(&qs).take(n) {
                        bq.set(*shape, cur[0], sq, 0.0, sr);
                    }
                    // Stages coming back on start from silence.
                    for bq in bqs.iter_mut().take(n).skip(*stages) {
                        bq.clear();
                    }
                    *stages = n;
                    *last = cur;
                }
                for f in frames {
                    for bq in bqs.iter_mut().take(*stages) {
                        f[0] = bq.tick(f[0], 0);
                        f[1] = bq.tick(f[1], 1);
                    }
                }
            }
            Node::Drive { amount, mix } => {
                let g = db_to_gain(amount.update(ctx));
                let wet = (mix.update(ctx) / 100.0).clamp(0.0, 1.0);
                for f in frames {
                    for x in f.iter_mut() {
                        *x += wet * (drive_curve(*x, g) - *x);
                    }
                }
            }
            Node::Comp(c) => c.process(frames, ctx),
            Node::Rider(r) => r.process(frames, ctx),
            Node::Transient(t) => t.process(frames, ctx),
            Node::BandSplit(b) => b.process(frames, ctx),
            Node::Parallel(p) => p.process(frames, ctx),
        }
    }
}

impl Haas {
    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let len = self.buf.len();
        let max = (len - 2) as f32;
        let target = (self.delay.update(ctx) * 0.001 * ctx.sr).clamp(0.0, max);
        let step = (target - self.cur) / frames.len() as f32;
        for f in frames {
            self.cur += step;
            self.buf[self.pos] = f[self.ch];
            // Linear interpolation between the two samples around `pos - cur`.
            let read = self.pos as f32 + len as f32 - self.cur;
            let i0 = read.floor();
            let frac = read - i0;
            let i0 = i0 as usize % len;
            let i1 = (i0 + 1) % len;
            f[self.ch] = self.buf[i0] + frac * (self.buf[i1] - self.buf[i0]);
            self.pos = (self.pos + 1) % len;
        }
    }
}

impl Split2 {
    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let n = frames.len();
        let [a_buf, b_buf] = &mut self.bufs;
        for (i, f) in frames.iter().enumerate() {
            let (a, b) = match self.mode {
                SplitMode::MidSide => (0.5 * (f[0] + f[1]), 0.5 * (f[0] - f[1])),
                SplitMode::LeftRight => (f[0], f[1]),
            };
            a_buf[i] = [a, a];
            b_buf[i] = [b, b];
        }
        for ((chain, buf), d) in self.chains.iter_mut().zip(self.bufs.iter_mut()).zip(&mut self.align) {
            process_nodes(chain, &mut buf[..n], ctx);
            d.process(&mut buf[..n]);
        }
        for (i, f) in frames.iter_mut().enumerate() {
            let (a, b) = (self.bufs[0][i][0], self.bufs[1][i][0]);
            *f = match self.mode {
                SplitMode::MidSide => [a + b, a - b],
                SplitMode::LeftRight => [a, b],
            };
        }
    }
}

impl BandSplit {
    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let n = frames.len();
        let nx = self.freqs.len();
        // Crossovers driven by params may cross each other: keep them ascending.
        let mut prev = 10.0f32;
        let mut changed = false;
        for k in 0..nx {
            let f = self.freqs[k].update(ctx).max(prev);
            prev = f;
            if f != self.last[k] {
                self.last[k] = f;
                self.lp[k].set(EqShape::LowPass, f, ctx.sr);
                self.hp[k].set(EqShape::HighPass, f, ctx.sr);
                changed = true;
            }
        }
        if changed {
            for (i, aps) in self.ap.iter_mut().enumerate() {
                for (j, ap) in aps.iter_mut().enumerate() {
                    ap.set(EqShape::AllPass, self.last[i + 1 + j], std::f32::consts::FRAC_1_SQRT_2, 0.0, ctx.sr);
                }
            }
        }
        for (idx, frame) in frames.iter().enumerate() {
            for (ch, &x) in frame.iter().enumerate() {
                let mut rest = x;
                for k in 0..nx {
                    let mut lo = self.lp[k].tick(rest, ch);
                    for ap in &mut self.ap[k] {
                        lo = ap.tick(lo, ch);
                    }
                    self.bufs[k][idx][ch] = lo;
                    rest = self.hp[k].tick(rest, ch);
                }
                self.bufs[nx][idx][ch] = rest;
            }
        }
        for ((band, buf), d) in self.bands.iter_mut().zip(self.bufs.iter_mut()).zip(&mut self.align) {
            process_nodes(band, &mut buf[..n], ctx);
            d.process(&mut buf[..n]);
        }
        for (idx, f) in frames.iter_mut().enumerate() {
            *f = [0.0; 2];
            for buf in &self.bufs {
                f[0] += buf[idx][0];
                f[1] += buf[idx][1];
            }
        }
    }
}

impl Parallel {
    fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let n = frames.len();
        for ((branch, buf), d) in self.branches.iter_mut().zip(self.bufs.iter_mut()).zip(&mut self.align) {
            buf[..n].copy_from_slice(frames);
            process_nodes(branch, &mut buf[..n], ctx);
            d.process(&mut buf[..n]);
        }
        match &mut self.mix {
            Some(mix) => {
                // Linear crossfade: the branches are correlated (same source), so
                // an equal-power fade would bump the level by ~3 dB in the middle.
                let m = (mix.update(ctx) / 100.0).clamp(0.0, 1.0);
                for (idx, f) in frames.iter_mut().enumerate() {
                    for (ch, y) in f.iter_mut().enumerate() {
                        *y = (1.0 - m) * self.bufs[0][idx][ch] + m * self.bufs[1][idx][ch];
                    }
                }
            }
            None => {
                for (idx, f) in frames.iter_mut().enumerate() {
                    *f = [0.0; 2];
                    for buf in &self.bufs {
                        f[0] += buf[idx][0];
                        f[1] += buf[idx][1];
                    }
                }
            }
        }
    }
}

/// The compressor's gain computer: gain reduction (dB) at `over` dB above the threshold, with
/// `slope` = 1 − 1/ratio and a soft knee `knee` dB wide.
#[inline]
pub(crate) fn comp_gr(over: f32, slope: f32, knee: f32) -> f32 {
    if over <= -knee / 2.0 {
        0.0
    } else if over >= knee / 2.0 || knee == 0.0 {
        over * slope
    } else {
        slope * (over + knee / 2.0).powi(2) / (2.0 * knee)
    }
}

/// The drive block's curve: tanh saturation, loudness-compensated by 1/√g.
#[inline]
pub(crate) fn drive_curve(x: f32, g: f32) -> f32 {
    (x * g).tanh() / g.sqrt()
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
            let gr = comp_gr(level - thr, slope, knee);
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
    /// The DSP structure (chain, params, sample rate): graphs with the same key can hand their
    /// running state over ([`Graph::adopt`]).
    chain_key: u64,
    tempo: f32,
    beat: Option<f64>,
    modulations: Vec<Vec<Modulation>>,
    /// Each modulation's settings (packed [`patch::ModSettings`]), in [`Patch::mod_index`]
    /// order; the editor changes them live.
    mod_settings: ModAtomics,
    /// Fixed modulation sources (Follow, LFO 1–3, Step).
    follow: modulators::Follow,
    lfos: [modulators::Lfo; 3],
    step: modulators::Step,
    /// Pre-resolved modulator assignments targeting each macro slot: `(source, assignment_index)`.
    macro_mods: [Vec<(ModulatorId, usize)>; MACRO_SLOTS],
    /// Pre-resolved modulator assignments targeting each param: `(source, assignment_index)`.
    param_mods: Vec<Vec<(ModulatorId, usize)>>,
    /// Packed [`patch::ModSettings`] for `patch.assignments` in assignment order.
    assignment_settings: ModAtomics,
    /// Live unipolar `[0, 1]` outputs of Follow, LFO 1–3, Step (in [`ModulatorId::ALL`] order).
    mod_out: Atomics,
    /// Where each modulator is in its cycle, for display: LFOs their phase (0–1), Step the index
    /// of the step playing, Follow 0.
    mod_phase: Atomics,
    /// Live effective macro values `[0, 1]` (host base + modulator deltas, clamped).
    macro_eff: Atomics,
    /// Live effective normalized param values `[0, 1]` (base + all modulation deltas, clamped).
    param_norm: Atomics,
    /// Every param's range/scale, to map base + macro offsets to plain values.
    specs: Vec<ParamSpec>,
    free: Atomics,
    meters: Atomics,
    values: Vec<f32>,
    sr: f32,
    k: f32,
}

/// A patch's LFO shape as the LFO's morph position ([`modulators::morph`]).
pub fn lfo_shape(shape: ModulatorLfoShape) -> f32 {
    match shape {
        ModulatorLfoShape::Sine => 0.0,
        ModulatorLfoShape::Triangle => 1.0 / 3.0,
        ModulatorLfoShape::Square => 1.0,
    }
}

/// A modulator's rate: synced to the host (beats) when `sync` is set, else free in Hz.
fn rate(hz: f32, sync: Option<f32>) -> modulators::Rate {
    sync.map_or(modulators::Rate::Hz(hz), modulators::Rate::Beats)
}

fn build_modulators(patch: &Patch, sr: f32) -> (modulators::Follow, [modulators::Lfo; 3], modulators::Step) {
    let mut follow_cfg = modulators::FollowSettings::default();
    let mut lfo_cfgs = [modulators::LfoSettings::default(); 3];
    let mut step_cfg = modulators::StepSettings::default();
    for spec in &patch.modulators {
        match (spec.id, &spec.settings) {
            (ModulatorId::Follow, ModulatorSettings::Follow { attack_ms, release_ms }) => {
                follow_cfg.attack_ms = *attack_ms;
                follow_cfg.release_ms = *release_ms;
            }
            (ModulatorId::Lfo1 | ModulatorId::Lfo2 | ModulatorId::Lfo3, ModulatorSettings::Lfo { rate_hz, sync, phase, shape }) => {
                let idx = match spec.id {
                    ModulatorId::Lfo1 => 0,
                    ModulatorId::Lfo2 => 1,
                    _ => 2,
                };
                lfo_cfgs[idx].rate = rate(*rate_hz, *sync);
                lfo_cfgs[idx].phase = *phase * 360.0;
                lfo_cfgs[idx].shape = lfo_shape(*shape);
            }
            (ModulatorId::Step, ModulatorSettings::Step { rate_hz, sync, values }) => {
                step_cfg.rate = rate(*rate_hz, *sync);
                step_cfg.length = values.len().clamp(1, modulators::STEPS);
                let mut arr = [0.5; modulators::STEPS];
                for (dst, src) in arr.iter_mut().zip(values.iter()) {
                    *dst = *src;
                }
                step_cfg.values = arr;
            }
            _ => {}
        }
    }
    (
        modulators::Follow::new(sr, follow_cfg),
        [
            modulators::Lfo::new(sr, lfo_cfgs[0]),
            modulators::Lfo::new(sr, lfo_cfgs[1]),
            modulators::Lfo::new(sr, lfo_cfgs[2]),
        ],
        modulators::Step::new(sr, step_cfg),
    )
}

/// Fingerprint of what `build_nodes` builds from: the chain, the params, the sample rate.
fn chain_key(patch: &Patch, sample_rate: f32) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    format!("{:?}|{:?}", patch.chain, patch.params).hash(&mut h);
    sample_rate.to_bits().hash(&mut h);
    h.finish()
}

impl Graph {
    /// `free` contains the plain base values of all params, including modulated ones.
    /// Missing entries use the patch defaults.
    pub fn build(patch: &Patch, sample_rate: f32, free: &[f32]) -> Self {
        let mut idle = Vec::new();
        patch::chain::walk(&patch.chain, &mut |b| idle.extend(b.kind.idle_meter()));
        let mut meter = 0;
        let nodes = build_nodes(&patch.chain, &patch.params, sample_rate, SUB_BLOCK, &mut meter);
        debug_assert_eq!(meter, patch.meters.len(), "meter order must match patch::chain::walk");
        debug_assert_eq!(meter, idle.len());
        let sub_rate = sample_rate / SUB_BLOCK as f32;
        let (follow, lfos, step) = build_modulators(patch, sample_rate);
        let mut macro_mods: [Vec<(ModulatorId, usize)>; MACRO_SLOTS] = std::array::from_fn(|_| Vec::new());
        let mut param_mods: Vec<Vec<(ModulatorId, usize)>> = vec![Vec::new(); patch.params.len()];
        for (idx, a) in patch.assignments.iter().enumerate() {
            match &a.destination {
                ModDestination::Param(id) => {
                    if let Some(p) = patch.param_index(id) {
                        param_mods[p].push((a.source, idx));
                    }
                }
                ModDestination::Macro(id) => {
                    if let Some(m) = patch.macro_index(id) {
                        macro_mods[m].push((a.source, idx));
                    }
                }
            }
        }
        let initial_free: Vec<f32> = patch
            .params
            .iter()
            .enumerate()
            .map(|(i, spec)| free.get(i).copied().unwrap_or_else(|| spec.default_plain()))
            .collect();
        let initial_norm: Vec<f32> = patch
            .params
            .iter()
            .zip(&initial_free)
            .map(|(spec, &v)| spec.normalize(v))
            .collect();
        Graph {
            nodes,
            chain_key: chain_key(patch, sample_rate),
            tempo: 120.0,
            beat: None,
            modulations: patch.modulations.clone(),
            mod_settings: patch.modulations.iter().flatten().map(|m| AtomicU64::new(m.settings.pack())).collect::<Vec<_>>().into(),
            follow,
            lfos,
            step,
            macro_mods,
            param_mods,
            assignment_settings: patch.assignments.iter().map(|a| AtomicU64::new(a.settings.pack())).collect::<Vec<_>>().into(),
            mod_out: atomics([0.0; 5]),
            mod_phase: atomics([0.0; 5]),
            macro_eff: atomics([0.0; MACRO_SLOTS]),
            param_norm: atomics(initial_norm),
            specs: patch.params.clone(),
            free: atomics(initial_free),
            meters: atomics(idle),
            values: vec![0.0; patch.params.len()],
            sr: sample_rate,
            k: 1.0 - coef(SETTING_SMOOTHING_MS, sub_rate),
        }
    }

    /// Takes over the running state of `old`, the graph this one replaces, when both have the
    /// same DSP structure: block state (filter memories, delay lines, reverb tails) and the
    /// modulators' phases and envelopes, with this graph's modulator settings applied on top.
    /// So changing routes or modulator settings (which rebuilds the graph) doesn't click or
    /// restart an LFO. A different chain starts fresh. Real-time safe: swaps, no allocation;
    /// `old` gets the fresh state, to be dropped off the audio thread.
    pub fn adopt(&mut self, old: &mut Graph) {
        self.tempo = old.tempo;
        self.beat = old.beat;
        if self.chain_key != old.chain_key {
            return;
        }
        std::mem::swap(&mut self.nodes, &mut old.nodes);
        let (follow, step) = (self.follow.settings(), self.step.settings());
        let lfos = [self.lfos[0].settings(), self.lfos[1].settings(), self.lfos[2].settings()];
        std::mem::swap(&mut self.follow, &mut old.follow);
        std::mem::swap(&mut self.lfos, &mut old.lfos);
        std::mem::swap(&mut self.step, &mut old.step);
        self.follow.set_settings(follow);
        self.step.set_settings(step);
        for (lfo, s) in self.lfos.iter_mut().zip(lfos) {
            lfo.set_settings(s);
        }
    }

    /// Host transport, once per host buffer: tempo for synced blocks
    /// (`sync = "1/8"`; `None` keeps the last, initially 120 BPM) and the position
    /// in beats at the buffer's first frame (`None` = not playing). The graph
    /// advances the position itself within the buffer.
    pub fn set_transport(&mut self, bpm: Option<f32>, beat: Option<f64>) {
        if let Some(t) = bpm.filter(|t| t.is_finite() && *t > 1.0) {
            self.tempo = t;
        }
        self.beat = beat.filter(|b| b.is_finite());
    }

    /// Latency in samples (oversampled blocks, lookahead), for the host's delay compensation.
    pub fn latency(&self) -> usize {
        chain_latency(&self.nodes)
    }

    /// Free param values (plain), writable from the editor.
    pub fn free_values(&self) -> Atomics {
        self.free.clone()
    }

    /// The modulations' settings (index = [`Patch::mod_index`]), writable from the editor.
    pub fn mod_settings(&self) -> ModAtomics {
        self.mod_settings.clone()
    }

    /// Modulator assignment settings (index = `Patch::assignments`), writable from the editor.
    pub fn assignment_settings(&self) -> ModAtomics {
        self.assignment_settings.clone()
    }

    /// Live unipolar `[0, 1]` outputs of Follow, LFO 1–3, Step, readable from the editor.
    pub fn modulator_values(&self) -> Atomics {
        self.mod_out.clone()
    }

    /// Live cycle positions (see `mod_phase`), readable from the editor. The editor draws the
    /// LFO's wave and Step's bars from the settings and marks these on them: sampling the output
    /// once per UI frame instead would alias anything faster than a few Hz.
    pub fn modulator_phases(&self) -> Atomics {
        self.mod_phase.clone()
    }

    /// Live effective macro values `[0, 1]`, readable from the editor.
    pub fn macro_values(&self) -> Atomics {
        self.macro_eff.clone()
    }

    /// Live effective normalized param values `[0, 1]`, readable from the editor.
    pub fn param_values(&self) -> Atomics {
        self.param_norm.clone()
    }

    /// Block meters (index = `Patch::meters`), readable from the editor.
    pub fn meters(&self) -> Atomics {
        self.meters.clone()
    }

    /// Processes up to [`SUB_BLOCK`] frames in place. Real-time safe.
    pub fn process(&mut self, macros: &[f32; MACRO_SLOTS], frames: &mut [Frame]) {
        debug_assert!(frames.len() <= SUB_BLOCK);
        let n = frames.len();
        if n == 0 {
            return;
        }

        // 1. Evaluate the five global modulators at control rate (no allocation).
        let transport = modulators::Transport { tempo: self.tempo, beat: self.beat };
        let mut buf = [0.0f32; SUB_BLOCK];
        let mut src = [0.0f32; 5];
        self.follow.process(frames, &mut buf[..n]);
        src[ModulatorId::Follow.index()] = buf[n - 1].clamp(0.0, 1.0);
        for (k, id) in [ModulatorId::Lfo1, ModulatorId::Lfo2, ModulatorId::Lfo3].into_iter().enumerate() {
            self.lfos[k].process(&transport, &mut buf[..n]);
            src[id.index()] = (0.5 * (buf[n - 1] + 1.0)).clamp(0.0, 1.0);
        }
        self.step.process(&transport, &mut buf[..n]);
        src[ModulatorId::Step.index()] = (0.5 * (buf[n - 1] + 1.0)).clamp(0.0, 1.0);
        for (slot, &val) in self.mod_out.iter().zip(&src) {
            slot.store(val, Ordering::Relaxed);
        }
        for (k, lfo) in self.lfos.iter().enumerate() {
            self.mod_phase[1 + k].store(lfo.phase(), Ordering::Relaxed);
        }
        self.mod_phase[ModulatorId::Step.index()].store(self.step.current_step() as f32, Ordering::Relaxed);

        // 2. Apply modulator assignments targeting host macros (host base stays untouched).
        let mut eff_macros = *macros;
        for (slot, mods) in self.macro_mods.iter().enumerate() {
            let mut v = macros[slot];
            for &(id, idx) in mods {
                v += patch::ModSettings::unpack(self.assignment_settings[idx].load(Ordering::Relaxed)).delta(src[id.index()]);
            }
            let clamped = v.clamp(0.0, 1.0);
            eff_macros[slot] = clamped;
            self.macro_eff[slot].store(clamped, Ordering::Relaxed);
        }

        // 3. Sum macro + modulator slot deltas onto each param's editable base in normalized space.
        let mut j = 0; // index into mod_settings
        for (i, spec) in self.specs.iter().enumerate() {
            let base = self.free[i].load(Ordering::Relaxed);
            if self.modulations[i].is_empty() && self.param_mods[i].is_empty() {
                self.values[i] = base;
                self.param_norm[i].store(spec.normalize(base), Ordering::Relaxed);
                continue;
            }
            let mut v = spec.normalize(base);
            for m in &self.modulations[i] {
                v += patch::ModSettings::unpack(self.mod_settings[j].load(Ordering::Relaxed)).delta(eff_macros[m.slot]);
                j += 1;
            }
            for &(id, idx) in &self.param_mods[i] {
                v += patch::ModSettings::unpack(self.assignment_settings[idx].load(Ordering::Relaxed)).delta(src[id.index()]);
            }
            let norm = v.clamp(0.0, 1.0);
            self.param_norm[i].store(norm, Ordering::Relaxed);
            self.values[i] = spec.plain(norm);
        }

        let ctx = Ctx { values: &self.values, sr: self.sr, k: self.k, meters: &self.meters, tempo: self.tempo, beat: self.beat };
        process_nodes(&mut self.nodes, frames, &ctx);
        if let Some(b) = &mut self.beat {
            *b += n as f64 * self.tempo as f64 / 60.0 / self.sr as f64;
        }
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

    /// `setup` = Lua statements (e.g. params), `chain` = the blocks of the chain.
    fn patch_with(chain: &str, setup: &str) -> (Patch, TempDir) {
        let d = TempDir::new();
        std::fs::write(
            d.0.join("init.luau"),
            format!("{setup}\nreturn Patch.new('T', {{ chain = Chain.new {{ {chain} }} }})"),
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

    fn run(chain: &str, setup: &str, input: &mut [Frame]) -> Graph {
        let (p, _d) = patch_with(chain, setup);
        let mut g = Graph::build(&p, SR, &p.default_param_values());
        g.process_all(&[0.0; MACRO_SLOTS], input);
        g
    }

    /// Level change (dB) of a sine through `chain`, measured after settling.
    fn gain_through(chain: &str, freq: f32) -> f32 {
        let mut x = sine(freq, -12.0, 0.5);
        let reference = tail_db(&x, 0.2);
        run(chain, "", &mut x);
        tail_db(&x, 0.2) - reference
    }

    #[test]
    fn graph_uses_editable_bases_and_sums_modulations() {
        let d = TempDir::new();
        std::fs::write(d.0.join("init.luau"), "
            local g = Blocks.Gain.new('g', { db = -12 })
            local a = Macro.new('a', 'A', { default = 0 }):drives(g.params.db, { from = -12, to = 12 })
            local b = Macro.new('b', 'B', { default = 0 }):drives(g.params.db, { from = -12, to = 12 })
            return Patch.new('T', { chain = Chain.new { g }, macros = { a, b } })").unwrap();
        let p = Patch::load(&d.0).unwrap();
        let mut g = Graph::build(&p, SR, &[]); // missing base uses the default
        let mut macros = [0.0; MACRO_SLOTS];
        g.process(&macros, &mut [[1.0, 1.0]]);
        assert!((g.values[0] + 12.0).abs() < 0.001);
        g.free_values()[0].store(-6.0, Ordering::Relaxed);
        macros[0] = 0.5;
        macros[1] = 0.25;
        g.process(&macros, &mut [[1.0, 1.0]]);
        assert!((g.values[0] - p.param_plain(0, &macros, &[-6.0])).abs() < 0.001);
        assert!((g.values[0] - 12.0).abs() < 0.001);
    }

    #[test]
    fn graph_evaluates_modulators_to_params_and_macros() {
        let d = TempDir::new();
        std::fs::write(d.0.join("init.luau"), "
            local g1 = Blocks.Gain.new('g1', { db = 0 })
            local g2 = Blocks.Gain.new('g2', { db = 0 })
            local m = Macro.new('m', 'M', { default = 0.5 }):drives(g2.params.db, { from = -12, to = 12 })
            return Patch.new('T', {
              chain = Chain.new { g1, g2 },
              macros = { m },
              modulators = {
                { id = 'follow', attack_ms = 1, release_ms = 50 },
                { id = 'lfo1', rate_hz = 2, phase = 0.25, shape = 'square' },
                { id = 'step', rate_hz = 4, values = { 1, 0 } },
              },
              assignments = {
                { source = 'follow', destination = g1.params.db, amount = 0.5 },
                { source = 'step', destination = m, amount = 0.5 },
              },
            })").unwrap();
        let p = Patch::load(&d.0).unwrap();
        let mut g = Graph::build(&p, SR, &p.default_param_values());
        let mut macros = [0.0; MACRO_SLOTS];
        macros[0] = 0.25;
        for _ in 0..8 {
            let mut loud = [[1.0, 1.0]; SUB_BLOCK];
            g.process(&macros, &mut loud);
        }
        // The editor's cycle positions: the LFO's phase, the index of the step playing.
        assert_eq!(g.modulator_phases()[ModulatorId::Lfo1.index()].load(Ordering::Relaxed), g.lfos[0].phase());
        assert_eq!(g.modulator_phases()[ModulatorId::Step.index()].load(Ordering::Relaxed), g.step.current_step() as f32);
        // Follow saw loud input -> g1.db moved above 0 dB.
        assert!(g.values[0] > 10.0, "g1.db={}", g.values[0]);
        assert!(g.modulator_values()[ModulatorId::Follow.index()].load(Ordering::Relaxed) > 0.8);
        // Step starts at value 1.0 (unipolar 1.0) -> adds +0.5 to macro 0 (0.25 -> 0.75) -> g2.db = +6 dB.
        assert!((g.macro_values()[0].load(Ordering::Relaxed) - 0.75).abs() < 0.01);
        assert!((g.values[1] - 6.0).abs() < 0.2, "g2.db={}", g.values[1]);
        // Live assignment update: bypassing step drops macro 0 back to its host base 0.25 (-6 dB).
        g.assignment_settings()[1].store(patch::ModSettings { amount: 0.5, bypass: true, ..Default::default() }.pack(), Ordering::Relaxed);
        g.process(&macros, &mut [[0.0, 0.0]; SUB_BLOCK]);
        assert!((g.macro_values()[0].load(Ordering::Relaxed) - 0.25).abs() < 0.01);
        assert!((g.values[1] + 6.0).abs() < 0.2, "g2.db={}", g.values[1]);
    }

    #[test]
    fn synced_modulators_follow_the_host_beat() {
        let d = TempDir::new();
        std::fs::write(d.0.join("init.luau"), "
            return Patch.new('T', { chain = Chain.new { Blocks.Gain.new('g', {}) },
              modulators = { { id = 'lfo2', rate_hz = 7, sync = '1/2' }, { id = 'step', sync = '1/16', values = { 0, 1, 0, 1 } } } })").unwrap();
        let p = Patch::load(&d.0).unwrap();
        assert!(matches!(p.modulators[2].settings, ModulatorSettings::Lfo { sync: Some(2.0), .. }));
        let mut g = Graph::build(&p, SR, &[]);
        // Playing at beat 0.5: a quarter into the half-note LFO cycle, step 2 of the 1/16 steps.
        g.set_transport(Some(120.0), Some(0.5));
        g.process(&[0.0; MACRO_SLOTS], &mut [[0.0; 2]; 1]);
        assert!((g.lfos[1].phase() - 0.25).abs() < 1e-3, "{}", g.lfos[1].phase());
        assert_eq!(g.step.current_step(), 2);
    }

    #[test]
    fn adopt_keeps_running_state_when_only_routing_changes() {
        let d = TempDir::new();
        let src = |extra: &str| format!("
            local dl = Blocks.Delay.new('d', {{ time = 10, feedback = 0, mix = 100 }})
            return Patch.new('T', {{ chain = Chain.new {{ dl }}, {extra} }})");
        std::fs::write(d.0.join("init.luau"), src("modulators = { { id = 'lfo1', rate_hz = 1, phase = 0, shape = 'sine' } }")).unwrap();
        let p = Patch::load(&d.0).unwrap();
        let mut old = Graph::build(&p, SR, &p.default_param_values());
        // An impulse goes into the 10 ms delay and the LFO runs for 3 ms.
        let mut buf = [[0.0; 2]; SUB_BLOCK];
        buf[0] = [1.0, 1.0];
        old.process_all(&[0.0; MACRO_SLOTS], &mut buf);
        for _ in 0..(144 / SUB_BLOCK) {
            old.process_all(&[0.0; MACRO_SLOTS], &mut [[0.0; 2]; SUB_BLOCK]);
        }
        let lfo_before = old.lfos[0].phase();

        // Same chain, an assignment added and the LFO faster: the echo still arrives, the LFO
        // continues from where it was.
        let mut q = Patch::load(&d.0).unwrap();
        q.add_route(patch::RouteSource::Modulator(ModulatorId::Lfo1), ModDestination::Param("d.mix".into())).unwrap();
        assert!(q.set_modulator(patch::ModulatorSpec {
            id: ModulatorId::Lfo1,
            settings: ModulatorSettings::Lfo { rate_hz: 5.0, sync: None, phase: 0.0, shape: ModulatorLfoShape::Sine },
        }));
        let mut new = Graph::build(&q, SR, &q.default_param_values());
        new.adopt(&mut old);
        assert_eq!(new.lfos[0].phase(), lfo_before);
        assert_eq!(new.lfos[0].settings().rate, modulators::Rate::Hz(5.0));
        let mut tail = vec![[0.0; 2]; 960];
        new.process_all(&[0.0; MACRO_SLOTS], &mut tail);
        assert!(tail.iter().any(|f| f[0].abs() > 0.1), "the echo was lost");

        // A different chain starts fresh.
        std::fs::write(d.0.join("init.luau"), src("").replace("time = 10", "time = 20")).unwrap();
        let r = Patch::load(&d.0).unwrap();
        let mut other = Graph::build(&r, SR, &r.default_param_values());
        other.adopt(&mut new);
        let mut out = vec![[0.0; 2]; 2000];
        other.process_all(&[0.0; MACRO_SLOTS], &mut out);
        assert!(out.iter().all(|f| f[0] == 0.0));
    }

    #[test]
    fn two_band_split_sums_flat() {
        for freq in [60.0, 250.0, 1000.0, 5000.0] {
            let diff = gain_through("Blocks.BandSplit.new('s', { crossovers = { 250 }, bands = { Chain.new(), Chain.new() } })", freq);
            assert!(diff.abs() < 0.05, "{freq} Hz: {diff} dB");
        }
    }

    #[test]
    fn four_band_split_sums_flat() {
        let chain = "Blocks.BandSplit.new('s', { crossovers = { 120, 800, 4000 },
            bands = { Chain.new(), Chain.new(), Chain.new(), Chain.new() } })";
        for freq in [40.0, 120.0, 300.0, 800.0, 2000.0, 4000.0, 9000.0] {
            let diff = gain_through(chain, freq);
            assert!(diff.abs() < 0.05, "{freq} Hz: {diff} dB");
        }
    }

    #[test]
    fn band_split_processes_bands_separately() {
        // Mute the middle band of three: 800 Hz (between 200 and 3000) disappears,
        // 50 Hz and 12 kHz stay.
        let chain = "Blocks.BandSplit.new('s', { crossovers = { 200, 3000 },
            bands = { Chain.new(), Chain.new { Blocks.Gain.new('mute', { db = -120 }) }, Chain.new() } })";
        assert!(gain_through(chain, 800.0) < -20.0);
        assert!(gain_through(chain, 50.0).abs() < 0.5);
        assert!(gain_through(chain, 12000.0).abs() < 0.5);
    }

    #[test]
    fn parallel_sums_or_crossfades() {
        let dry = "Chain.new()";
        let quiet = "Chain.new { Blocks.Gain.new('g', { db = -6.0206 }) }";
        // Sum of dry + half = 1.5x.
        let sum = gain_through(&format!("Blocks.Parallel.new('p', {{ branches = {{ {dry}, {quiet} }} }})"), 1000.0);
        assert!((sum - 20.0 * 1.5f32.log10()).abs() < 0.05, "sum {sum}");
        // Mix 0 % = first branch, 100 % = second, 50 % = halfway (0.75x).
        for (mix, expect) in [(0.0, 1.0f32), (100.0, 0.5), (50.0, 0.75)] {
            let g = gain_through(&format!("Blocks.Parallel.new('p', {{ mix = {mix}, branches = {{ {dry}, {quiet} }} }})"), 1000.0);
            assert!((g - 20.0 * expect.log10()).abs() < 0.05, "mix {mix}: {g}");
        }
    }

    #[test]
    fn peak_eq_boosts_at_center() {
        let g = gain_through("Blocks.PeakEq.new('e', { freq = 1000, q = 1, gain = 6 })", 1000.0);
        assert!((g - 6.0).abs() < 0.1);
    }

    #[test]
    fn fet_comp_reduces_by_ratio() {
        // Sine peak at -6 dBFS, threshold -20, ratio 4, hard knee → ~10.5 dB GR.
        let mut x = sine(1000.0, -6.0, 1.0);
        let reference = tail_db(&x, 0.3);
        let g = run("Blocks.FETCompressor.new('c', { threshold = -20, ratio = 4, knee = 0 })", "", &mut x);
        let gr = reference - tail_db(&x, 0.3);
        assert!((gr - 10.5).abs() < 1.0, "gr {gr}");
        let meter = g.meters()[0].load(Ordering::Relaxed) * 24.0;
        assert!((meter - 10.5).abs() < 1.0, "meter {meter}");
    }

    #[test]
    fn rider_lifts_quiet_signal_within_range() {
        let mut x = sine(100.0, -30.0, 4.0);
        let reference = tail_db(&x, 0.5);
        run("Blocks.Rider.new('r', { target = -18, range = 6, speed = 100 })", "", &mut x);
        let lift = tail_db(&x, 0.5) - reference;
        assert!((lift - 6.0).abs() < 0.5, "lift {lift}");
    }

    #[test]
    fn param_refs_follow_macros() {
        let d = TempDir::new();
        std::fs::write(
            d.0.join("init.luau"),
            "local out = Blocks.Gain.new('out', { db = 0 })
             local m = Macro.new('m', 'M'):drives(out.params.db, { from = -24, to = 0 })
             return Patch.new('T', { chain = Chain.new { out }, macros = { m } })",
        )
        .unwrap();
        let p = Patch::load(&d.0).unwrap();
        let mut g = Graph::build(&p, SR, &p.default_param_values());
        let mut macros = [0.0; MACRO_SLOTS];
        macros[0] = 0.5; // -12 dB
        let mut x = sine(1000.0, -6.0, 1.0);
        let reference = tail_db(&x, 0.2);
        g.process_all(&macros, &mut x);
        assert!((reference - tail_db(&x, 0.2) - 12.0).abs() < 0.1);
    }

    #[test]
    fn meters_follow_walk_order_through_sub_chains() {
        let (p, _d) = patch_with(
            "Blocks.Parallel.new('p', { branches = {
               Chain.new { Blocks.Compressor.new('a', { threshold = -10 }) },
               Chain.new { Blocks.Rider.new('b', { target = -18, range = 3 }) } } }),
             Blocks.Transient.new('c', { attack = 0 })",
            "",
        );
        assert_eq!(p.meters, ["a", "b", "c"]);
        let g = Graph::build(&p, SR, &p.default_param_values());
        let idle: Vec<f32> = g.meters().iter().map(|m| m.load(Ordering::Relaxed)).collect();
        assert_eq!(idle, [0.0, 0.5, 0.5]);
    }

    /// Runs a stereo signal through `chain` (no setup), returns the output.
    fn through(chain: &str, mut x: Vec<Frame>) -> Vec<Frame> {
        run(chain, "", &mut x);
        x
    }

    fn peak(x: &[Frame], ch: usize) -> f32 {
        x.iter().fold(0.0f32, |p, f| p.max(f[ch].abs()))
    }

    #[test]
    fn pan_attenuates_the_other_side() {
        let x = sine(1000.0, -6.0, 0.2);
        let y = through("Blocks.Pan.new('p', { pan = 100 })", x.clone());
        assert!(peak(&y[4800..], 0) < 1e-6 && (peak(&y[4800..], 1) - peak(&x, 1)).abs() < 1e-4);
        let y = through("Blocks.Pan.new('p', { pan = -50 })", x.clone());
        assert!((peak(&y[4800..], 1) / peak(&x, 1) - 0.5).abs() < 1e-3);
        assert!((peak(&y[4800..], 0) - peak(&x, 0)).abs() < 1e-4);
    }

    #[test]
    fn width_scales_the_side_signal() {
        // Left only = mid x/2 + side x/2.
        let x: Vec<Frame> = sine(1000.0, -6.0, 0.2).into_iter().map(|f| [f[0], 0.0]).collect();
        let mono = through("Blocks.Width.new('w', { width = 0 })", x.clone());
        assert!(mono[4800..].iter().all(|f| (f[0] - f[1]).abs() < 1e-6));
        let same = through("Blocks.Width.new('w', { width = 100 })", x.clone());
        assert!(same.iter().zip(&x).all(|(a, b)| (a[0] - b[0]).abs() < 1e-6 && a[1].abs() < 1e-6));
        let wide = through("Blocks.Width.new('w', { width = 200 })", x.clone());
        let a = peak(&x, 0);
        assert!((peak(&wide[4800..], 0) - 1.5 * a).abs() < 1e-3 && (peak(&wide[4800..], 1) - 0.5 * a).abs() < 1e-3);
    }

    #[test]
    fn mono_bass_removes_low_side_only() {
        let left_only = |freq| sine(freq, -6.0, 0.5).into_iter().map(|f| [f[0], 0.0]).collect::<Vec<Frame>>();
        let chain = "Blocks.MonoBass.new('m', { freq = 200 })";
        let low = through(chain, left_only(40.0));
        let tail = &low[12_000..];
        let diff = tail.iter().fold(0.0f32, |p, f| p.max((f[0] - f[1]).abs()));
        assert!(diff < 0.02 * peak(tail, 0), "40 Hz side left: {diff}");
        let high = through(chain, left_only(3000.0));
        // Above the cutoff the image is unchanged: the right channel stays silent.
        let leak = gain_to_db(peak(&high[12_000..], 1)) + 6.0;
        assert!(leak < -60.0, "3 kHz leaks into the right channel: {leak} dB");
        assert!((peak(&high[12_000..], 0) - db_to_gain(-6.0)).abs() < 0.01);
    }

    #[test]
    fn haas_delays_one_channel() {
        let mut x = vec![[0.0f32; 2]; 2000];
        x[100] = [1.0, 1.0];
        let y = through("Blocks.Haas.new('h', { delay = 10 })", x);
        assert_eq!(y[100], [1.0, 0.0]);
        assert!((y[580][1] - 1.0).abs() < 1e-6, "{:?}", &y[578..583]);
        let mut x = vec![[0.0f32; 2]; 2000];
        x[0] = [1.0, 1.0];
        let y = through("Blocks.Haas.new('h', { delay = 1, channel = 'left' })", x);
        assert_eq!(y[0], [0.0, 1.0]);
        assert!((y[48][0] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn invert_and_dc_block() {
        let y = through("Blocks.Invert.new('i', { channels = 'left' })", vec![[0.5, 0.5]; 64]);
        assert!(y.iter().all(|f| *f == [-0.5, 0.5]));
        let y = through("Blocks.Invert.new('i')", vec![[0.5, 0.25]; 64]);
        assert!(y.iter().all(|f| *f == [-0.5, -0.25]));
        let y = through("Blocks.DCBlock.new('dc')", vec![[0.5, -0.5]; 48_000]);
        assert!(peak(&y[24_000..], 0) < 1e-3 && peak(&y[24_000..], 1) < 1e-3);
    }

    #[test]
    fn meter_block_reports_peak_level() {
        let mut x = sine(1000.0, -6.0, 0.5);
        let g = run("Blocks.Gain.new('g', { db = -12 }), Blocks.Meter.new('m')", "", &mut x);
        let level = g.meters()[0].load(Ordering::Relaxed);
        assert!((level - (-18.0 + 60.0) / 60.0).abs() < 0.01, "{level}");
    }

    #[test]
    fn mid_side_and_left_right_run_their_own_chains() {
        let x: Vec<Frame> = sine(1000.0, -6.0, 0.2).into_iter().map(|f| [f[0], 0.3 * f[0]]).collect();
        let mute = "Chain.new { Blocks.Gain.new('mute', { db = -120 }) }";
        let y = through(&format!("Blocks.MidSide.new('ms', {{ side = {mute} }})"), x.clone());
        assert!(y[4800..].iter().all(|f| (f[0] - f[1]).abs() < 1e-5), "side muted = mono");
        let y = through(&format!("Blocks.LeftRight.new('lr', {{ right = {mute} }})"), x.clone());
        assert!(peak(&y[4800..], 1) < 1e-6);
        assert!(y.iter().zip(&x).all(|(a, b)| (a[0] - b[0]).abs() < 1e-6));
        // Empty chains: transparent.
        let y = through("Blocks.MidSide.new('ms', { mid = Chain.new() })", x.clone());
        assert!(y.iter().zip(&x).all(|(a, b)| (a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5));
    }

    fn latency(chain: &str, sr: f32) -> usize {
        let d = TempDir::new();
        std::fs::write(d.0.join("init.luau"), format!("return Patch.new('T', {{ chain = Chain.new {{ {chain} }} }})")).unwrap();
        let p = Patch::load(&d.0).unwrap();
        Graph::build(&p, sr, &p.default_param_values()).latency()
    }

    #[test]
    fn nonlinear_blocks_are_oversampled_automatically() {
        let two = latency("Blocks.Gain.new('g', { db = 0, oversampling = 2 })", 48_000.0);
        assert!(two > 0 && two < 48, "2x latency {two}");
        assert_eq!(latency("Blocks.Drive.new('d', { amount = 12 })", 48_000.0), two);
        assert_eq!(latency("Blocks.Drive.new('d', { amount = 12 })", 44_100.0), two);
        assert_eq!(latency("Blocks.Drive.new('d', { amount = 12 })", 96_000.0), 0);
        assert_eq!(latency("Blocks.Drive.new('d', { amount = 12, oversampling = 1 })", 48_000.0), 0);
        assert_eq!(latency("Blocks.Gain.new('g', { db = 0 })", 48_000.0), 0);
        // Neighbours share one up/down pair, also across a block without its own
        // setting; an explicit `oversampling = 1` in between splits them.
        assert_eq!(latency("Blocks.Drive.new('a', { amount = 6 }), Blocks.Drive.new('b', { amount = 6 })", 48_000.0), two);
        assert_eq!(
            latency("Blocks.Drive.new('a', { amount = 6 }), Blocks.LowPass.new('lp', { freq = 5000 }), Blocks.Drive.new('b', { amount = 6 })", 48_000.0),
            two
        );
        assert_eq!(
            latency("Blocks.Drive.new('a', { amount = 6 }), Blocks.Gain.new('g', { db = 0, oversampling = 1 }), Blocks.Drive.new('b', { amount = 6 })", 48_000.0),
            2 * two
        );
        assert!(latency("Blocks.Drive.new('a', { amount = 6 }), Blocks.Drive.new('b', { amount = 6, oversampling = 4 })", 48_000.0) > two);
        // Inside an oversampled block, Drive already runs at 96 kHz.
        assert_eq!(
            latency("Blocks.MidSide.new('ms', { oversampling = 2, mid = Chain.new { Blocks.Drive.new('d', { amount = 6 }) } })", 48_000.0),
            two
        );
        // Parallel: the longest branch.
        assert_eq!(
            latency("Blocks.Parallel.new('p', { branches = { Chain.new(), Chain.new { Blocks.Drive.new('d', { amount = 6 }) } } })", 48_000.0),
            two
        );
    }

    #[test]
    fn reported_latency_matches_the_signal() {
        for chain in [
            "Blocks.Gain.new('g', { db = 0, oversampling = 2 })",
            "Blocks.Gain.new('g', { db = 0, oversampling = 8 })",
            "Blocks.Parallel.new('p', { mix = 50, branches = { Chain.new(), Chain.new { Blocks.Gain.new('g', { db = 0, oversampling = 4 }) } } })",
            "Blocks.MidSide.new('ms', { oversampling = 2, side = Chain.new { Blocks.Gain.new('g', { db = 0, oversampling = 4 }) } })",
        ] {
            let lat = latency(chain, SR);
            let mut x = vec![[0.0f32; 2]; 2048];
            x[100] = [1.0, 1.0];
            let y = through(chain, x);
            let peak_at = y.iter().enumerate().max_by(|a, b| a.1[0].abs().total_cmp(&b.1[0].abs())).unwrap().0;
            assert_eq!(peak_at, 100 + lat, "{chain}");
            // A band-limited impulse peaks a little below 1 (its energy spreads).
            assert!(y[peak_at][0] > 0.9, "{chain}: peak {}", y[peak_at][0]);
        }
    }

    #[test]
    fn branches_with_different_latency_are_aligned() {
        // Dry + the same signal through an oversampled (= delayed) branch: without
        // compensation this would comb-filter (cancel at some frequencies).
        let os = "Chain.new { Blocks.Gain.new('g', { db = 0, oversampling = 2 }) }";
        let par = format!("Blocks.Parallel.new('p', {{ branches = {{ Chain.new(), {os} }} }})");
        let split = format!("Blocks.BandSplit.new('s', {{ crossovers = {{ 1000 }}, bands = {{ Chain.new(), {os} }} }})");
        let ms = format!("Blocks.MidSide.new('ms', {{ side = {os} }})");
        for freq in [100.0, 1000.0, 5000.0, 10_000.0, 15_000.0] {
            let sum = gain_through(&par, freq);
            assert!((sum - 6.0206).abs() < 0.05, "parallel {freq} Hz: {sum}");
            let flat = gain_through(&split, freq);
            assert!(flat.abs() < 0.05, "band split {freq} Hz: {flat}");
        }
        // Mid/side with a stereo signal: L and R come back unchanged (just delayed).
        let x: Vec<Frame> = sine(3000.0, -6.0, 0.2).into_iter().map(|f| [f[0], 0.3 * f[0]]).collect();
        let lat = latency(&ms, SR);
        let y = through(&ms, x.clone());
        for i in 4800..x.len() {
            assert!((y[i][0] - x[i - lat][0]).abs() < 1e-3 && (y[i][1] - x[i - lat][1]).abs() < 1e-3, "frame {i}");
        }
    }

    #[test]
    fn oversampling_keeps_linear_blocks_transparent() {
        for os in [2, 4, 8] {
            let g = gain_through(&format!("Blocks.PeakEq.new('e', {{ freq = 1000, q = 1, gain = 6, oversampling = {os} }})"), 1000.0);
            assert!((g - 6.0).abs() < 0.1, "{os}x: {g}");
        }
    }

    /// Level (dB re full scale) of `freq` in the left channel (Goertzel, Hann window).
    fn tone_db(x: &[Frame], freq: f32) -> f32 {
        let w = std::f32::consts::TAU * freq / SR;
        let c = 2.0 * w.cos() as f64;
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        let n = x.len();
        for (i, f) in x.iter().enumerate() {
            let win = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos();
            let s0 = f[0] as f64 * win + c * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        let power = s1 * s1 + s2 * s2 - c * s1 * s2;
        (20.0 * (power.sqrt() / (n as f64 / 4.0)).log10()) as f32
    }

    #[test]
    fn oversampled_drive_aliases_far_less() {
        // 7.5 kHz driven hard: the 5th harmonic (37.5 kHz) folds to 10.5 kHz at 48 kHz.
        let alias = |opts: &str| {
            let d = TempDir::new();
            std::fs::write(
                d.0.join("init.luau"),
                format!("return Patch.new('T', {{ chain = Chain.new {{ Blocks.Drive.new('d', {{ amount = 24 {opts} }}) }} }})"),
            )
            .unwrap();
            let p = Patch::load(&d.0).unwrap();
            let mut g = Graph::build(&p, SR, &p.default_param_values());
            let mut x = sine(7500.0, -6.0, 0.5);
            g.process_all(&[0.0; MACRO_SLOTS], &mut x);
            tone_db(&x[4800..], 10_500.0)
        };
        let plain = alias(", oversampling = 1");
        let auto = alias("");
        let os4 = alias(", oversampling = 4");
        eprintln!("alias at 10.5 kHz: 1x {plain:.1} dB, auto (2x) {auto:.1} dB, 4x {os4:.1} dB");
        assert!(plain > -40.0, "1x should alias audibly: {plain}");
        // ~38 dB better. 4x isn't better than 2x *here*: the 27th harmonic
        // (202.5 kHz) folds onto 10.5 kHz at both 96 and 192 kHz.
        assert!(auto < plain - 30.0, "2x: {auto} vs {plain}");
        assert!(os4 < plain - 30.0, "4x: {os4} vs {plain}");
    }

    #[test]
    fn bundled_patches_build() {
        let root = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../patches"));
        for dir in patch::discover(&root) {
            let p = Patch::load(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
            let mut g = Graph::build(&p, SR, &p.default_param_values());
            let mut x = sine(110.0, -12.0, 0.2);
            g.process_all(&[0.5; MACRO_SLOTS], &mut x);
            assert!(x.iter().all(|f| f[0].is_finite()), "{}", dir.display());
        }
    }
}
