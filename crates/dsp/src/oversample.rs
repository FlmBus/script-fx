//! Oversampling: linear-phase FIR halfband filters (Kaiser-windowed), cascaded
//! 2× stages. Any block can be wrapped in an [`Oversampler`]; no block oversamples
//! on its own.
//!
//! Linear phase = a pure delay, so routing blocks can line branches up again
//! (latency compensation) and parallel blends don't comb-filter. The latency is
//! always a whole number of samples at the outer rate (odd remainders are padded
//! inside the oversampled domain).

use crate::Frame;

/// Kaiser-windowed halfband low-pass (cutoff at a quarter of the rate), `n` taps
/// (`n % 4 == 3`, so the outermost taps aren't zero), stopband ≈ `atten` dB.
fn halfband(n: usize, atten: f64) -> Vec<f64> {
    assert!(n % 4 == 3);
    let c = (n - 1) / 2;
    let beta = 0.1102 * (atten - 8.7);
    let i0 = |x: f64| {
        // Modified Bessel function of the first kind, order 0 (series).
        let (mut sum, mut term, mut k) = (1.0, 1.0, 1.0);
        while term > 1e-12 * sum {
            term *= (x / (2.0 * k)).powi(2);
            sum += term;
            k += 1.0;
        }
        sum
    };
    let mut h: Vec<f64> = (0..n)
        .map(|t| {
            let k = t as f64 - c as f64;
            if t == c {
                return 0.5;
            }
            if (t as isize - c as isize) % 2 == 0 {
                return 0.0;
            }
            let r = k / c as f64;
            let w = i0(beta * (1.0 - r * r).max(0.0).sqrt()) / i0(beta);
            (std::f64::consts::PI * k / 2.0).sin() / (std::f64::consts::PI * k) * w
        })
        .collect();
    // Exact halfband: the odd taps sum to 0.5 (unity gain at DC).
    let odd: f64 = h.iter().enumerate().filter(|(t, _)| *t != c).map(|(_, v)| v).sum();
    for (t, v) in h.iter_mut().enumerate() {
        if t != c {
            *v *= 0.5 / odd;
        }
    }
    h
}

/// Taps of the `stage`-th 2× stage (0 = closest to the outer rate). The first
/// stage keeps the audio band (≤ 0.41 × the outer rate, ~18 kHz at 44.1 kHz) and
/// rejects from 0.59 × (what would fold back into it); later stages only need to
/// reject images far above the audio band, so they're short.
fn stage_taps(stage: usize) -> Vec<f64> {
    match stage {
        0 => halfband(71, 100.0),
        1 => halfband(23, 100.0),
        _ => halfband(19, 100.0),
    }
}

/// A delay line for frames (latency compensation). Zero length = pass-through.
pub struct Delay {
    buf: Vec<Frame>,
    pos: usize,
}

impl Delay {
    pub fn new(samples: usize) -> Self {
        Delay { buf: vec![[0.0; 2]; samples], pos: 0 }
    }

    #[inline]
    pub fn process(&mut self, frames: &mut [Frame]) {
        if self.buf.is_empty() {
            return;
        }
        for f in frames {
            std::mem::swap(f, &mut self.buf[self.pos]);
            self.pos = (self.pos + 1) % self.buf.len();
        }
    }
}

/// History of frames, written twice so the last `len` are always contiguous.
struct History {
    buf: Vec<Frame>,
    len: usize,
    /// Index of the newest frame in the first copy.
    pos: usize,
}

impl History {
    fn new(len: usize) -> Self {
        History { buf: vec![[0.0; 2]; 2 * len], len, pos: 0 }
    }

    #[inline]
    fn push(&mut self, f: Frame) {
        self.pos += 1;
        if self.pos == self.len {
            self.pos = 0;
        }
        self.buf[self.pos] = f;
        self.buf[self.pos + self.len] = f;
    }

    /// The last `len` frames, oldest first (the newest is the last element).
    #[inline]
    fn window(&self) -> &[Frame] {
        &self.buf[self.pos + 1..self.pos + 1 + self.len]
    }
}

/// One 2× stage: an upsampler and a downsampler with the same halfband, in
/// polyphase form. With `n = 4k + 3` taps and center `c = 2k + 1`, the nonzero
/// taps besides the center are the even ones: `m = 2k + 2` symmetric taps.
/// - Up (gain 2): the even output phase is the symmetric `m`-tap FIR over the
///   input; the odd phase only meets the center tap: the input `k` frames ago.
/// - Down: the symmetric FIR over the even input frames, plus 0.5 × the odd
///   input frame `k + 1` pairs ago (the center tap).
struct Stage {
    /// First half of the symmetric taps (up: × 2).
    up_taps: Vec<f32>,
    down_taps: Vec<f32>,
    k: usize,
    up_hist: History,
    even_hist: History,
    odd_hist: History,
    /// Center tap index = delay of one filter, in high-rate samples.
    center: usize,
}

impl Stage {
    fn new(h: &[f64]) -> Self {
        let n = h.len();
        let k = (n - 3) / 4;
        let m = 2 * k + 2;
        let half: Vec<f64> = (0..m / 2).map(|j| h[2 * j]).collect();
        Stage {
            up_taps: half.iter().map(|v| (2.0 * v) as f32).collect(),
            down_taps: half.iter().map(|&v| v as f32).collect(),
            k,
            up_hist: History::new(m),
            even_hist: History::new(m),
            odd_hist: History::new(k + 2),
            center: (n - 1) / 2,
        }
    }

    /// Symmetric FIR over the last `m` frames (= the whole window): `taps[j]`
    /// applies to the frames `j` and `m - 1 - j` ago.
    #[inline]
    fn symmetric(taps: &[f32], hist: &History) -> Frame {
        let w = hist.window();
        let (old, new) = w.split_at(w.len() / 2);
        let mut acc = [0.0f32; 2];
        for ((c, a), b) in taps.iter().zip(new.iter().rev()).zip(old) {
            acc[0] += c * (a[0] + b[0]);
            acc[1] += c * (a[1] + b[1]);
        }
        acc
    }

    #[inline]
    fn up(&mut self, f: Frame) -> [Frame; 2] {
        self.up_hist.push(f);
        let w = self.up_hist.window();
        [Self::symmetric(&self.up_taps, &self.up_hist), w[w.len() - 1 - self.k]]
    }

    #[inline]
    fn down(&mut self, f0: Frame, f1: Frame) -> Frame {
        self.even_hist.push(f0);
        self.odd_hist.push(f1);
        let mut y = Self::symmetric(&self.down_taps, &self.even_hist);
        let w = self.odd_hist.window();
        let mid = w[w.len() - 1 - (self.k + 1)];
        y[0] += 0.5 * mid[0];
        y[1] += 0.5 * mid[1];
        y
    }
}

/// Up- and downsampler for a factor of 1, 2, 4 or 8 around something with
/// `inner_latency` samples of latency at the oversampled rate.
pub struct Oversampler {
    factor: usize,
    stages: Vec<Stage>,
    pad: Delay,
    latency: usize,
    a: Vec<Frame>,
    b: Vec<Frame>,
}

impl Oversampler {
    /// `max_frames` = the most frames per call at the outer rate.
    pub fn new(factor: usize, max_frames: usize, inner_latency: usize) -> Self {
        assert!(matches!(factor, 1 | 2 | 4 | 8), "oversampling factor {factor}");
        let n_stages = factor.trailing_zeros() as usize;
        let stages: Vec<Stage> = (0..n_stages).map(|s| Stage::new(&stage_taps(s))).collect();
        // Delay at the innermost rate: stage s runs at 2^(s+1) × the outer rate; its
        // up + down delay 2 × center samples there = center × 2^(n_stages - s) inside.
        let filters: usize = stages.iter().enumerate().map(|(s, st)| st.center << (n_stages - s)).sum();
        let total = filters + inner_latency;
        let pad = (factor - total % factor) % factor;
        Oversampler {
            factor,
            stages,
            pad: Delay::new(pad),
            latency: (total + pad) / factor,
            a: vec![[0.0; 2]; max_frames * factor],
            b: vec![[0.0; 2]; max_frames * factor],
        }
    }

    pub fn factor(&self) -> usize {
        self.factor
    }

    /// Latency at the outer rate (filters + the inner latency + padding).
    pub fn latency(&self) -> usize {
        self.latency
    }

    /// Upsamples `frames`, lets `f` process the oversampled signal in place, and
    /// downsamples the result back into `frames`. Real-time safe.
    pub fn run(&mut self, frames: &mut [Frame], f: impl FnOnce(&mut [Frame])) {
        if self.factor == 1 {
            f(frames);
            return;
        }
        let n = frames.len();
        self.a[..n].copy_from_slice(frames);
        let (mut cur, mut other) = (&mut self.a, &mut self.b);
        let mut len = n;
        for st in &mut self.stages {
            for i in 0..len {
                let [y0, y1] = st.up(cur[i]);
                other[2 * i] = y0;
                other[2 * i + 1] = y1;
            }
            std::mem::swap(&mut cur, &mut other);
            len *= 2;
        }
        f(&mut cur[..len]);
        self.pad.process(&mut cur[..len]);
        for st in self.stages.iter_mut().rev() {
            len /= 2;
            for i in 0..len {
                other[i] = st.down(cur[2 * i], cur[2 * i + 1]);
            }
            std::mem::swap(&mut cur, &mut other);
        }
        frames.copy_from_slice(&cur[..n]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Level (dB) of `freq` in `x` (mono, left channel) via a Goertzel filter.
    pub(crate) fn tone_db(x: &[Frame], freq: f32, sr: f32) -> f32 {
        let w = std::f32::consts::TAU * freq / sr;
        let c = 2.0 * w.cos();
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        // Hann window against leakage.
        let n = x.len();
        for (i, f) in x.iter().enumerate() {
            let win = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos();
            let s0 = (f[0] * win) as f64 + c as f64 * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        let power = s1 * s1 + s2 * s2 - c as f64 * s1 * s2;
        // Normalize: a full-scale sine with a Hann window → amplitude n/4.
        (20.0 * (power.sqrt() / (n as f64 / 4.0)).log10()) as f32
    }

    fn sine(freq: f32, sr: f32, n: usize) -> Vec<Frame> {
        (0..n)
            .map(|i| {
                let x = (std::f32::consts::TAU * freq * i as f32 / sr).sin();
                [x, x]
            })
            .collect()
    }

    /// Runs `x` through up → identity → down in 32-frame calls.
    fn round_trip(factor: usize, x: &mut [Frame]) {
        let mut os = Oversampler::new(factor, 32, 0);
        for chunk in x.chunks_mut(32) {
            os.run(chunk, |_| {});
        }
    }

    #[test]
    fn latency_is_a_whole_number_of_samples_and_linear_phase() {
        for factor in [2, 4, 8] {
            for inner in [0, 1, 3] {
                let mut os = Oversampler::new(factor, 32, 0);
                let lat = os.latency();
                let mut pad = Delay::new(inner);
                let mut wrapped = Oversampler::new(factor, 32, inner);
                let mut x = vec![[0.0f32; 2]; 1024];
                x[10] = [1.0, 1.0];
                let mut y = x.clone();
                for chunk in x.chunks_mut(32) {
                    os.run(chunk, |_| {});
                }
                for chunk in y.chunks_mut(32) {
                    wrapped.run(chunk, |buf| pad.process(buf));
                }
                let peak = |v: &[Frame]| v.iter().enumerate().max_by(|a, b| a.1[0].abs().total_cmp(&b.1[0].abs())).unwrap().0;
                assert_eq!(peak(&x), 10 + lat, "{factor}x");
                assert_eq!(peak(&y), 10 + wrapped.latency(), "{factor}x inner {inner}");
                // Linear phase: symmetric around the peak.
                let p = 10 + lat;
                for k in 1..8 {
                    assert!((x[p - k][0] - x[p + k][0]).abs() < 1e-4, "{factor}x asymmetric at ±{k}");
                }
            }
        }
    }

    #[test]
    fn passband_is_flat() {
        for factor in [2, 4, 8] {
            for freq in [100.0, 1000.0, 10_000.0, 18_000.0] {
                let mut x = sine(freq, 44_100.0, 44_100);
                round_trip(factor, &mut x);
                let db = tone_db(&x[4410..], freq, 44_100.0);
                assert!(db.abs() < 0.1, "{factor}x {freq} Hz: {db} dB");
            }
        }
    }

    #[test]
    fn rejects_content_above_the_base_nyquist() {
        // A tone generated at the high rate above the base Nyquist must not fold back.
        for factor in [2usize, 4, 8] {
            let sr = 48_000.0;
            let mut os = Oversampler::new(factor, 32, 0);
            let hi = 30_000.0; // would alias to 18 kHz
            let mut out = vec![[0.0f32; 2]; 48_000];
            let mut t = 0usize;
            for chunk in out.chunks_mut(32) {
                os.run(chunk, |buf| {
                    for f in buf.iter_mut() {
                        let x = (std::f32::consts::TAU * hi * t as f32 / (sr * factor as f32)).sin();
                        *f = [x, x];
                        t += 1;
                    }
                });
            }
            let alias = tone_db(&out[4800..], sr - hi, sr);
            assert!(alias < -90.0, "{factor}x: alias at {} Hz is {alias} dB", sr - hi);
        }
    }

    #[test]
    fn upsampling_leaves_no_images() {
        let sr = 48_000.0;
        let mut os = Oversampler::new(2, 32, 0);
        let mut x = sine(5000.0, sr, 48_000);
        let mut high = Vec::new();
        for chunk in x.chunks_mut(32) {
            os.run(chunk, |buf| high.extend_from_slice(buf));
        }
        let image = tone_db(&high[9600..], sr - 5000.0, 2.0 * sr); // mirrored around the base rate
        let tone = tone_db(&high[9600..], 5000.0, 2.0 * sr);
        assert!(tone.abs() < 0.1, "tone {tone}");
        assert!(image < -90.0, "image {image}");
    }
}
