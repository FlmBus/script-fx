//! Offline analysis of a captured stereo snippet → compact, LLM-friendly JSON.
//!
//! Units are part of every key name. Values that can't be measured (e.g. loudness
//! of silence) are `null`. Runs off the audio thread; allocation is fine here.

use std::f32::consts::TAU;

use rustfft::num_complex::Complex32;
use rustfft::FftPlanner;
use serde::Serialize;

pub type Frame = [f32; 2];

#[derive(Debug, Clone, Serialize)]
pub struct Analysis {
    pub meta: Meta,
    pub loudness: Loudness,
    pub spectrum: Spectrum,
    pub dynamics: Dynamics,
    pub stereo: Stereo,
    pub pitch: Pitch,
}

#[derive(Debug, Clone, Serialize)]
pub struct Meta {
    pub duration_s: f32,
    pub sample_rate_hz: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Loudness {
    pub integrated_lufs: Option<f32>,
    pub short_term_max_lufs: Option<f32>,
    pub momentary_max_lufs: Option<f32>,
    pub loudness_range_lu: Option<f32>,
    pub true_peak_dbtp: Option<f32>,
    pub sample_peak_dbfs: Option<f32>,
    pub rms_dbfs: Option<f32>,
    /// Sample peak − RMS.
    pub crest_factor_db: Option<f32>,
    /// Peak-to-loudness ratio: true peak − integrated loudness.
    pub plr_db: Option<f32>,
    /// K-system readings (RMS relative to the K-20/14/12 zero point).
    pub k20_db: Option<f32>,
    pub k14_db: Option<f32>,
    pub k12_db: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Band {
    pub hz: f32,
    pub db: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Resonance {
    pub hz: f32,
    /// How far the narrow peak sticks out above the octave-smoothed curve.
    pub prominence_db: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct BandShares {
    pub sub_20_60hz: f32,
    pub low_60_250hz: f32,
    pub low_mid_250_500hz: f32,
    pub mid_500_2khz: f32,
    pub presence_2_6khz: f32,
    pub air_6_20khz: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Spectrum {
    /// Long-term average level per 1/3-octave band, dBFS (RMS); bands sum to `loudness.rms_dbfs`.
    pub third_octave: Vec<Band>,
    /// Linear-regression slope of the 1/3-octave curve between 100 Hz and 10 kHz.
    pub tilt_db_per_octave: Option<f32>,
    pub centroid_hz: Option<f32>,
    /// Share of total energy per region, percent.
    pub energy_share_percent: BandShares,
    /// Narrow peaks, strongest first. Tonal sources also show their notes/harmonics here.
    pub resonances: Vec<Resonance>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BandDynamics {
    pub level_dbfs: Option<f32>,
    /// p95 − p10 of the band's short-term level (~85 ms frames).
    pub spread_db: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Dynamics {
    /// Short-term (400 ms) RMS percentiles over active parts, dBFS.
    pub short_term_p10_dbfs: Option<f32>,
    pub short_term_p50_dbfs: Option<f32>,
    pub short_term_p95_dbfs: Option<f32>,
    /// p95 − p10: how much the level moves (playing dynamics, note-to-note evenness).
    pub level_spread_db: Option<f32>,
    /// Level of the quietest parts (5th percentile of all 400 ms windows).
    pub noise_floor_dbfs: Option<f32>,
    pub onsets_per_second: f32,
    /// Median 10 %→90 % rise time of detected onsets (smaller = sharper attacks).
    pub attack_time_ms: Option<f32>,
    pub low_below_250hz: BandDynamics,
    pub mid_250_2khz: BandDynamics,
    pub high_above_2khz: BandDynamics,
}

#[derive(Debug, Clone, Serialize)]
pub struct Stereo {
    pub mono: bool,
    /// Pearson correlation L/R (1 = mono, 0 = unrelated, < 0 = phase problems).
    pub correlation: Option<f32>,
    /// Side vs. mid energy (−∞ = mono, 0 = as much side as mid).
    pub side_to_mid_db: Option<f32>,
    /// Left vs. right energy (positive = louder left).
    pub balance_db: Option<f32>,
    /// Correlation below 150 Hz (should be close to 1 for a solid low end).
    pub low_end_correlation: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Pitch {
    /// Share of active frames with a clear single pitch (low for chords/drums/noise).
    pub pitched_percent: f32,
    pub lowest_note: Option<String>,
    pub highest_note: Option<String>,
    pub median_note: Option<String>,
    pub lowest_hz: Option<f32>,
    pub highest_hz: Option<f32>,
}

fn r1(x: f32) -> Option<f32> {
    x.is_finite().then(|| (x * 10.0).round() / 10.0)
}

fn r2(x: f32) -> Option<f32> {
    x.is_finite().then(|| (x * 100.0).round() / 100.0)
}

fn pow_db(p: f32) -> f32 {
    10.0 * p.log10()
}

fn percentile(sorted: &[f32], p: f32) -> f32 {
    if sorted.is_empty() {
        return f32::NAN;
    }
    let i = ((sorted.len() - 1) as f32 * p).round() as usize;
    sorted[i]
}

fn sorted(mut v: Vec<f32>) -> Vec<f32> {
    v.sort_by(|a, b| a.total_cmp(b));
    v
}

/// Analyzes a stereo capture.
pub fn analyze(frames: &[Frame], sample_rate: f32) -> Analysis {
    let mono: Vec<f32> = frames.iter().map(|f| 0.5 * (f[0] + f[1])).collect();
    let stft = Stft::new(&mono, sample_rate, 8192, 4096);
    let loudness = loudness(frames, sample_rate);
    Analysis {
        meta: Meta { duration_s: r2(frames.len() as f32 / sample_rate).unwrap_or(0.0), sample_rate_hz: sample_rate },
        spectrum: spectrum(&stft, sample_rate),
        dynamics: dynamics(frames, &mono, &stft, sample_rate),
        stereo: stereo(frames, sample_rate),
        pitch: pitch(&mono, sample_rate),
        loudness,
    }
}

// ---------------------------------------------------------------- loudness

fn loudness(frames: &[Frame], sr: f32) -> Loudness {
    use ebur128::{EbuR128, Mode};
    let mut ebu = EbuR128::new(2, sr as u32, Mode::I | Mode::LRA | Mode::TRUE_PEAK | Mode::SAMPLE_PEAK | Mode::S | Mode::M)
        .expect("valid ebur128 config");
    let (mut st_max, mut m_max) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    let chunk = (sr * 0.1) as usize;
    let mut interleaved = Vec::with_capacity(chunk * 2);
    for c in frames.chunks(chunk) {
        interleaved.clear();
        interleaved.extend(c.iter().flat_map(|f| [f[0], f[1]]));
        let _ = ebu.add_frames_f32(&interleaved);
        if let Ok(v) = ebu.loudness_shortterm() {
            st_max = st_max.max(v);
        }
        if let Ok(v) = ebu.loudness_momentary() {
            m_max = m_max.max(v);
        }
    }
    let get = |r: Result<f64, ebur128::Error>| r.map(|v| v as f32).unwrap_or(f32::NAN);
    let tp = get(ebu.true_peak(0)).max(get(ebu.true_peak(1)));
    let sp = get(ebu.sample_peak(0)).max(get(ebu.sample_peak(1)));
    let integrated = get(ebu.loudness_global());
    let ms = frames.iter().map(|f| 0.5 * (f[0] * f[0] + f[1] * f[1])).sum::<f32>() / frames.len().max(1) as f32;
    let rms_db = pow_db(ms);
    let tp_db = 20.0 * tp.log10();
    let sp_db = 20.0 * sp.log10();
    Loudness {
        integrated_lufs: r1(integrated),
        short_term_max_lufs: r1(st_max as f32),
        momentary_max_lufs: r1(m_max as f32),
        loudness_range_lu: r1(get(ebu.loudness_range())),
        true_peak_dbtp: r1(tp_db),
        sample_peak_dbfs: r1(sp_db),
        rms_dbfs: r1(rms_db),
        crest_factor_db: r1(sp_db - rms_db),
        plr_db: r1(tp_db - integrated),
        k20_db: r1(rms_db + 20.0),
        k14_db: r1(rms_db + 14.0),
        k12_db: r1(rms_db + 12.0),
    }
}

// ---------------------------------------------------------------- spectrum

/// Short-time power spectra (Hann window), one-sided, scaled so that summing
/// a frame's bins gives that frame's mean-square value (Parseval).
struct Stft {
    n: usize,
    frames: Vec<Vec<f32>>,
    sr: f32,
}

impl Stft {
    fn new(x: &[f32], sr: f32, n: usize, hop: usize) -> Self {
        let fft = FftPlanner::<f32>::new().plan_fft_forward(n);
        let win: Vec<f32> = (0..n).map(|i| 0.5 - 0.5 * (TAU * i as f32 / n as f32).cos()).collect();
        let wsum2: f32 = win.iter().map(|w| w * w).sum();
        let mut frames = Vec::new();
        let mut buf = vec![Complex32::new(0.0, 0.0); n];
        let mut start = 0;
        while start + n <= x.len().max(n) {
            for i in 0..n {
                let s = x.get(start + i).copied().unwrap_or(0.0);
                buf[i] = Complex32::new(s * win[i], 0.0);
            }
            fft.process(&mut buf);
            let scale = 2.0 / (n as f32 * wsum2);
            frames.push((0..=n / 2).map(|k| buf[k].norm_sqr() * scale).collect());
            start += hop;
            if x.len() < n {
                break;
            }
        }
        Stft { n, frames, sr }
    }

    fn bin_hz(&self, k: usize) -> f32 {
        k as f32 * self.sr / self.n as f32
    }

    fn bins(&self, lo: f32, hi: f32) -> std::ops::Range<usize> {
        let k = |f: f32| ((f * self.n as f32 / self.sr).round() as usize).clamp(1, self.n / 2);
        k(lo)..k(hi).max(k(lo) + 1)
    }

    fn average(&self) -> Vec<f32> {
        let mut avg = vec![0.0; self.n / 2 + 1];
        for f in &self.frames {
            for (a, p) in avg.iter_mut().zip(f) {
                *a += p;
            }
        }
        let n = self.frames.len().max(1) as f32;
        avg.iter_mut().for_each(|a| *a /= n);
        avg
    }
}

fn third_octave_centers() -> Vec<f32> {
    (-17..=13).map(|n| 1000.0 * 2f32.powf(n as f32 / 3.0)).collect()
}

fn spectrum(stft: &Stft, sr: f32) -> Spectrum {
    let avg = stft.average();
    let total: f32 = avg[1..].iter().sum();
    let band_power = |lo: f32, hi: f32| avg[stft.bins(lo, hi.min(sr / 2.0))].iter().sum::<f32>();

    let centers = third_octave_centers();
    let third_octave: Vec<Band> = centers
        .iter()
        .map(|&c| Band {
            hz: if c < 100.0 { (c * 10.0).round() / 10.0 } else { c.round() },
            db: r1(pow_db(band_power(c / 2f32.powf(1.0 / 6.0), c * 2f32.powf(1.0 / 6.0)))),
        })
        .collect();

    // Tilt: regression of band level over log2(f), 100 Hz..10 kHz, audible bands only.
    let pts: Vec<(f32, f32)> = centers
        .iter()
        .zip(&third_octave)
        .filter(|(c, b)| **c >= 100.0 && **c <= 10_000.0 && b.db.is_some_and(|d| d > -100.0))
        .map(|(c, b)| (c.log2(), b.db.unwrap()))
        .collect();
    let tilt = if pts.len() >= 3 {
        let n = pts.len() as f32;
        let (mx, my) = (pts.iter().map(|p| p.0).sum::<f32>() / n, pts.iter().map(|p| p.1).sum::<f32>() / n);
        let cov: f32 = pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
        let var: f32 = pts.iter().map(|p| (p.0 - mx).powi(2)).sum();
        cov / var
    } else {
        f32::NAN
    };

    let centroid = avg.iter().enumerate().skip(1).map(|(k, p)| stft.bin_hz(k) * p).sum::<f32>() / total;
    let share = |lo: f32, hi: f32| ((band_power(lo, hi) / total * 1000.0).round() / 10.0).max(0.0);

    Spectrum {
        third_octave,
        tilt_db_per_octave: r1(tilt),
        centroid_hz: centroid.is_finite().then(|| centroid.round()),
        energy_share_percent: BandShares {
            sub_20_60hz: share(20.0, 60.0),
            low_60_250hz: share(60.0, 250.0),
            low_mid_250_500hz: share(250.0, 500.0),
            mid_500_2khz: share(500.0, 2000.0),
            presence_2_6khz: share(2000.0, 6000.0),
            air_6_20khz: share(6000.0, 20_000.0),
        },
        resonances: resonances(&avg, stft),
    }
}

/// Narrow peaks: 1/24-octave smoothed level vs. 1-octave smoothed level.
fn resonances(avg: &[f32], stft: &Stft) -> Vec<Resonance> {
    // Evaluate on a log-frequency grid (48 points per octave, 30 Hz..12 kHz).
    let grid: Vec<f32> = (0..)
        .map(|i| 30.0 * 2f32.powf(i as f32 / 48.0))
        .take_while(|f| *f < 12_000.0)
        .collect();
    let smooth = |f: f32, oct: f32| {
        let r = stft.bins(f / 2f32.powf(oct / 2.0), f * 2f32.powf(oct / 2.0));
        let n = r.len().max(1) as f32;
        pow_db(avg[r].iter().sum::<f32>() / n)
    };
    let narrow: Vec<f32> = grid.iter().map(|&f| smooth(f, 1.0 / 24.0)).collect();
    let broad: Vec<f32> = grid.iter().map(|&f| smooth(f, 1.0)).collect();
    let floor = narrow.iter().copied().fold(f32::NEG_INFINITY, f32::max) - 60.0;
    let mut peaks: Vec<Resonance> = (1..grid.len() - 1)
        .filter(|&i| narrow[i] > narrow[i - 1] && narrow[i] >= narrow[i + 1] && narrow[i] > floor)
        .map(|i| Resonance { hz: grid[i].round(), prominence_db: narrow[i] - broad[i] })
        .filter(|r| r.prominence_db > 4.0)
        .collect();
    peaks.sort_by(|a, b| b.prominence_db.total_cmp(&a.prominence_db));
    peaks.truncate(8);
    peaks.iter_mut().for_each(|r| r.prominence_db = r1(r.prominence_db).unwrap_or(0.0));
    peaks
}

// ---------------------------------------------------------------- dynamics

fn dynamics(frames: &[Frame], mono: &[f32], stft: &Stft, sr: f32) -> Dynamics {
    // 400 ms windows, 100 ms hop.
    let (win, hop) = ((0.4 * sr) as usize, (0.1 * sr) as usize);
    let mut levels = Vec::new();
    let mut i = 0;
    while i + win <= frames.len() {
        let ms = frames[i..i + win].iter().map(|f| 0.5 * (f[0] * f[0] + f[1] * f[1])).sum::<f32>() / win as f32;
        levels.push(pow_db(ms.max(1e-12)));
        i += hop;
    }
    let all = sorted(levels.clone());
    let active = sorted(levels.into_iter().filter(|l| *l > -70.0).collect());
    let (p10, p50, p95) = (percentile(&active, 0.1), percentile(&active, 0.5), percentile(&active, 0.95));

    let band = |lo: f32, hi: f32| {
        let r = stft.bins(lo, hi);
        let lv = sorted(
            stft.frames
                .iter()
                .map(|f| pow_db(f[r.clone()].iter().sum::<f32>().max(1e-12)))
                .filter(|l| *l > -80.0)
                .collect(),
        );
        let total = stft.frames.iter().map(|f| f[r.clone()].iter().sum::<f32>()).sum::<f32>()
            / stft.frames.len().max(1) as f32;
        BandDynamics {
            level_dbfs: r1(pow_db(total)),
            spread_db: r1(percentile(&lv, 0.95) - percentile(&lv, 0.1)),
        }
    };

    let (onsets, attack) = onsets(mono, sr);
    Dynamics {
        short_term_p10_dbfs: r1(p10),
        short_term_p50_dbfs: r1(p50),
        short_term_p95_dbfs: r1(p95),
        level_spread_db: r1(p95 - p10),
        noise_floor_dbfs: r1(percentile(&all, 0.05)),
        onsets_per_second: r2(onsets as f32 / (frames.len() as f32 / sr)).unwrap_or(0.0),
        attack_time_ms: r1(attack),
        low_below_250hz: band(20.0, 250.0),
        mid_250_2khz: band(250.0, 2000.0),
        high_above_2khz: band(2000.0, sr / 2.0),
    }
}

/// Envelope-based onsets (robust for low notes, whose cycles are longer than
/// short FFT frames): an onset is a ≥ 2.5 dB rise within 15 ms on a follower with
/// 1 ms attack / 150 ms release. Returns (count, median 10→90 % rise time in ms).
fn onsets(x: &[f32], sr: f32) -> (usize, f32) {
    let att = (-1.0 / (0.001 * sr)).exp();
    let rel = (-1.0 / (0.150 * sr)).exp();
    let step = (0.001 * sr).max(1.0) as usize; // evaluate every 1 ms
    let mut env = Vec::with_capacity(x.len() / step + 1);
    let mut e = 0.0f32;
    for (i, s) in x.iter().enumerate() {
        let a = s.abs();
        let c = if a > e { att } else { rel };
        e = c * e + (1.0 - c) * a;
        if i % step == 0 {
            env.push(e);
        }
    }
    let db: Vec<f32> = env.iter().map(|v| 20.0 * v.max(1e-6).log10()).collect();
    let peak_db = db.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let (look, min_gap) = (15usize, 60usize); // ms
    let mut onset_ms: Vec<usize> = Vec::new();
    let mut i = look;
    while i < db.len() {
        let rise = db[i] - db[i - look];
        if rise >= 2.5 && db[i] > peak_db - 50.0 && onset_ms.last().is_none_or(|&l| i - l >= min_gap) {
            onset_ms.push(i);
            i += min_gap;
            continue;
        }
        i += 1;
    }

    // Rise time 10 % → 90 % of the local peak within 80 ms after the onset.
    let mut rises = Vec::new();
    for &o in &onset_ms {
        let s0 = o.saturating_sub(look * 2);
        let s1 = (o + 80).min(env.len());
        let (pk_i, pk) = (s0..s1).fold((s0, 0.0f32), |a, i| if env[i] > a.1 { (i, env[i]) } else { a });
        let t90 = (s0..=pk_i).find(|&i| env[i] >= 0.9 * pk);
        let t10 = (s0..=pk_i).rev().find(|&i| env[i] < 0.1 * pk).map(|i| i + 1).or(Some(s0));
        if let (Some(a), Some(b)) = (t10, t90) {
            if b >= a {
                rises.push((b - a) as f32 * step as f32 / sr * 1000.0);
            }
        }
    }
    (onset_ms.len(), percentile(&sorted(rises), 0.5))
}

// ---------------------------------------------------------------- stereo

fn corr(a: impl Iterator<Item = (f32, f32)> + Clone) -> f32 {
    let (mut sxy, mut sxx, mut syy) = (0.0f64, 0.0f64, 0.0f64);
    for (x, y) in a {
        sxy += (x * y) as f64;
        sxx += (x * x) as f64;
        syy += (y * y) as f64;
    }
    (sxy / (sxx * syy).sqrt()) as f32
}

fn stereo(frames: &[Frame], sr: f32) -> Stereo {
    let (mut mid, mut side, mut l, mut r) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for f in frames {
        let (m, s) = (0.5 * (f[0] + f[1]), 0.5 * (f[0] - f[1]));
        mid += (m * m) as f64;
        side += (s * s) as f64;
        l += (f[0] * f[0]) as f64;
        r += (f[1] * f[1]) as f64;
    }
    let mono = side <= mid * 1e-9;
    // Low end: 4th-order lowpass at 150 Hz on both channels.
    let mut lp = [Biquad::lowpass(150.0, sr), Biquad::lowpass(150.0, sr)];
    let mut lp2 = [Biquad::lowpass(150.0, sr), Biquad::lowpass(150.0, sr)];
    let low: Vec<(f32, f32)> = frames
        .iter()
        .map(|f| (lp2[0].tick(lp[0].tick(f[0])), lp2[1].tick(lp[1].tick(f[1]))))
        .collect();
    Stereo {
        mono,
        correlation: r2(corr(frames.iter().map(|f| (f[0], f[1])))),
        side_to_mid_db: if mono { None } else { r1(10.0 * (side / mid).log10() as f32) },
        balance_db: r1(10.0 * (l / r).log10() as f32),
        low_end_correlation: r2(corr(low.iter().copied())),
    }
}

/// 2nd-order Butterworth lowpass (TDF2).
struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    z: [f32; 2],
}

impl Biquad {
    fn lowpass(freq: f32, sr: f32) -> Self {
        let w0 = TAU * freq / sr;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
        let a0 = 1.0 + alpha;
        Biquad {
            b: [(1.0 - cos) / 2.0 / a0, (1.0 - cos) / a0, (1.0 - cos) / 2.0 / a0],
            a: [-2.0 * cos / a0, (1.0 - alpha) / a0],
            z: [0.0; 2],
        }
    }
    fn tick(&mut self, x: f32) -> f32 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

// ---------------------------------------------------------------- pitch

const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

pub fn note_name(hz: f32) -> String {
    let midi = (69.0 + 12.0 * (hz / 440.0).log2()).round() as i32;
    format!("{}{}", NOTE_NAMES[midi.rem_euclid(12) as usize], midi.div_euclid(12) - 1)
}

/// YIN on a decimated mono signal, 30 Hz..1 kHz.
fn pitch(x: &[f32], sr: f32) -> Pitch {
    let dec = (sr / 12_000.0).round().max(1.0) as usize;
    let sr2 = sr / dec as f32;
    let mut lp = [Biquad::lowpass(sr2 * 0.4, sr), Biquad::lowpass(sr2 * 0.4, sr)];
    let y: Vec<f32> = x
        .iter()
        .map(|s| {
            let a = lp[0].tick(*s);
            lp[1].tick(a)
        })
        .step_by(dec)
        .collect();

    let (frame, hop) = (1024usize, 256usize);
    let (tau_min, tau_max) = ((sr2 / 1000.0) as usize, (sr2 / 30.0) as usize);
    let w = frame - tau_max;
    let mut d = vec![0.0f32; tau_max + 1];
    let (mut active, mut f0s) = (0usize, Vec::new());
    let mut start = 0;
    while start + frame <= y.len() {
        let s = &y[start..start + frame];
        let rms = (s.iter().map(|v| v * v).sum::<f32>() / frame as f32).sqrt();
        start += hop;
        if rms < 10f32.powf(-50.0 / 20.0) {
            continue;
        }
        active += 1;
        for tau in 1..=tau_max {
            d[tau] = (0..w).map(|j| (s[j] - s[j + tau]).powi(2)).sum();
        }
        // Cumulative mean normalized difference.
        let mut run = 0.0;
        let mut cmnd = vec![1.0f32; tau_max + 1];
        for tau in 1..=tau_max {
            run += d[tau];
            cmnd[tau] = if run > 0.0 { d[tau] * tau as f32 / run } else { 1.0 };
        }
        let found = (tau_min.max(2)..tau_max).find(|&t| cmnd[t] < 0.15 && cmnd[t] <= cmnd[t + 1]);
        if let Some(t) = found {
            // Parabolic interpolation around the minimum.
            let (a, b, c) = (cmnd[t - 1], cmnd[t], cmnd[t + 1]);
            let shift = 0.5 * (a - c) / (a - 2.0 * b + c);
            let tau = t as f32 + if shift.is_finite() { shift.clamp(-1.0, 1.0) } else { 0.0 };
            f0s.push(sr2 / tau);
        }
    }
    let f = sorted(f0s);
    let some = !f.is_empty();
    let (lo, hi, med) = (percentile(&f, 0.05), percentile(&f, 0.95), percentile(&f, 0.5));
    Pitch {
        pitched_percent: if active > 0 { (f.len() as f32 / active as f32 * 1000.0).round() / 10.0 } else { 0.0 },
        lowest_note: some.then(|| note_name(lo)),
        highest_note: some.then(|| note_name(hi)),
        median_note: some.then(|| note_name(med)),
        lowest_hz: if some { r1(lo) } else { None },
        highest_hz: if some { r1(hi) } else { None },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const SR: f32 = 48_000.0;

    fn sine(freq: f32, peak_db: f32, secs: f32) -> Vec<Frame> {
        let a = 10f32.powf(peak_db / 20.0);
        (0..(secs * SR) as usize)
            .map(|i| {
                let x = a * (TAU * freq * i as f32 / SR).sin();
                [x, x]
            })
            .collect()
    }

    fn bass_line(secs: f32) -> Vec<Frame> {
        let notes = [41.2, 55.0, 49.0, 61.7];
        let mut phase = 0.0f32;
        (0..(secs * SR) as usize)
            .map(|i| {
                let t = i as f32 / SR;
                let step = (t / 0.25) as usize;
                let local = t - step as f32 * 0.25;
                phase = (phase + notes[step % 4] / SR).fract();
                let x = 0.5 * (2.0 * phase - 1.0) * (-local * 6.0).exp() * (1.0 - (-local * 800.0).exp());
                [x, x]
            })
            .collect()
    }

    #[test]
    fn loudness_of_reference_sine() {
        // Stereo 1 kHz sine at -20 dBFS peak ≈ -20 LUFS (K-weighting ≈ +0.7 dB at 1 kHz).
        let a = analyze(&sine(1000.0, -20.0, 5.0), SR);
        let l = &a.loudness;
        assert!((l.integrated_lufs.unwrap() + 20.0).abs() < 0.2, "{l:?}");
        assert!((l.sample_peak_dbfs.unwrap() + 20.0).abs() < 0.1);
        assert!((l.rms_dbfs.unwrap() + 23.0).abs() < 0.1);
        assert!((l.crest_factor_db.unwrap() - 3.0).abs() < 0.1);
        assert!(a.stereo.mono && a.stereo.correlation.unwrap() > 0.99);
    }

    #[test]
    fn spectrum_finds_the_sine() {
        let a = analyze(&sine(1000.0, -20.0, 3.0), SR);
        let peak = a.spectrum.third_octave.iter().max_by(|x, y| x.db.unwrap_or(-200.0).total_cmp(&y.db.unwrap_or(-200.0))).unwrap();
        assert_eq!(peak.hz, 1000.0);
        // bands sum to the RMS (Parseval): the 1 kHz band alone ≈ -23 dBFS
        assert!((peak.db.unwrap() + 23.0).abs() < 0.3, "{peak:?}");
        assert!(a.spectrum.energy_share_percent.mid_500_2khz > 99.0);
    }

    #[test]
    fn bass_line_pitch_onsets_and_balance() {
        let a = analyze(&bass_line(8.0), SR);
        assert_eq!(a.pitch.lowest_note.as_deref(), Some("E1"), "{:?}", a.pitch);
        assert_eq!(a.pitch.highest_note.as_deref(), Some("B1"), "{:?}", a.pitch);
        assert!(a.pitch.pitched_percent > 70.0, "{:?}", a.pitch);
        assert!((a.dynamics.onsets_per_second - 4.0).abs() < 0.6, "{:?}", a.dynamics);
        assert!(a.spectrum.energy_share_percent.low_60_250hz > 40.0, "{:?}", a.spectrum.energy_share_percent);
        assert!(a.dynamics.level_spread_db.unwrap() > 1.0);
    }

    #[test]
    fn note_names() {
        assert_eq!(note_name(440.0), "A4");
        assert_eq!(note_name(41.2), "E1");
    }
}
