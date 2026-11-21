//! Linear-phase filtering (`phase = "linear"` on EQ and filter blocks and `BandSplit`).
//!
//! A [`Convolver`] turns a magnitude response into a zero-phase FIR (frequency sampling, then a
//! Hann window; the window is 1 at the centre, so responses that sum to 1 give FIRs that sum to
//! an exact delay: a linear-phase band split reconstructs perfectly) and runs it by uniformly
//! partitioned overlap-save FFT convolution.
//!
//! Sizes: the FIR is ~170 ms long (8192 taps at 48 kHz: ~6 Hz resolution, enough for bass EQ),
//! the blocks 1/32 of it. Latency = half the FIR (the filter's delay) + one block (buffering):
//! 4352 samples (~91 ms) at 48 kHz, reported to the host.
//!
//! When the response changes (a knob moves, a modulator), the FIR is redesigned on the audio
//! thread, at most once per block, and the next block crossfades from the old filter to the new
//! one. Everything is preallocated: no allocation in `design` or `process`.

use std::sync::Arc;

use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};

use crate::oversample::Delay;
use crate::{Biquad, Ctx, EqShape, FILTER_STAGES, Frame, Node, P, filter_stages, process_nodes};

/// Partitions per FIR.
const PARTS: usize = 32;

/// FIR length at `sr`: a power of two near 170 ms.
pub(crate) fn fir_len(sr: f32) -> usize {
    ((sr * 0.17) as usize).next_power_of_two().clamp(1024, 32768)
}

pub(crate) struct Convolver {
    n: usize,
    b: usize,
    fft: Arc<dyn Fft<f32>>,
    ifft: Arc<dyn Fft<f32>>,
    design_ifft: Arc<dyn Fft<f32>>,
    scratch: Vec<Complex32>,
    /// The filter's partition spectra (PARTS × 2b), and the next one while `pending`.
    h: Vec<Complex32>,
    h_next: Vec<Complex32>,
    pending: bool,
    designed: bool,
    /// Per channel: the last 2b input samples, the spectra of the last PARTS input blocks (a
    /// ring), the block of output being played.
    input: [Vec<f32>; 2],
    fdl: [Vec<Complex32>; 2],
    out: [Vec<f32>; 2],
    /// Ring position in `fdl` of the newest block.
    fdl_pos: usize,
    /// Samples into the current block.
    pos: usize,
    work: Vec<Complex32>,
    acc: Vec<Complex32>,
    acc_next: Vec<Complex32>,
    design: Vec<Complex32>,
    window: Vec<f32>,
    sr: f32,
}

impl Convolver {
    pub(crate) fn new(sr: f32) -> Self {
        let n = fir_len(sr);
        let b = n / PARTS;
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(2 * b);
        let ifft = planner.plan_fft_inverse(2 * b);
        let design_ifft = planner.plan_fft_inverse(n);
        let scratch_len = fft.get_inplace_scratch_len().max(ifft.get_inplace_scratch_len()).max(design_ifft.get_inplace_scratch_len());
        let zero = Complex32::new(0.0, 0.0);
        let spectra = || vec![zero; PARTS * 2 * b];
        Convolver {
            n,
            b,
            fft,
            ifft,
            design_ifft,
            scratch: vec![zero; scratch_len],
            h: spectra(),
            h_next: spectra(),
            pending: false,
            designed: false,
            input: [vec![0.0; 2 * b], vec![0.0; 2 * b]],
            fdl: [spectra(), spectra()],
            out: [vec![0.0; b], vec![0.0; b]],
            fdl_pos: 0,
            pos: 0,
            work: vec![zero; 2 * b],
            acc: vec![zero; 2 * b],
            acc_next: vec![zero; 2 * b],
            design: vec![zero; n],
            window: (0..n).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos()).collect(),
            sr,
        }
    }

    pub(crate) fn latency(&self) -> usize {
        self.n / 2 + self.b
    }

    /// Whether a new response can be designed now (the last one has been taken up).
    pub(crate) fn can_design(&self) -> bool {
        !self.pending
    }

    /// Designs the FIR for `mag(freq Hz)` (linear magnitude, zero phase). The first design applies
    /// at once; later ones crossfade in over the next block.
    pub(crate) fn design(&mut self, mag: impl Fn(f32) -> f32) {
        let (n, b) = (self.n, self.b);
        let df = self.sr / n as f32;
        for k in 0..=n / 2 {
            let m = mag(k as f32 * df);
            let m = if m.is_finite() { m } else { 0.0 };
            self.design[k] = Complex32::new(m, 0.0);
            if k > 0 && k < n / 2 {
                self.design[n - k] = Complex32::new(m, 0.0);
            }
        }
        self.design_ifft.process_with_scratch(&mut self.design, &mut self.scratch[..self.design_ifft.get_inplace_scratch_len()]);
        // Zero phase (centred at 0, wrapping) → causal, centred at n/2, windowed.
        let target = if self.designed { &mut self.h_next } else { &mut self.h };
        for p in 0..PARTS {
            for i in 0..b {
                let t = p * b + i;
                let tap = self.design[(t + n / 2) % n].re / n as f32 * self.window[t];
                self.work[i] = Complex32::new(tap, 0.0);
                self.work[b + i] = Complex32::new(0.0, 0.0);
            }
            self.fft.process_with_scratch(&mut self.work, &mut self.scratch[..self.fft.get_inplace_scratch_len()]);
            target[p * 2 * b..(p + 1) * 2 * b].copy_from_slice(&self.work);
        }
        self.pending = self.designed;
        self.designed = true;
    }

    /// One frame in, the frame from `latency()` samples ago out.
    #[inline]
    pub(crate) fn process(&mut self, f: Frame) -> Frame {
        let b = self.b;
        let y = [self.out[0][self.pos], self.out[1][self.pos]];
        self.input[0][b + self.pos] = f[0];
        self.input[1][b + self.pos] = f[1];
        self.pos += 1;
        if self.pos == b {
            self.pos = 0;
            self.block();
        }
        y
    }

    /// A full block of input: convolve it, fill `out`.
    fn block(&mut self) {
        let b = self.b;
        let m = 2 * b;
        let fading = self.pending;
        let (fft_s, ifft_s) = (self.fft.get_inplace_scratch_len(), self.ifft.get_inplace_scratch_len());
        for ch in 0..2 {
            for (w, &x) in self.work.iter_mut().zip(&self.input[ch]) {
                *w = Complex32::new(x, 0.0);
            }
            self.fft.process_with_scratch(&mut self.work, &mut self.scratch[..fft_s]);
            let slot = self.fdl_pos * m;
            self.fdl[ch][slot..slot + m].copy_from_slice(&self.work);
            // Σ over partitions: input block (newest − p) × filter partition p.
            self.acc.fill(Complex32::new(0.0, 0.0));
            if fading {
                self.acc_next.fill(Complex32::new(0.0, 0.0));
            }
            for p in 0..PARTS {
                let s = ((self.fdl_pos + PARTS - p) % PARTS) * m;
                let x = &self.fdl[ch][s..s + m];
                let h = &self.h[p * m..(p + 1) * m];
                for ((a, &xi), &hi) in self.acc.iter_mut().zip(x).zip(h) {
                    *a += xi * hi;
                }
                if fading {
                    let h = &self.h_next[p * m..(p + 1) * m];
                    for ((a, &xi), &hi) in self.acc_next.iter_mut().zip(x).zip(h) {
                        *a += xi * hi;
                    }
                }
            }
            self.ifft.process_with_scratch(&mut self.acc, &mut self.scratch[..ifft_s]);
            let norm = 1.0 / m as f32;
            if fading {
                self.ifft.process_with_scratch(&mut self.acc_next, &mut self.scratch[..ifft_s]);
                for i in 0..b {
                    let t = i as f32 / b as f32;
                    self.out[ch][i] = norm * ((1.0 - t) * self.acc[b + i].re + t * self.acc_next[b + i].re);
                }
            } else {
                for i in 0..b {
                    self.out[ch][i] = norm * self.acc[b + i].re;
                }
            }
            self.input[ch].copy_within(b..m, 0);
        }
        self.fdl_pos = (self.fdl_pos + 1) % PARTS;
        if fading {
            std::mem::swap(&mut self.h, &mut self.h_next);
            self.pending = false;
        }
    }
}

// ---- blocks ------------------------------------------------------------------------------------

/// What a linear-phase EQ node reproduces: an EQ biquad, or a low/high-pass with a slope.
#[derive(Clone, Copy)]
pub(crate) enum Response {
    Eq(EqShape),
    Filter(EqShape),
}

/// An EQ or filter block with `phase = "linear"`: the same magnitude as the minimum-phase
/// block, no phase shift.
pub(crate) struct LinearEq {
    pub(crate) response: Response,
    pub(crate) freq: P,
    pub(crate) q: P,
    pub(crate) gain: P,
    pub(crate) slope: P,
    pub(crate) last: [f32; 4],
    pub(crate) conv: Convolver,
}

impl LinearEq {
    pub(crate) fn new(response: Response, freq: P, q: P, gain: P, slope: P, sr: f32) -> Self {
        LinearEq { response, freq, q, gain, slope, last: [f32::NAN; 4], conv: Convolver::new(sr) }
    }

    pub(crate) fn latency(&self) -> usize {
        self.conv.latency()
    }

    pub(crate) fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let cur = [self.freq.update(ctx), self.q.update(ctx), self.gain.update(ctx), self.slope.update(ctx)];
        // A change waits while the last one is still crossfading in; `last` keeps it pending.
        if cur != self.last && self.conv.can_design() {
            self.last = cur;
            let [freq, q, gain, slope] = cur;
            let sr = ctx.sr;
            match self.response {
                Response::Eq(shape) => {
                    let mut bq = Biquad::default();
                    bq.set(shape, freq, q, gain, sr);
                    self.conv.design(|f| bq.magnitude(f, sr));
                }
                Response::Filter(shape) => {
                    let (n, qs) = filter_stages(slope, q);
                    let mut bqs = [Biquad::default(); FILTER_STAGES];
                    for (bq, &sq) in bqs.iter_mut().zip(&qs).take(n) {
                        bq.set(shape, freq, sq, 0.0, sr);
                    }
                    self.conv.design(|f| bqs[..n].iter().map(|bq| bq.magnitude(f, sr)).product());
                }
            }
        }
        for f in frames {
            *f = self.conv.process(*f);
        }
    }
}

/// `BandSplit` with `phase = "linear"`: each band is its own linear-phase FIR. Band k's magnitude
/// is the difference of the LR4 low-pass magnitudes at its crossovers, so the bands sum to 1
/// and their FIRs to an exact delay: the split reconstructs the input perfectly.
pub(crate) struct LinearSplit {
    pub(crate) freqs: Vec<P>,
    pub(crate) last: Vec<f32>,
    /// This sub-block's crossovers (preallocated).
    pub(crate) cur: Vec<f32>,
    pub(crate) convs: Vec<Convolver>,
    pub(crate) bands: Vec<Vec<Node>>,
    pub(crate) bufs: Vec<Vec<Frame>>,
    /// Latency compensation of the band chains, and their common latency.
    pub(crate) align: Vec<Delay>,
    pub(crate) chain_latency: usize,
}

/// LR4 low-pass magnitude at `f` for crossover `fc` (a squared Butterworth).
fn lr4_low(f: f32, fc: f32) -> f32 {
    1.0 / (1.0 + (f / fc).powi(4))
}

impl LinearSplit {
    pub(crate) fn latency(&self) -> usize {
        self.convs[0].latency() + self.chain_latency
    }

    pub(crate) fn process(&mut self, frames: &mut [Frame], ctx: &Ctx) {
        let n = frames.len();
        let nx = self.freqs.len();
        // Crossovers driven by params may cross each other: keep them ascending.
        let mut prev = 10.0f32;
        for (c, f) in self.cur.iter_mut().zip(&mut self.freqs) {
            *c = f.update(ctx).max(prev);
            prev = *c;
        }
        if self.cur != self.last && self.convs.iter().all(Convolver::can_design) {
            self.last.copy_from_slice(&self.cur);
            let xs = &self.cur;
            for (k, conv) in self.convs.iter_mut().enumerate() {
                let low = |j: usize, f: f32| if j == 0 { 0.0 } else if j > nx { 1.0 } else { lr4_low(f, xs[j - 1]) };
                conv.design(|f| low(k + 1, f) - low(k, f));
            }
        }
        for (idx, frame) in frames.iter().enumerate() {
            for (conv, buf) in self.convs.iter_mut().zip(&mut self.bufs) {
                buf[idx] = conv.process(*frame);
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

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    /// Runs `x` (mono, both channels) through, compensating the latency.
    fn run(c: &mut Convolver, x: &[f32]) -> Vec<f32> {
        let lat = c.latency();
        let mut out = Vec::with_capacity(x.len());
        for i in 0..x.len() + lat {
            let s = x.get(i).copied().unwrap_or(0.0);
            let y = c.process([s, s]);
            if i >= lat {
                out.push(y[0]);
            }
        }
        out
    }

    #[test]
    fn flat_response_is_a_pure_delay() {
        let mut c = Convolver::new(SR);
        assert_eq!(c.latency(), 4096 + 256);
        c.design(|_| 1.0);
        let x: Vec<f32> = (0..20_000).map(|i| ((i * 7919) % 1000) as f32 / 500.0 - 1.0).collect();
        let y = run(&mut c, &x);
        let err = x.iter().zip(&y).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(err < 1e-3, "{err}");
    }

    #[test]
    fn impulse_response_is_symmetric() {
        let mut c = Convolver::new(SR);
        c.design(|f| 1.0 / (1.0 + (f / 1000.0).powi(4)));
        let mut x = vec![0.0; 9000];
        x[0] = 1.0;
        let mut out = Vec::new();
        for &s in &x {
            out.push(c.process([s, s])[0]);
        }
        // Centred at n/2 + b (the latency), mirror-symmetric around it.
        let centre = c.latency();
        for k in 1..2000 {
            assert!((out[centre - k] - out[centre + k]).abs() < 1e-5, "{k}");
        }
    }
}
