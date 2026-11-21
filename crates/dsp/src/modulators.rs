//! Modulation sources: the fixed set of modulators, Follow, LFO and Step, as plain DSP.
//!
//! Each source turns its settings plus either the host transport ([`Transport`]) or an input
//! signal ([`Follow`]) into a control signal, one value per frame of the caller's buffer. There is
//! no UI and no patch knowledge here: the caller routes the outputs to assignments.
//!
//! Real-time rules, as in the rest of `dsp`: `process` does no allocation and takes no locks, all
//! state is fixed-size, and any chunk length works (0 and 1 included). Settings are clamped on the
//! way in ([`LfoSettings::sanitized`] and friends; `new` and `set_settings` apply them), and
//! non-finite values fall back to their defaults. A non-finite input sample counts as silence and a
//! non-finite host beat as "stopped".
//!
//! Smoothing: amount, offset, gain and floor glide over 10 ms, so moving a knob while playing
//! doesn't click. Times in ms are one-pole time constants (63 % of the way after that long).
//!
//! Timing: a [`Rate::Hz`] runs free. A [`Rate::Beats`] follows the host beat while the host plays
//! (so it lines up with the bar), and otherwise keeps running free at the host tempo from where it
//! was, so stopping the host doesn't make it jump.

/// Number of step levels in a [`Step`] sequence.
pub const STEPS: usize = 16;
/// Range of [`Rate::Hz`], in Hz.
pub const RATE_HZ: (f32, f32) = (0.01, 50.0);
/// Range of [`Rate::Beats`], in quarter notes (1/16 = 0.25, a bar in 4/4 = 4).
pub const RATE_BEATS: (f32, f32) = (1.0 / 16.0, 64.0);
/// Glide time of setting changes (amount, offset, gain, floor), ms.
const SETTING_GLIDE_MS: f32 = 10.0;

/// Host timing for one `process` call.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transport {
    /// Tempo in BPM. Non-finite or at most 1 counts as 120.
    pub tempo: f32,
    /// Position in quarter notes at the call's first frame. `None` when the host is stopped or
    /// doesn't report a position (non-finite counts as `None`).
    pub beat: Option<f64>,
}

impl Transport {
    /// No host transport: 120 BPM, stopped.
    pub const STOPPED: Transport = Transport { tempo: 120.0, beat: None };

    fn bpm(&self) -> f64 {
        if self.tempo.is_finite() && self.tempo > 1.0 { self.tempo as f64 } else { 120.0 }
    }

    fn playing_beat(&self) -> Option<f64> {
        self.beat.filter(|b| b.is_finite())
    }
}

/// How fast a modulator cycles (LFO) or steps (Step).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Rate {
    /// Free running, in cycles (or steps) per second. Clamped to [`RATE_HZ`].
    Hz(f32),
    /// Quarter notes per cycle (or step): 1 = a quarter note, 0.25 = 1/16. Clamped to
    /// [`RATE_BEATS`]. Locked to the host beat while the host plays; otherwise free at the host tempo.
    Beats(f32),
}

impl Rate {
    fn sanitized(self) -> Rate {
        match self {
            Rate::Hz(hz) => Rate::Hz(bounded(hz, RATE_HZ, 1.0)),
            Rate::Beats(beats) => Rate::Beats(bounded(beats, RATE_BEATS, 1.0)),
        }
    }
}

/// `v` clamped to `range`, or `default` when `v` isn't finite.
fn bounded(v: f32, (lo, hi): (f32, f32), default: f32) -> f32 {
    if v.is_finite() { v.clamp(lo, hi) } else { default }
}

/// The sample rate as f64; non-finite or below 1 Hz counts as 48 kHz.
fn rate_of(sample_rate: f32) -> f64 {
    if sample_rate.is_finite() && sample_rate >= 1.0 { sample_rate as f64 } else { 48_000.0 }
}

/// One-pole coefficient for a time constant of `ms` at `sr`; 0 ms means no smoothing.
fn one_pole(ms: f32, sr: f64) -> f32 {
    if ms <= 0.0 { 1.0 } else { (1.0 - (-1.0 / (ms as f64 * 1e-3 * sr)).exp()) as f32 }
}

/// A value that moves towards its target by a one-pole step per frame.
#[derive(Clone, Copy, Debug)]
struct Glide(f32);

impl Glide {
    #[inline]
    fn next(&mut self, target: f32, c: f32) -> f32 {
        self.0 += c * (target - self.0);
        self.0
    }
}

/// Linear gain of a level in dB.
fn gain_of(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Position in cycles (LFO) or steps (Step): free running, or locked to the host beat.
#[derive(Clone, Copy, Debug, Default)]
struct Clock {
    /// Position at the next frame, while running free.
    pos: f64,
}

impl Clock {
    /// Position at frame `i` of the current call; the free position moves on by one frame.
    #[inline]
    fn at(&mut self, rate: Rate, t: &Transport, i: usize, sr: f64) -> f64 {
        // Quarter notes per frame.
        let per_frame = t.bpm() / 60.0 / sr;
        match (rate, t.playing_beat()) {
            (Rate::Hz(hz), _) => {
                let p = self.pos;
                self.pos += hz as f64 / sr;
                p
            }
            (Rate::Beats(beats), Some(beat)) => {
                let beats = beats as f64;
                let p = (beat + i as f64 * per_frame) / beats;
                self.pos = p + per_frame / beats;
                p
            }
            (Rate::Beats(beats), None) => {
                let p = self.pos;
                self.pos += per_frame / beats as f64;
                p
            }
        }
    }
}

// ---------------------------------------------------------------- Follow

/// Follow settings. The envelope follows the peak of its input: the larger of |left| and |right|,
/// each capped at full scale (so hot input doesn't stretch the release). A non-finite channel counts
/// as silence.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FollowSettings {
    /// Time constant of rises, ms (0–1000).
    pub attack_ms: f32,
    /// How long the envelope stays at a peak before it releases, ms (0–1000).
    pub hold_ms: f32,
    /// Time constant of falls, ms (0–5000).
    pub release_ms: f32,
    /// Gain on the envelope, dB (−24…+24).
    pub gain_db: f32,
    /// Output while the envelope is at zero (0–1).
    pub floor: f32,
    /// Output smoothing (time constant), ms (0–1000).
    pub smooth_ms: f32,
}

impl Default for FollowSettings {
    fn default() -> Self {
        FollowSettings { attack_ms: 10.0, hold_ms: 0.0, release_ms: 150.0, gain_db: 0.0, floor: 0.0, smooth_ms: 0.0 }
    }
}

impl FollowSettings {
    /// These settings clamped into their ranges; non-finite values become the default.
    pub fn sanitized(self) -> Self {
        let d = Self::default();
        FollowSettings {
            attack_ms: bounded(self.attack_ms, (0.0, 1000.0), d.attack_ms),
            hold_ms: bounded(self.hold_ms, (0.0, 1000.0), d.hold_ms),
            release_ms: bounded(self.release_ms, (0.0, 5000.0), d.release_ms),
            gain_db: bounded(self.gain_db, (-24.0, 24.0), d.gain_db),
            floor: bounded(self.floor, (0.0, 1.0), d.floor),
            smooth_ms: bounded(self.smooth_ms, (0.0, 1000.0), d.smooth_ms),
        }
    }
}

/// Envelope follower: peak detection with attack, hold and release, then gain, floor and smoothing.
/// Output is unipolar, 0–1.
pub struct Follow {
    sr: f64,
    settings: FollowSettings,
    /// Detector envelope, linear, ≥ 0.
    env: f32,
    /// Frames left to hold the envelope at its peak.
    hold: usize,
    gain: Glide,
    floor: Glide,
    out: Glide,
}

impl Follow {
    pub fn new(sample_rate: f32, settings: FollowSettings) -> Self {
        let s = settings.sanitized();
        Follow {
            sr: rate_of(sample_rate),
            settings: s,
            env: 0.0,
            hold: 0,
            gain: Glide(gain_of(s.gain_db)),
            floor: Glide(s.floor),
            out: Glide(s.floor),
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sr = rate_of(sample_rate);
    }

    pub fn settings(&self) -> FollowSettings {
        self.settings
    }

    pub fn set_settings(&mut self, settings: FollowSettings) {
        self.settings = settings.sanitized();
    }

    /// Drops the envelope and the hold (e.g. when the patch is loaded).
    pub fn reset(&mut self) {
        self.env = 0.0;
        self.hold = 0;
    }

    /// The last output value, 0–1.
    pub fn level(&self) -> f32 {
        self.out.0
    }

    /// Follows `input` (stereo frames, the same layout as `dsp::Frame`) into `out`, one value per
    /// frame. Processes `min(input.len(), out.len())` frames.
    pub fn process(&mut self, input: &[[f32; 2]], out: &mut [f32]) {
        let s = self.settings;
        let sr = self.sr;
        let c_attack = one_pole(s.attack_ms, sr);
        let c_release = one_pole(s.release_ms, sr);
        let c_smooth = one_pole(s.smooth_ms, sr);
        let c_glide = one_pole(SETTING_GLIDE_MS, sr);
        let hold_frames = (s.hold_ms as f64 * 1e-3 * sr).round() as usize;
        let gain = gain_of(s.gain_db);
        for (x, y) in input.iter().zip(out.iter_mut()) {
            // A non-finite sample would poison the envelope for good, so it counts as silence.
            let left = if x[0].is_finite() { x[0].abs().min(1.0) } else { 0.0 };
            let right = if x[1].is_finite() { x[1].abs().min(1.0) } else { 0.0 };
            let peak = left.max(right);
            if peak >= self.env {
                self.env += c_attack * (peak - self.env);
                self.hold = hold_frames;
            } else if self.hold > 0 {
                self.hold -= 1;
            } else {
                self.env += c_release * (peak - self.env);
            }
            let g = self.gain.next(gain, c_glide);
            let floor = self.floor.next(s.floor, c_glide);
            *y = self.out.next((floor + g * self.env).clamp(0.0, 1.0), c_smooth);
        }
    }
}

// ---------------------------------------------------------------- LFO

/// LFO settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LfoSettings {
    /// Cycle length, in Hz or beats.
    pub rate: Rate,
    /// Depth, −1…+1. Negative flips the wave.
    pub amount: f32,
    /// Wave, 0–1: 0 sine, ⅓ triangle, ⅔ saw, 1 square. Between two neighbours the waves crossfade.
    pub shape: f32,
    /// Phase offset in degrees, wrapped into 0–360.
    pub phase: f32,
    /// Output smoothing (time constant), ms (0–1000). Softens the saw and square edges.
    pub smooth_ms: f32,
    /// Fade-in after construction or [`Lfo::reset`], ms (0–10000).
    pub fade_in_ms: f32,
}

impl Default for LfoSettings {
    fn default() -> Self {
        LfoSettings { rate: Rate::Hz(1.0), amount: 1.0, shape: 0.0, phase: 0.0, smooth_ms: 0.0, fade_in_ms: 0.0 }
    }
}

impl LfoSettings {
    /// These settings clamped into their ranges; non-finite values become the default.
    pub fn sanitized(self) -> Self {
        let d = Self::default();
        LfoSettings {
            rate: self.rate.sanitized(),
            amount: bounded(self.amount, (-1.0, 1.0), d.amount),
            shape: bounded(self.shape, (0.0, 1.0), d.shape),
            phase: if self.phase.is_finite() { self.phase.rem_euclid(360.0) } else { d.phase },
            smooth_ms: bounded(self.smooth_ms, (0.0, 1000.0), d.smooth_ms),
            fade_in_ms: bounded(self.fade_in_ms, (0.0, 10_000.0), d.fade_in_ms),
        }
    }
}

/// One bipolar wave at cycle position `p` (0–1): 0 sine, 1 triangle, 2 saw, 3 square.
fn wave(k: usize, p: f32) -> f32 {
    match k {
        0 => (std::f32::consts::TAU * p).sin(),
        // 0 at the start, +1 at ¼, 0 at ½, −1 at ¾: the sine's phase.
        1 => {
            if p < 0.25 {
                4.0 * p
            } else if p < 0.75 {
                2.0 - 4.0 * p
            } else {
                4.0 * p - 4.0
            }
        }
        // A ramp from −1 up to +1 over the cycle, dropping back at its end.
        2 => 2.0 * p - 1.0,
        _ => {
            if p < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
    }
}

/// Continuous morph over the four waves: `shape` 0–1 runs sine → triangle → saw → square, each
/// step a linear crossfade between neighbours. Public so the editor draws exactly what plays.
pub fn morph(shape: f32, p: f32) -> f32 {
    let s = shape.clamp(0.0, 1.0) * 3.0;
    let k = (s as usize).min(2);
    let f = s - k as f32;
    let a = wave(k, p);
    a + (wave(k + 1, p) - a) * f
}

/// A free-running (or beat-locked) LFO. Output is bipolar, −1…+1 scaled by `amount`.
pub struct Lfo {
    sr: f64,
    settings: LfoSettings,
    clock: Clock,
    /// Fade-in progress, 0–1.
    fade: f32,
    amount: Glide,
    shape: Glide,
    lag: Glide,
    /// Cycle position (0–1, offset included) at the last frame.
    phase: f32,
}

impl Lfo {
    pub fn new(sample_rate: f32, settings: LfoSettings) -> Self {
        let s = settings.sanitized();
        Lfo {
            sr: rate_of(sample_rate),
            settings: s,
            clock: Clock::default(),
            fade: 0.0,
            amount: Glide(s.amount),
            shape: Glide(s.shape),
            lag: Glide(0.0),
            phase: 0.0,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sr = rate_of(sample_rate);
    }

    pub fn settings(&self) -> LfoSettings {
        self.settings
    }

    /// Phase and shape changes take effect at the next frame (shape and amount glide over 10 ms).
    /// A phase change is a jump, not a glide.
    pub fn set_settings(&mut self, settings: LfoSettings) {
        self.settings = settings.sanitized();
    }

    /// Restarts the free-running cycle at 0 (plus the phase offset) and the fade-in.
    pub fn reset(&mut self) {
        self.clock = Clock::default();
        self.fade = 0.0;
    }

    /// The cycle position (0–1, offset included) at the last frame, for display.
    pub fn phase(&self) -> f32 {
        self.phase
    }

    /// Writes `out.len()` frames of the LFO for the host timing `t`.
    pub fn process(&mut self, t: &Transport, out: &mut [f32]) {
        let s = self.settings;
        let sr = self.sr;
        let c_glide = one_pole(SETTING_GLIDE_MS, sr);
        let c_lag = one_pole(s.smooth_ms, sr);
        let fade_step = if s.fade_in_ms > 0.0 { (1.0 / (s.fade_in_ms as f64 * 1e-3 * sr)) as f32 } else { 1.0 };
        let offset = (s.phase / 360.0) as f64;
        for (i, y) in out.iter_mut().enumerate() {
            let cycle = self.clock.at(s.rate, t, i, sr);
            let p = (cycle + offset).rem_euclid(1.0) as f32;
            self.phase = p;
            self.fade = (self.fade + fade_step).min(1.0);
            let wave = morph(self.shape.next(s.shape, c_glide), p);
            let depth = self.amount.next(s.amount, c_glide);
            *y = self.lag.next(depth * wave * self.fade, c_lag);
        }
    }
}

// ---------------------------------------------------------------- Step

/// Step sequencer settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StepSettings {
    /// Level of each step, 0–1: 0 plays −amount, 0.5 plays 0, 1 plays +amount. Only the first
    /// `length` steps are played.
    pub values: [f32; STEPS],
    /// Steps before the pattern wraps, 1–16.
    pub length: usize,
    /// Time per step: Hz (steps per second) or beats per step.
    pub rate: Rate,
    /// Glide into each step as a fraction of the step's length, 0–1. 0 jumps; 1 slides across the
    /// whole step, arriving at its level as the next step starts.
    pub glide: f32,
    /// Swing, 0–1. 0 is straight; 1 delays every other step by a third of a step (triplet feel).
    /// Steps are counted on the clock, so with an odd `length` the swing alternates from cycle to
    /// cycle.
    pub swing: f32,
    /// Depth, −1…+1.
    pub amount: f32,
    /// Added to the output, −1…+1.
    pub offset: f32,
}

impl Default for StepSettings {
    fn default() -> Self {
        StepSettings {
            values: [0.5; STEPS],
            length: 8,
            rate: Rate::Beats(0.25),
            glide: 0.0,
            swing: 0.0,
            amount: 1.0,
            offset: 0.0,
        }
    }
}

impl StepSettings {
    /// These settings clamped into their ranges; non-finite values become the default.
    pub fn sanitized(self) -> Self {
        let d = Self::default();
        StepSettings {
            values: self.values.map(|v| bounded(v, (0.0, 1.0), 0.5)),
            length: self.length.clamp(1, STEPS),
            rate: self.rate.sanitized(),
            glide: bounded(self.glide, (0.0, 1.0), d.glide),
            swing: bounded(self.swing, (0.0, 1.0), d.swing),
            amount: bounded(self.amount, (-1.0, 1.0), d.amount),
            offset: bounded(self.offset, (-1.0, 1.0), d.offset),
        }
    }
}

/// Which step the straight-grid time `u` (in steps) falls in when the off-steps are delayed by
/// `delay` steps. Returns the step's global index, its start and its length, in steps.
///
/// Steps start at `2k` (even) and `2k + 1 + delay` (odd), so an even step lasts `1 + delay` and an
/// odd one `1 - delay`.
fn step_at(u: f64, delay: f64) -> (i64, f64, f64) {
    let pair = (u / 2.0).floor();
    if u - 2.0 * pair < 1.0 + delay {
        (2 * pair as i64, 2.0 * pair, 1.0 + delay)
    } else {
        (2 * pair as i64 + 1, 2.0 * pair + 1.0 + delay, 1.0 - delay)
    }
}

/// A step sequencer, output bipolar (−1…+1).
pub struct Step {
    sr: f64,
    settings: StepSettings,
    clock: Clock,
    amount: Glide,
    offset: Glide,
    step: usize,
}

impl Step {
    pub fn new(sample_rate: f32, settings: StepSettings) -> Self {
        let s = settings.sanitized();
        Step {
            sr: rate_of(sample_rate),
            settings: s,
            clock: Clock::default(),
            amount: Glide(s.amount),
            offset: Glide(s.offset),
            step: 0,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sr = rate_of(sample_rate);
    }

    pub fn settings(&self) -> StepSettings {
        self.settings
    }

    pub fn set_settings(&mut self, settings: StepSettings) {
        self.settings = settings.sanitized();
    }

    /// Restarts the sequence at step 0 (while the host is free-running; a beat-locked sequence
    /// snaps to the host position on the next frame anyway).
    pub fn reset(&mut self) {
        self.clock = Clock::default();
    }

    /// The step being played at the last frame (index into `values`), for display.
    pub fn current_step(&self) -> usize {
        self.step
    }

    /// Writes `out.len()` frames of the sequence for the host timing `t`.
    pub fn process(&mut self, t: &Transport, out: &mut [f32]) {
        let s = self.settings;
        let sr = self.sr;
        let c_glide = one_pole(SETTING_GLIDE_MS, sr);
        let delay = s.swing as f64 / 3.0;
        let len = s.length as i64;
        // Bipolar level of step `k` (the pattern wraps every `length` steps).
        let level = |k: i64| 2.0 * s.values[k.rem_euclid(len) as usize] - 1.0;
        for (i, y) in out.iter_mut().enumerate() {
            let u = self.clock.at(s.rate, t, i, sr);
            let (n, start, dur) = step_at(u, delay);
            let cur = level(n);
            let v = if s.glide > 0.0 {
                let prev = level(n - 1);
                let p = ((u - start) / (s.glide as f64 * dur)).min(1.0) as f32;
                prev + (cur - prev) * p
            } else {
                cur
            };
            self.step = n.rem_euclid(len) as usize;
            let depth = self.amount.next(s.amount, c_glide);
            let offset = self.offset.next(s.offset, c_glide);
            *y = (offset + depth * v).clamp(-1.0, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;
    /// 120 BPM at 48 kHz: 24 000 frames per beat, 6 000 per 1/16.
    const BEAT: usize = 24_000;
    const SIXTEENTH: usize = 6_000;

    fn playing(beat: f64) -> Transport {
        Transport { tempo: 120.0, beat: Some(beat) }
    }

    fn assert_near(a: f32, b: f32, tol: f32) {
        assert!((a - b).abs() <= tol, "{a} != {b} (tolerance {tol})");
    }

    fn lfo_out(s: LfoSettings, t: &Transport, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0; frames];
        Lfo::new(SR, s).process(t, &mut out);
        out
    }

    fn lfo_hz(hz: f32) -> LfoSettings {
        LfoSettings { rate: Rate::Hz(hz), ..Default::default() }
    }

    /// Runs `total` frames in calls of the given sizes (cycled). The beat at each call's start
    /// advances with the frames when `beat0` is given.
    fn in_chunks(
        sizes: &[usize],
        total: usize,
        beat0: Option<f64>,
        mut run: impl FnMut(&mut [f32], &Transport),
    ) -> Vec<f32> {
        let mut out = vec![0.0; total];
        let (mut start, mut k) = (0, 0);
        while start < total {
            let n = sizes[k % sizes.len()].min(total - start);
            let t = Transport {
                tempo: 120.0,
                beat: beat0.map(|b| b + start as f64 * 120.0 / 60.0 / SR as f64),
            };
            run(&mut out[start..start + n], &t);
            start += n;
            k += 1;
        }
        out
    }

    /// Frames at which the step index changes (one-frame calls), with the new index.
    fn step_changes(s: StepSettings, frames: usize) -> Vec<(usize, usize)> {
        let mut st = Step::new(SR, s);
        let mut last = st.current_step();
        let mut changes = Vec::new();
        let mut buf = [0.0];
        for i in 0..frames {
            st.process(&playing(i as f64 / BEAT as f64), &mut buf);
            if st.current_step() != last {
                last = st.current_step();
                changes.push((i, last));
            }
        }
        changes
    }

    // ---- LFO: timing

    #[test]
    fn lfo_hz_runs_free_at_the_rate() {
        let y = lfo_out(lfo_hz(1.0), &Transport::STOPPED, 2 * 48_000);
        assert_near(y[0], 0.0, 1e-6);
        assert_near(y[12_000], 1.0, 1e-4); // ¼ s
        assert_near(y[24_000], 0.0, 1e-4); // ½ s
        assert_near(y[36_000], -1.0, 1e-4); // ¾ s
        assert_near(y[48_000], 0.0, 1e-4); // one full cycle again
    }

    #[test]
    fn lfo_synced_locks_to_the_beat() {
        // One cycle per beat: a quarter of the cycle after 6 000 frames.
        let s = LfoSettings { rate: Rate::Beats(1.0), ..Default::default() };
        let y = lfo_out(s, &playing(0.0), SIXTEENTH + 1);
        assert_near(y[SIXTEENTH], 1.0, 1e-4);
        // Two beats in is the same phase as zero: a cycle is one beat.
        assert_near(lfo_out(s, &playing(2.0), 1)[0], 0.0, 1e-4);
        // A bar (4 beats) starting at beat 1 is a quarter of the way round.
        let bar = LfoSettings { rate: Rate::Beats(4.0), ..Default::default() };
        assert_near(lfo_out(bar, &playing(1.0), 1)[0], 1.0, 1e-4);
    }

    #[test]
    fn lfo_stopped_transport_runs_free_from_where_it_was() {
        let s = LfoSettings { rate: Rate::Beats(1.0), shape: 1.0 / 3.0, ..Default::default() };
        let mut l = Lfo::new(SR, s);
        let mut a = vec![0.0; 1000];
        let mut b = vec![0.0; 1000];
        l.process(&playing(0.0), &mut a);
        l.process(&Transport { tempo: 120.0, beat: None }, &mut b);
        let reference = lfo_out(s, &playing(0.0), 2000);
        for (x, r) in a.iter().chain(b.iter()).zip(&reference) {
            assert_near(*x, *r, 1e-5);
        }
    }

    #[test]
    fn lfo_non_finite_or_zero_transport_counts_as_stopped() {
        let s = LfoSettings { rate: Rate::Beats(1.0), ..Default::default() };
        let stopped = lfo_out(s, &Transport::STOPPED, 500);
        let nan_beat = lfo_out(s, &Transport { tempo: 120.0, beat: Some(f64::NAN) }, 500);
        let nan_tempo = lfo_out(s, &Transport { tempo: f32::NAN, beat: None }, 500);
        let zero_tempo = lfo_out(s, &Transport { tempo: 0.0, beat: None }, 500);
        assert_eq!(stopped, nan_beat);
        assert_eq!(stopped, nan_tempo);
        assert_eq!(stopped, zero_tempo);
    }

    #[test]
    fn lfo_chunking_does_not_change_the_output() {
        let s = LfoSettings { rate: Rate::Hz(3.7), shape: 0.6, smooth_ms: 5.0, ..Default::default() };
        let whole = lfo_out(s, &Transport::STOPPED, 5000);
        let mut l = Lfo::new(SR, s);
        // Free-running output must not depend on how the frames are split into calls.
        let chunked = in_chunks(&[1, 7, 32, 333], 5000, None, |buf, t| l.process(t, buf));
        assert_eq!(whole, chunked);

        // Synced: the beat at each call's start is computed, so allow float rounding.
        let beat = LfoSettings { rate: Rate::Beats(0.5), shape: 0.8, ..Default::default() };
        let whole = lfo_out(beat, &playing(3.25), 5000);
        let mut l = Lfo::new(SR, beat);
        let chunked = in_chunks(&[1, 7, 32, 333], 5000, Some(3.25), |buf, t| l.process(t, buf));
        for (w, c) in whole.iter().zip(&chunked) {
            assert_near(*w, *c, 1e-4);
        }
    }

    #[test]
    fn lfo_empty_and_single_frame_calls_are_fine() {
        let mut l = Lfo::new(SR, LfoSettings::default());
        l.process(&Transport::STOPPED, &mut []);
        let mut one = [0.0];
        l.process(&Transport::STOPPED, &mut one);
        assert!(one[0].is_finite());
    }

    // ---- LFO: shape, amount, phase, smoothing, fade

    #[test]
    fn lfo_shape_morphs_between_the_four_waves() {
        for p in (0..100).map(|i| i as f32 / 100.0) {
            assert_near(morph(0.0, p), wave(0, p), 1e-6); // sine
            assert_near(morph(1.0 / 3.0, p), wave(1, p), 1e-6); // triangle
            assert_near(morph(2.0 / 3.0, p), wave(2, p), 1e-6); // saw
            assert_near(morph(1.0, p), wave(3, p), 1e-6); // square
            // Halfway between triangle and saw.
            assert_near(morph(0.5, p), 0.5 * (wave(1, p) + wave(2, p)), 1e-6);
        }
        // Triangle corners and the saw ramp.
        assert_near(wave(1, 0.25), 1.0, 1e-6);
        assert_near(wave(1, 0.75), -1.0, 1e-6);
        assert_near(wave(2, 0.0), -1.0, 1e-6);
        assert_near(wave(2, 0.5), 0.0, 1e-6);
    }

    #[test]
    fn lfo_waves_stay_in_range() {
        for k in 0..4 {
            for i in 0..1000 {
                for shape in [0.0, 0.3, 0.7, 1.0] {
                    let v = morph(shape, i as f32 / 1000.0);
                    assert!((-1.0..=1.0).contains(&v), "shape {shape} p {i}: {v}");
                }
                let v = wave(k, i as f32 / 1000.0);
                assert!((-1.0..=1.0).contains(&v));
            }
        }
    }

    #[test]
    fn lfo_amount_scales_and_flips() {
        let half = LfoSettings { amount: 0.5, ..lfo_hz(1.0) };
        assert_near(lfo_out(half, &Transport::STOPPED, 12_001)[12_000], 0.5, 1e-4);
        let flipped = LfoSettings { amount: -1.0, ..lfo_hz(1.0) };
        assert_near(lfo_out(flipped, &Transport::STOPPED, 12_001)[12_000], -1.0, 1e-4);
    }

    #[test]
    fn lfo_phase_offset_shifts_the_wave() {
        // 90° on a sine: starts at its peak.
        let s = LfoSettings { phase: 90.0, ..lfo_hz(1.0) };
        assert_near(lfo_out(s, &Transport::STOPPED, 1)[0], 1.0, 1e-6);
        // 450° is the same as 90°.
        let s = LfoSettings { phase: 450.0, ..lfo_hz(1.0) };
        assert_near(lfo_out(s, &Transport::STOPPED, 1)[0], 1.0, 1e-6);
    }

    #[test]
    fn lfo_smooth_is_a_one_pole_lag() {
        // A square with 100 ms smoothing: rises like 1 − e^(−t/100 ms).
        let s = LfoSettings { rate: Rate::Hz(1.0), shape: 1.0, smooth_ms: 100.0, ..Default::default() };
        let y = lfo_out(s, &Transport::STOPPED, 24_000);
        let c = one_pole(100.0, SR as f64);
        let mut r = 0.0;
        for v in &y {
            r += c * (1.0 - r); // the square is +1 for the first half cycle
            assert_near(*v, r, 1e-4);
        }
        assert_near(y[4799], 1.0 - (-1.0f32).exp(), 0.002);
    }

    #[test]
    fn lfo_fade_in_ramps_after_reset() {
        let s = LfoSettings { fade_in_ms: 100.0, ..lfo_hz(1.0) };
        let mut l = Lfo::new(SR, s);
        l.reset();
        let mut y = vec![0.0; 9600];
        l.process(&Transport::STOPPED, &mut y);
        for (n, v) in y.iter().enumerate() {
            let fade = ((n + 1) as f32 / 4800.0).min(1.0);
            let expect = (std::f32::consts::TAU * n as f32 / SR).sin() * fade;
            assert_near(*v, expect, 1e-4);
        }
    }

    #[test]
    fn lfo_without_fade_starts_at_full_level_after_reset() {
        let s = LfoSettings { shape: 1.0, ..lfo_hz(1.0) };
        let mut l = Lfo::new(SR, s);
        l.reset();
        let mut y = [0.0];
        l.process(&Transport::STOPPED, &mut y);
        assert_near(y[0], 1.0, 1e-6);
    }

    #[test]
    fn lfo_sanitizes_settings() {
        let s = LfoSettings {
            rate: Rate::Hz(1000.0),
            amount: 5.0,
            shape: -1.0,
            phase: 810.0,
            smooth_ms: f32::NAN,
            fade_in_ms: 1e9,
        }
        .sanitized();
        assert_eq!(s.rate, Rate::Hz(RATE_HZ.1));
        assert_eq!(s.amount, 1.0);
        assert_eq!(s.shape, 0.0);
        assert_near(s.phase, 90.0, 1e-4);
        assert_eq!(s.smooth_ms, 0.0);
        assert_eq!(s.fade_in_ms, 10_000.0);
        assert_eq!(Rate::Hz(-3.0).sanitized(), Rate::Hz(RATE_HZ.0));
        assert_eq!(Rate::Beats(0.0).sanitized(), Rate::Beats(RATE_BEATS.0));
        assert_eq!(Rate::Beats(f32::INFINITY).sanitized(), Rate::Beats(1.0));
        let s = LfoSettings { amount: f32::NAN, phase: f32::NAN, ..Default::default() }.sanitized();
        assert_eq!((s.amount, s.phase), (1.0, 0.0));
    }

    // ---- Follow

    fn follow_out(s: FollowSettings, input: &[[f32; 2]]) -> Vec<f32> {
        let mut out = vec![0.0; input.len()];
        Follow::new(SR, s).process(input, &mut out);
        out
    }

    fn instant_attack() -> FollowSettings {
        FollowSettings { attack_ms: 0.0, ..Default::default() }
    }

    #[test]
    fn follow_attack_is_a_time_constant() {
        let input = vec![[1.0, 1.0]; 4800];
        let out = follow_out(FollowSettings { attack_ms: 10.0, ..Default::default() }, &input);
        // 10 ms = 480 frames: 63 % of the way to the input.
        assert_near(out[479], 1.0 - (-1.0f32).exp(), 0.002);
        assert!(out.windows(2).all(|w| w[1] >= w[0]), "a rise never falls");
    }

    #[test]
    fn follow_release_is_a_time_constant() {
        let mut input = vec![[1.0, 1.0]; 2400];
        input.extend(vec![[0.0, 0.0]; 4800]);
        let out = follow_out(
            FollowSettings { attack_ms: 0.0, release_ms: 100.0, ..Default::default() },
            &input,
        );
        assert_near(out[2399], 1.0, 1e-6);
        // 100 ms after the input drops to zero: 37 % of the peak left.
        assert_near(out[7199], (-1.0f32).exp(), 0.002);
    }

    #[test]
    fn follow_hold_keeps_the_peak_before_releasing() {
        let mut input = vec![[0.0, 0.0]; 10_000];
        input[0] = [1.0, 1.0];
        let s = FollowSettings { attack_ms: 0.0, hold_ms: 50.0, release_ms: 100.0, ..Default::default() };
        let out = follow_out(s, &input);
        // 50 ms = 2 400 frames of hold after the peak.
        assert_near(out[0], 1.0, 1e-6);
        assert_near(out[2400], 1.0, 1e-6);
        assert!(out[2401] < 1.0 - 1e-4, "release starts after the hold");
        // Then 100 ms of release from the peak.
        assert_near(out[2400 + 4800], (-1.0f32).exp(), 0.002);
    }

    #[test]
    fn follow_retriggers_hold_on_a_new_peak() {
        let mut input = vec![[0.0, 0.0]; 5000];
        input[0] = [1.0, 1.0];
        input[1000] = [1.0, 1.0];
        let s = FollowSettings { attack_ms: 0.0, hold_ms: 50.0, release_ms: 100.0, ..Default::default() };
        let out = follow_out(s, &input);
        // The second peak restarts the hold: still at the peak 50 ms after it.
        assert_near(out[1000 + 2400], 1.0, 1e-6);
    }

    #[test]
    fn follow_uses_the_louder_channel_as_absolute_peak() {
        let out = follow_out(instant_attack(), &[[0.0, -0.8]; 4]);
        assert_near(out[3], 0.8, 1e-6);
    }

    #[test]
    fn follow_gain_and_floor() {
        let s = FollowSettings { gain_db: 20.0, floor: 0.1, attack_ms: 0.0, release_ms: 10.0, ..Default::default() };
        let mut f = Follow::new(SR, s);
        let mut out = vec![0.0; 24_000];
        f.process(&vec![[0.05, 0.05]; 12_000], &mut out[..12_000]);
        f.process(&vec![[0.0, 0.0]; 12_000], &mut out[12_000..]);
        // Gain ×10 on the envelope, on top of the floor.
        assert_near(out[11_999], 0.6, 1e-3);
        // Silent for a while: back to the floor.
        assert_near(out[23_999], 0.1, 1e-3);
    }

    #[test]
    fn follow_output_is_clamped_to_unit_range() {
        let s = FollowSettings { gain_db: 24.0, floor: 0.9, ..instant_attack() };
        let out = follow_out(s, &[[1.0, 1.0]; 4800]);
        assert!(out.iter().all(|v| (0.0..=1.0).contains(v)));
        assert_near(out[4799], 1.0, 1e-4);
    }

    #[test]
    fn follow_smooth_lags_the_output() {
        let s = FollowSettings { smooth_ms: 100.0, ..instant_attack() };
        let out = follow_out(s, &[[1.0, 1.0]; 4800]);
        assert_near(out[4799], 1.0 - (-1.0f32).exp(), 0.002);
    }

    #[test]
    fn follow_ignores_non_finite_input() {
        let mut input = vec![[f32::NAN, 0.3], [f32::INFINITY, 0.0], [f32::NEG_INFINITY, f32::NAN]];
        input.extend(vec![[0.5, 0.5]; 100]);
        let out = follow_out(instant_attack(), &input);
        assert!(out.iter().all(|v| v.is_finite()));
        assert_near(out[3 + 99], 0.5, 1e-6);
    }

    #[test]
    fn follow_chunking_does_not_change_the_output() {
        let input: Vec<[f32; 2]> = (0..3000).map(|i| [((i as f32) * 0.05).sin(), 0.3]).collect();
        let s = FollowSettings { hold_ms: 3.0, smooth_ms: 2.0, ..Default::default() };
        let whole = follow_out(s, &input);
        let mut f = Follow::new(SR, s);
        let mut chunked = Vec::new();
        let mut start = 0;
        for n in [1usize, 7, 32, 333].iter().cycle() {
            if start >= input.len() {
                break;
            }
            let end = (start + n).min(input.len());
            let mut buf = vec![0.0; end - start];
            f.process(&input[start..end], &mut buf);
            chunked.extend(buf);
            start = end;
        }
        assert_eq!(whole, chunked);
    }

    #[test]
    fn follow_reset_drops_the_envelope() {
        let s = FollowSettings { release_ms: 5000.0, ..instant_attack() };
        let mut f = Follow::new(SR, s);
        let mut buf = vec![0.0; 1000];
        f.process(&vec![[1.0, 1.0]; 1000], &mut buf);
        f.reset();
        f.process(&vec![[0.0, 0.0]; 1], &mut buf[..1]);
        assert_near(buf[0], 0.0, 1e-6);
    }

    #[test]
    fn follow_sanitizes_settings() {
        let s = FollowSettings {
            attack_ms: -5.0,
            hold_ms: 1e9,
            release_ms: f32::NAN,
            gain_db: 100.0,
            floor: 3.0,
            smooth_ms: -1.0,
        }
        .sanitized();
        assert_eq!(s.attack_ms, 0.0);
        assert_eq!(s.hold_ms, 1000.0);
        assert_eq!(s.release_ms, FollowSettings::default().release_ms);
        assert_eq!(s.gain_db, 24.0);
        assert_eq!(s.floor, 1.0);
        assert_eq!(s.smooth_ms, 0.0);
    }

    // ---- Step

    fn step_settings(values: &[f32], rate: Rate) -> StepSettings {
        let mut v = [0.5; STEPS];
        v[..values.len()].copy_from_slice(values);
        StepSettings { values: v, length: values.len(), rate, ..Default::default() }
    }

    #[test]
    fn step_plays_values_and_wraps_at_length() {
        // 10 Hz: a step every 4 800 frames. Checked mid-step, away from the boundary frames.
        let s = step_settings(&[0.0, 0.25, 0.5, 1.0], Rate::Hz(10.0));
        let mut st = Step::new(SR, s);
        let mut y = vec![0.0; 5 * 4800];
        st.process(&Transport::STOPPED, &mut y);
        assert_eq!(y[2400], -1.0);
        assert_eq!(y[4800 + 2400], -0.5);
        assert_eq!(y[9600 + 2400], 0.0);
        assert_eq!(y[14400 + 2400], 1.0);
        assert_eq!(y[19200 + 2400], -1.0); // wrapped to step 0
        assert_eq!(st.current_step(), 0);
    }

    #[test]
    fn step_changes_only_on_step_boundaries() {
        let s = step_settings(&[0.0, 1.0], Rate::Hz(10.0));
        let changes = step_changes(s, 20_000);
        // Frame 4 800 steps to index 1 (checked away from the exact boundary frame).
        let frames: Vec<usize> = changes.iter().map(|c| c.0).collect();
        assert_eq!(frames.len(), 4);
        for (f, want) in frames.iter().zip([4800, 9600, 14400, 19200]) {
            assert!((*f as i64 - want).abs() <= 2, "change at {f}, expected {want}");
        }
    }

    #[test]
    fn step_length_limits_the_pattern() {
        let mut s = step_settings(&[0.0, 0.25, 0.5, 1.0], Rate::Hz(10.0));
        s.length = 2;
        let changes = step_changes(s, 12_000);
        // Two steps: 0 → 1 at 4 800, back to 0 at 9 600.
        assert!(changes.iter().all(|&(_, idx)| idx < 2));
        assert_eq!(changes.iter().map(|c| c.1).collect::<Vec<_>>(), [1, 0]);
    }

    #[test]
    fn step_amount_offset_and_clamp() {
        let base = step_settings(&[0.5], Rate::Hz(1.0));
        // Level 0.5 is 0: output is the offset.
        let mut st = Step::new(SR, StepSettings { offset: -0.2, ..base });
        let mut y = [0.0];
        st.process(&Transport::STOPPED, &mut y);
        assert_near(y[0], -0.2, 1e-6);
        // Level 1 with amount 0.5 and offset −0.2: 0.3.
        let s = StepSettings { values: [1.0; STEPS], amount: 0.5, offset: -0.2, ..base };
        Step::new(SR, s).process(&Transport::STOPPED, &mut y);
        assert_near(y[0], 0.3, 1e-6);
        // Negative amount inverts; the sum is clamped to −1…+1.
        let s = StepSettings { values: [0.0; STEPS], amount: -1.0, offset: 0.8, ..base };
        Step::new(SR, s).process(&Transport::STOPPED, &mut y);
        assert_near(y[0], 1.0, 1e-6);
        let s = StepSettings { values: [1.0; STEPS], amount: 1.0, offset: 0.8, ..base };
        Step::new(SR, s).process(&Transport::STOPPED, &mut y);
        assert_eq!(y[0], 1.0);
    }

    #[test]
    fn step_glide_slides_into_each_step() {
        // Two steps, one per 1/16: step 0 at −1, step 1 at +1, full-step glide.
        let s = StepSettings { glide: 1.0, ..step_settings(&[0.0, 1.0], Rate::Beats(0.25)) };
        let mut st = Step::new(SR, s);
        let mut y = vec![0.0; 2 * SIXTEENTH];
        st.process(&playing(0.0), &mut y);
        // Step 0 glides from the previous step (+1) down to −1: halfway at its middle.
        assert_near(y[SIXTEENTH / 2], 0.0, 1e-3);
        assert!(y[SIXTEENTH - 1] < -0.999);
        // Step 1 glides up from −1 to +1.
        assert_near(y[SIXTEENTH + SIXTEENTH / 2], 0.0, 1e-3);
        assert!(y[SIXTEENTH] < -0.999);
    }

    #[test]
    fn step_without_glide_jumps() {
        let s = step_settings(&[0.0, 1.0], Rate::Beats(0.25));
        let mut y = vec![0.0; 2 * SIXTEENTH];
        Step::new(SR, s).process(&playing(0.0), &mut y);
        // A frame either side of the boundary (the beat arithmetic rounds on the boundary itself).
        assert_eq!(y[SIXTEENTH - 2], -1.0);
        assert_eq!(y[SIXTEENTH + 2], 1.0);
    }

    #[test]
    fn step_swing_delays_the_off_steps() {
        let s = StepSettings { length: 16, ..step_settings(&[0.0], Rate::Beats(0.25)) };
        let changes = step_changes(s, 24_100);
        let frames: Vec<usize> = changes.iter().map(|c| c.0).collect();
        assert_eq!(frames.len(), 4);
        for (f, want) in frames.iter().zip([6000, 12000, 18000, 24000]) {
            assert!((*f as i64 - want).abs() <= 2, "straight: change at {f}, expected {want}");
        }

        // Full swing: the odd steps start a third of a step late (8 000, 20 000 …).
        let swung = StepSettings { swing: 1.0, ..s };
        let changes = step_changes(swung, 24_100);
        let got: Vec<(i64, usize)> = changes.iter().map(|&(f, idx)| (f as i64, idx)).collect();
        let want = [(8000, 1), (12000, 2), (20000, 3), (24000, 4)];
        assert_eq!(got.len(), want.len(), "{got:?}");
        for ((f, idx), (wf, widx)) in got.iter().zip(want) {
            assert!((f - wf).abs() <= 2, "swing: {got:?}");
            assert_eq!(*idx, widx);
        }
    }

    #[test]
    fn step_swing_keeps_the_steps_contiguous() {
        // Half swing over a few steps: every frame belongs to exactly one step, with no gap or overlap.
        for u in (0..200).map(|i| i as f64 * 0.0137) {
            let (n, start, dur) = step_at(u, 0.5 / 3.0);
            assert!(start <= u && u < start + dur + 1e-12, "u {u}: step {n} [{start}, {})", start + dur);
            let (n2, start2, _) = step_at(start + dur + 1e-9, 0.5 / 3.0);
            assert_eq!(n2, n + 1);
            assert!((start2 - (start + dur)).abs() < 1e-9);
        }
    }

    #[test]
    fn step_synced_follows_the_bar_grid() {
        let s = StepSettings { length: 4, rate: Rate::Beats(1.0), ..Default::default() };
        let mut st = Step::new(SR, s);
        let mut one = [0.0];
        st.process(&playing(1.0), &mut one);
        assert_eq!(st.current_step(), 1);
        st.process(&playing(5.0), &mut one);
        assert_eq!(st.current_step(), 1); // beat 5 is step 5 of 4 → 1
        st.process(&playing(7.0), &mut one);
        assert_eq!(st.current_step(), 3);
    }

    #[test]
    fn step_stopped_transport_continues_from_the_beat() {
        let values = [0.0, 1.0, 0.25, 0.75];
        let s = StepSettings { glide: 0.5, ..step_settings(&values, Rate::Beats(0.25)) };
        let mut st = Step::new(SR, s);
        let mut a = vec![0.0; 3000];
        let mut b = vec![0.0; 9000];
        st.process(&playing(0.0), &mut a);
        st.process(&Transport { tempo: 120.0, beat: None }, &mut b);
        let mut reference = vec![0.0; 12_000];
        Step::new(SR, s).process(&playing(0.0), &mut reference);
        for (x, r) in a.iter().chain(b.iter()).zip(&reference) {
            assert_near(*x, *r, 1e-5);
        }
    }

    #[test]
    fn step_chunking_does_not_change_the_output() {
        let s = StepSettings { swing: 0.5, glide: 0.6, ..step_settings(&[0.0, 1.0, 0.3, 0.9], Rate::Beats(0.25)) };
        let whole = {
            let mut y = vec![0.0; 40_000];
            Step::new(SR, s).process(&playing(0.5), &mut y);
            y
        };
        let mut st = Step::new(SR, s);
        let chunked = in_chunks(&[1, 7, 32, 333], 40_000, Some(0.5), |buf, t| st.process(t, buf));
        for (w, c) in whole.iter().zip(&chunked) {
            assert_near(*w, *c, 1e-4);
        }
    }

    #[test]
    fn step_reset_restarts_the_free_sequence() {
        let s = step_settings(&[0.0, 1.0, 0.5], Rate::Hz(10.0));
        let mut st = Step::new(SR, s);
        let mut y = vec![0.0; 10_000];
        st.process(&Transport::STOPPED, &mut y);
        assert_eq!(st.current_step(), 2);
        st.reset();
        let mut one = [0.0];
        st.process(&Transport::STOPPED, &mut one);
        assert_eq!(st.current_step(), 0);
    }

    #[test]
    fn step_sanitizes_settings() {
        let mut values = [0.25; STEPS];
        values[0] = f32::NAN;
        values[1] = 2.0;
        values[2] = -1.0;
        let s = StepSettings {
            values,
            length: 40,
            rate: Rate::Hz(f32::NAN),
            glide: 3.0,
            swing: -1.0,
            amount: -3.0,
            offset: f32::INFINITY,
        }
        .sanitized();
        assert_eq!(s.values[0], 0.5);
        assert_eq!(s.values[1], 1.0);
        assert_eq!(s.values[2], 0.0);
        assert_eq!(s.length, STEPS);
        assert_eq!(s.rate, Rate::Hz(1.0));
        assert_eq!((s.glide, s.swing, s.amount, s.offset), (1.0, 0.0, -1.0, 0.0));
        assert_eq!(StepSettings { length: 0, ..Default::default() }.sanitized().length, 1);
    }

    #[test]
    fn step_non_finite_transport_counts_as_stopped() {
        let s = step_settings(&[0.0, 1.0, 0.25], Rate::Beats(0.25));
        let mut a = vec![0.0; 500];
        let mut b = vec![0.0; 500];
        Step::new(SR, s).process(&Transport::STOPPED, &mut a);
        Step::new(SR, s).process(&Transport { tempo: f32::NAN, beat: Some(f64::NAN) }, &mut b);
        assert_eq!(a, b);
    }

    #[test]
    fn bad_sample_rate_and_empty_calls_are_safe() {
        let mut l = Lfo::new(f32::NAN, LfoSettings::default());
        l.process(&Transport::STOPPED, &mut []);
        let mut out = [0.0; 4];
        l.process(&Transport::STOPPED, &mut out);
        assert!(out.iter().all(|v| v.is_finite()));
        let mut st = Step::new(0.0, StepSettings::default());
        st.process(&Transport::STOPPED, &mut out);
        assert!(out.iter().all(|v| v.is_finite()));
    }
}
