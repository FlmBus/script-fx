//! What a block does, as curves for the UI (the block page's display). Computed with the same
//! formulas the DSP runs, so the picture can't drift from the sound.
//!
//! Every function takes the block and `v`, which gives a param's current plain value (after
//! modulation), and returns `None` for blocks it has no picture for.
//!
//! - [`response_db`]: magnitude response of the filter/EQ blocks (one biquad each).
//! - [`transfer`]: static input → output curve of the waveshaping blocks (incl. their mix).
//! - [`dynamics`]: static level → gain-reduction curve of the compressors and the limiter.

use patch::chain::{BlockKind, ParamRef};

use crate::fx::distortion::{clip, fold, lut_at};
use crate::{Biquad, EqShape, db_to_gain, drive_curve};

/// The sample rate the responses are drawn at (shapes barely differ at others below ~15 kHz).
const SR: f32 = 48_000.0;

/// Magnitude (dB) at each of `freqs` (Hz), or `None` if the block isn't a filter/EQ.
pub fn response_db(kind: &BlockKind, v: &dyn Fn(&ParamRef) -> f32, freqs: &[f32]) -> Option<Vec<f32>> {
    sweep_db(kind, v, freqs, 0.0)
}

/// [`response_db`] with a swept block's LFO held at `sweep` (−1..1; flanger, phaser).
pub fn sweep_db(kind: &BlockKind, v: &dyn Fn(&ParamRef) -> f32, freqs: &[f32], sweep: f32) -> Option<Vec<f32>> {
    let (shape, freq, q, gain) = match kind {
        BlockKind::PeakEq(e) => (EqShape::Peak, v(&e.freq), v(&e.q), v(&e.gain)),
        BlockKind::LowShelf(e) => (EqShape::LowShelf, v(&e.freq), v(&e.q), v(&e.gain)),
        BlockKind::HighShelf(e) => (EqShape::HighShelf, v(&e.freq), v(&e.q), v(&e.gain)),
        BlockKind::LowPass(f) | BlockKind::HighPass(f) => {
            let shape = if matches!(kind, BlockKind::LowPass(_)) { EqShape::LowPass } else { EqShape::HighPass };
            let (n, qs) = crate::filter_stages(v(&f.slope), v(&f.q));
            let stages: Vec<Biquad> = qs[..n].iter().map(|&q| {
                let mut bq = Biquad::default();
                bq.set(shape, v(&f.freq), q, 0.0, SR);
                bq
            }).collect();
            return Some(freqs.iter().map(|&f| stages.iter().map(|bq| bq.magnitude_db(f, SR)).sum()).collect());
        }
        _ => return Some(freqs.iter().map(|&f| Some(db_of(linear(kind, v, f as f64, sweep)?))).collect::<Option<Vec<f32>>>()?),
    };
    let mut bq = Biquad::default();
    bq.set(shape, freq, q, gain, SR);
    Some(freqs.iter().map(|&f| bq.magnitude_db(f, SR)).collect())
}

/// Output for input `x` (−1..1), or `None` if the block isn't a waveshaper. The wavefolder's DC
/// blocker is left out (it only removes the bias' offset over time).
pub fn transfer(kind: &BlockKind, v: &dyn Fn(&ParamRef) -> f32, x: f32) -> Option<f32> {
    let wet = |m: &ParamRef| (v(m) / 100.0).clamp(0.0, 1.0);
    let mixed = |y: f32, w: f32| x + w * (y - x);
    Some(match kind {
        BlockKind::Drive { amount, mix } => mixed(drive_curve(x, db_to_gain(v(amount))), wet(mix)),
        BlockKind::Waveshaper { curve, drive, output, mix } => {
            mixed(lut_at(curve, x * db_to_gain(v(drive))) * db_to_gain(v(output)), wet(mix))
        }
        BlockKind::Wavefolder { amount, bias, mix } => {
            mixed(fold(x, db_to_gain(v(amount)), (v(bias) / 100.0).clamp(-1.0, 1.0)), wet(mix))
        }
        BlockKind::Clipper { ceiling, softness, drive } => {
            clip(x, db_to_gain(v(drive)), db_to_gain(v(ceiling)).max(1e-6), (v(softness) / 100.0).clamp(0.0, 1.0))
        }
        _ => return None,
    })
}

/// Static gain reduction (dB, ≥ 0) at input level `level_db`, or `None` if the block isn't a
/// compressor or the limiter. Attack/release and the opto's program dependence aren't static,
/// so they're not in it.
pub fn dynamics(kind: &BlockKind, v: &dyn Fn(&ParamRef) -> f32, level_db: f32) -> Option<f32> {
    match kind {
        BlockKind::Compressor(c) | BlockKind::FetCompressor(c) | BlockKind::OptoCompressor(c) | BlockKind::VariMuCompressor(c) => {
            Some(crate::fx::comp_static(level_db - v(&c.threshold), v(&c.ratio), v(&c.knee).max(0.0), (v(&c.ratio_curve) / 100.0).clamp(0.0, 1.0)))
        }
        BlockKind::Limiter { ceiling, .. } => Some((level_db - v(ceiling).min(0.0)).max(0.0)),
        _ => None,
    }
}

// ---- the other filters' transfer functions (small signal, the structures in `fx`) --------------

#[derive(Clone, Copy)]
struct C(f64, f64);

impl std::ops::Add for C {
    type Output = C;
    fn add(self, o: C) -> C {
        C(self.0 + o.0, self.1 + o.1)
    }
}
impl std::ops::Mul for C {
    type Output = C;
    fn mul(self, o: C) -> C {
        C(self.0 * o.0 - self.1 * o.1, self.0 * o.1 + self.1 * o.0)
    }
}
impl std::ops::Div for C {
    type Output = C;
    fn div(self, o: C) -> C {
        let d = (o.0 * o.0 + o.1 * o.1).max(1e-30);
        C((self.0 * o.0 + self.1 * o.1) / d, (self.1 * o.0 - self.0 * o.1) / d)
    }
}

fn re(x: f64) -> C {
    C(x, 0.0)
}

fn db_of(h: C) -> f32 {
    (20.0 * h.0.hypot(h.1).max(1e-6).log10()) as f32
}

/// `n` samples of delay at `f` Hz.
fn delay(f: f64, n: f64) -> C {
    let w = std::f64::consts::TAU * f / SR as f64 * n;
    C(w.cos(), -w.sin())
}

fn blend(wet: f64, h: C) -> C {
    re(1.0 - wet) + re(wet) * h
}

/// H at `f` Hz of the blocks whose response isn't a plain biquad.
fn linear(kind: &BlockKind, v: &dyn Fn(&ParamRef) -> f32, f: f64, sweep: f32) -> Option<C> {
    use std::f64::consts::PI;
    let sr = SR as f64;
    let pct = |r: &ParamRef| (v(r) as f64 / 100.0).clamp(0.0, 1.0);
    let signed = |r: &ParamRef| (v(r) as f64 / 100.0).clamp(-0.99, 0.99);
    // The TPT structures' s at `f` for a cutoff `fc` (bilinear, prewarped at fc).
    let s_at = |fc: f64| C(0.0, (PI * f / sr).tan() / (PI * fc.clamp(10.0, sr * 0.45) / sr).tan());
    Some(match kind {
        BlockKind::MorphFilter { freq, morph, resonance } => {
            let (m, r) = (pct(morph), pct(resonance));
            let k = 1.0 / (0.5 + r * r * 24.5);
            let s = s_at(v(freq) as f64);
            let den = s * s + re(k) * s + re(1.0);
            let (wl, wb, wh) = if m < 0.5 { (1.0 - 2.0 * m, 2.0 * m, 0.0) } else { (0.0, 2.0 - 2.0 * m, 2.0 * m - 1.0) };
            (re(wl) + re(wb * k) * s + re(wh) * s * s) / den
        }
        BlockKind::LadderFilter { freq, resonance, drive } => {
            let k = 4.0 * pct(resonance);
            let gd = db_to_gain(v(drive)) as f64;
            let g = re(1.0) / (re(1.0) + s_at(v(freq) as f64));
            let g4 = g * g * g * g;
            re(gd * (1.0 + 0.5 * k) / gd.sqrt()) * g4 / (re(1.0) + re(k) * delay(f, 1.0) * g4)
        }
        BlockKind::CombFilter { freq, feedback, mix } => {
            let fb = signed(feedback);
            let d = (sr / (v(freq) as f64).max(1.0)).max(1.0);
            blend(pct(mix), re(1.0 - fb.abs()) / (re(1.0) + re(-fb) * delay(f, d)))
        }
        BlockKind::Formant { vowel, shift, resonance, mix } => {
            let vw = v(vowel).clamp(0.0, 4.0);
            let k = 2f32.powf(v(shift).clamp(-24.0, 24.0) / 12.0);
            let q = 2.0 + pct(resonance) as f32 * 18.0;
            let i = (vw.floor() as usize).min(3);
            let t = vw - i as f32;
            let (fa, aa) = crate::fx::filter::VOWELS[i];
            let (fb, ab) = crate::fx::filter::VOWELS[i + 1];
            let mut sum = re(0.0);
            for b in 0..3 {
                let mut bq = Biquad::default();
                bq.set(EqShape::BandPass, (fa[b] + t * (fb[b] - fa[b])) * k, q, 0.0, SR);
                let (r, im) = bq.response(f, sr);
                sum = sum + re((aa[b] + t * (ab[b] - aa[b])) as f64) * C(r, im);
            }
            blend(pct(mix), re(((q / 4.0).sqrt()) as f64) * sum)
        }
        BlockKind::Resonator { notes, decay, mix } => {
            let r = (-6.907_755 / (v(decay).clamp(10.0, 30_000.0) as f64 * 0.001 * sr)).exp();
            let b0 = (1.0 - r * r) / 2.0;
            let mut sum = re(0.0);
            for hz in notes {
                let w = std::f64::consts::TAU * (*hz as f64).clamp(10.0, sr * 0.45) / sr;
                let den = re(1.0) + re(-2.0 * r * w.cos()) * delay(f, 1.0) + re(r * r) * delay(f, 2.0);
                sum = sum + re(b0) * (re(1.0) + re(-1.0) * delay(f, 2.0)) / den;
            }
            blend(pct(mix), re(1.0 / (notes.len().max(1) as f64).sqrt()) * sum)
        }
        BlockKind::Flanger { depth, feedback, mix, .. } => {
            let lfo = 0.5 + 0.5 * sweep as f64;
            let d = (0.0003 + pct(depth) * 0.005 * lfo) * sr;
            let z = delay(f, d);
            blend(pct(mix), z / (re(1.0) + re(-signed(feedback)) * z * delay(f, 1.0)))
        }
        BlockKind::Phaser { depth, feedback, freq, stages, mix, .. } => {
            let fc = ((v(freq) as f64).clamp(20.0, sr * 0.2) * 2f64.powf(pct(depth) * 2.0 * sweep as f64)).clamp(10.0, sr * 0.45);
            let t = (PI * fc / sr).tan();
            let a = (t - 1.0) / (t + 1.0);
            let z = delay(f, 1.0);
            let stage = (re(a) + z) / (re(1.0) + re(a) * z);
            let mut chain = re(1.0);
            for _ in 0..stages.unwrap_or(4) {
                chain = chain * stage;
            }
            blend(pct(mix), chain / (re(1.0) + re(-signed(feedback)) * z * chain))
        }
        _ => return None,
    })
}

impl Biquad {
    /// |H(e^jω)| in dB at `freq`.
    fn magnitude_db(&self, freq: f32, sr: f32) -> f32 {
        20.0 * self.magnitude(freq, sr).max(1e-10).log10()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(i: usize) -> ParamRef {
        ParamRef(i)
    }

    #[test]
    fn peak_eq_response_peaks_at_its_gain() {
        let kind = BlockKind::PeakEq(patch::chain::EqSpec { freq: r(0), gain: r(1), q: r(2), phase: Default::default() });
        let vals = [1000.0, 6.0, 1.0];
        let v = |p: &ParamRef| vals[p.0];
        let db = response_db(&kind, &v, &[20.0, 1000.0, 15000.0]).unwrap();
        assert!(db[0].abs() < 0.2 && db[2].abs() < 0.3, "{db:?}");
        assert!((db[1] - 6.0).abs() < 0.05, "{db:?}");
    }

    #[test]
    fn low_pass_is_3_db_down_at_cutoff_at_every_slope() {
        let kind = BlockKind::LowPass(patch::chain::FilterSpec { freq: r(0), q: r(1), slope: r(2), phase: Default::default() });
        for slope in [12.0, 24.0, 36.0, 48.0] {
            let vals = [1000.0, std::f32::consts::FRAC_1_SQRT_2, slope];
            let v = |p: &ParamRef| vals[p.0];
            let db = response_db(&kind, &v, &[100.0, 1000.0, 4000.0]).unwrap();
            assert!(db[0].abs() < 0.1 && (db[1] + 3.01).abs() < 0.15, "{slope}: {db:?}");
            // Two octaves above: about the slope × 2 down (a little less near the cutoff).
            assert!((db[2] + 2.0 * slope).abs() < 2.5, "{slope}: {db:?}");
        }
    }

    #[test]
    fn high_pass_resonance_peaks_at_cutoff() {
        let kind = BlockKind::HighPass(patch::chain::FilterSpec { freq: r(0), q: r(1), slope: r(2), phase: Default::default() });
        let vals = [1000.0, 4.0, 24.0];
        let v = |p: &ParamRef| vals[p.0];
        let db = response_db(&kind, &v, &[1000.0, 10000.0]).unwrap();
        assert!(db[0] > 6.0 && db[1].abs() < 0.5, "{db:?}");
    }

    #[test]
    fn limiter_reduces_above_its_ceiling() {
        let kind = BlockKind::Limiter { ceiling: r(0), release: r(1), lookahead: None };
        let v = |p: &ParamRef| [-1.0, 80.0][p.0];
        assert_eq!(dynamics(&kind, &v, -10.0), Some(0.0));
        assert_eq!(dynamics(&kind, &v, 2.0), Some(3.0));
    }
}
