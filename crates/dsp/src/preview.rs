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
use crate::{Biquad, EqShape, comp_gr, db_to_gain, drive_curve};

/// The sample rate the responses are drawn at (shapes barely differ at others below ~15 kHz).
const SR: f32 = 48_000.0;

/// Magnitude (dB) at each of `freqs` (Hz), or `None` if the block isn't a filter/EQ.
pub fn response_db(kind: &BlockKind, v: &dyn Fn(&ParamRef) -> f32, freqs: &[f32]) -> Option<Vec<f32>> {
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
        _ => return None,
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
        BlockKind::Compressor(c) | BlockKind::FetCompressor(c) | BlockKind::OptoCompressor(c) => {
            let slope = 1.0 - 1.0 / v(&c.ratio).max(1.0);
            Some(comp_gr(level_db - v(&c.threshold), slope, v(&c.knee).max(0.0)))
        }
        BlockKind::Limiter { ceiling, .. } => Some((level_db - v(ceiling).min(0.0)).max(0.0)),
        _ => None,
    }
}

impl Biquad {
    /// |H(e^jω)| in dB at `freq`.
    fn magnitude_db(&self, freq: f32, sr: f32) -> f32 {
        let w = std::f32::consts::TAU * freq / sr;
        let (c1, s1) = (w.cos(), -w.sin());
        let (c2, s2) = ((2.0 * w).cos(), -(2.0 * w).sin());
        let num = (self.b0 + self.b1 * c1 + self.b2 * c2, self.b1 * s1 + self.b2 * s2);
        let den = (1.0 + self.a1 * c1 + self.a2 * c2, self.a1 * s1 + self.a2 * s2);
        let mag2 = (num.0 * num.0 + num.1 * num.1) / (den.0 * den.0 + den.1 * den.1).max(1e-20);
        10.0 * mag2.max(1e-20).log10()
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
        let kind = BlockKind::PeakEq(patch::chain::EqSpec { freq: r(0), gain: r(1), q: r(2) });
        let vals = [1000.0, 6.0, 1.0];
        let v = |p: &ParamRef| vals[p.0];
        let db = response_db(&kind, &v, &[20.0, 1000.0, 15000.0]).unwrap();
        assert!(db[0].abs() < 0.2 && db[2].abs() < 0.3, "{db:?}");
        assert!((db[1] - 6.0).abs() < 0.05, "{db:?}");
    }

    #[test]
    fn low_pass_is_3_db_down_at_cutoff_at_every_slope() {
        let kind = BlockKind::LowPass(patch::chain::FilterSpec { freq: r(0), q: r(1), slope: r(2) });
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
        let kind = BlockKind::HighPass(patch::chain::FilterSpec { freq: r(0), q: r(1), slope: r(2) });
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
