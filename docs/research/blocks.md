# Blocks: what they are for, what we have, what's missing

_2026-11-21. Started from Flemming's questions: do our compressors have lookahead, do we have
linear- and minimum-phase filters, a reverb and a delay, could a vari-mu compressor be built?
Answers below, from the code at that date; §4 items marked **built** were done the same day.
Related: [`../STATUS.md`](../STATUS.md) (glossary,
decisions), the block reference with every option in `patches/.types/scriptfx.d.luau`._

## 1. What a block is

A **block** is a *building block*: the unit patches build professional effect chains from. The
name is deliberate (Flemming, 2026-11-21). A patch is not a choice between fixed effects but a
combination of blocks, so blocks are designed like modules in a modular system, not like finished
plugins:

- **Expose every parameter we can.** If a real device or a mixing engineer would want to set it
  (detector type, knee, stereo link, sidechain filter, oversampling, topology), it is a knob or an
  option on the block. A versatile block beats three fixed variants. Hidden constants are a
  smell: they limit what a patch (or the LLM) can build.
- **Parameters are Params.** Anything continuous is a `Param`: it gets a knob on the block page,
  can be driven by macros and modulators, and is saved with the project. Structural choices
  (mode strings, sub-chains, lists) are options fixed at load.
- **Styles are presets, not separate DSP**, where possible. `FETCompressor` and
  `OptoCompressor` today are separate block types with different detectors; the goal is one
  `Compressor` whose detector, topology and release behaviour are parameters, with the styles as
  defaults (§4).
- **Good defaults.** A block with no options sounds right (musical, not neutral-but-useless).
- **Composable.** Blocks take sub-chains where processing belongs inside them (`Delay`'s
  `feedback_chain` runs on every repeat; `BandSplit`, `MidSide`, `Parallel` route chains). The
  same idea gives compressors a sidechain chain (§4).
- **Honest latency.** Blocks that look ahead or oversample report it; the host compensates.

## 2. The questions

| Question | Today | Notes |
|---|---|---|
| Compressor lookahead? | **No** at the time; only `Limiter` had it. **Built since:** `lookahead` (0–20 ms, fixed: it is latency) on every compressor, `Gate`, `DeEsser`, `Transient` | |
| Linear- and minimum-phase filters? | **Minimum-phase only** at the time (**built since:** `phase = "linear"`, see §4). All EQs and filters are IIR biquads (RBJ); `BandSplit` uses Linkwitz-Riley crossovers (phase-aligned between bands, but not linear-phase). `LowPass`/`HighPass` now have a slope (12–48 dB/oct) | Linear phase needs FIR or FFT convolution and costs latency (~ tens of ms at low frequencies). Worth it for mastering EQ and multiband splits: a `phase: "minimum" \| "linear"` option on EQ blocks and `BandSplit` |
| Reverb? Delay? | **Yes.** `Reverb` (8-line FDN: size, decay, damping, predelay, width, freeze), `Shimmer`, `Delay` (time or sync, feedback, ping-pong, `feedback_chain`) | Missing: convolution (impulse responses), algorithm variants (plate, room, hall, spring), reverb modulation and early-reflection control, delay ducking, multi-tap, reverse |
| Could users model a vari-mu? | **Only roughly** at the time (**built since:** `VariMuCompressor`, a preset of the versatile compressor). `Compressor` with a wide knee (24 dB) gives a ratio that rises with level, `OptoCompressor` has program-dependent release, `Drive` before/after adds tube-ish saturation | A vari-mu's character: ratio that keeps rising with gain reduction (no fixed ratio), feedback topology, slow program-dependent attack *and* release, tube saturation and its asymmetry. Needs the compressor parameters in §4; then a vari-mu is a preset |

## 3. The catalogue (2026-11-21, 51 blocks)

| Category | Blocks |
|---|---|
| Utility | Gain, Pan, Width, MonoBass, Haas, Invert (polarity), DCBlock, Meter |
| EQ | PeakEq, LowShelf, HighShelf |
| Filter | LowPass, HighPass (slope 12–48), MorphFilter (SVF LP/BP/HP), LadderFilter, CombFilter, Formant, Resonator, DynamicEq |
| Distortion | Drive, Waveshaper (custom curve), Wavefolder, Clipper, Rectifier, Bitcrusher, RingMod, Tape |
| Dynamics | Compressor, FETCompressor, OptoCompressor, VariMuCompressor (presets of one compressor), Rider, Transient, Limiter, Gate, DeEsser |
| Modulation | Chorus, Flanger, Phaser, Tremolo, AutoPan |
| Space | Delay, Reverb, Shimmer |
| Pitch | PitchShift, Granular |
| Rhythm | Stutter |
| Routing | MidSide, LeftRight, BandSplit, Parallel |

Every block also takes `oversampling` (1, 2, 4, 8). Plus, outside the chain: macros and the
modulators (Follow, LFO 1–3, Step). `patches/kitchen-sink` uses every block, every modulator.

## 4. What's missing for professional chains

Ordered by how much it opens up. P1 = needed for "professional mixing/mastering chains" to be
true; P2 = expected by producers; P3 = nice to have.

### Dynamics

- **P1 · Sidechain.** Two parts: an *internal* sidechain chain on every dynamics block
  (`detector_chain = Chain.new { HighPass(...) }`, like `Delay`'s `feedback_chain`: the key
  signal is processed before detection; covers SC high-pass, de-essing by band, tilt) and an
  *external* key input (a second stereo input bus in VST3/CLAP, `AUDIO_IO_LAYOUTS` aux input;
  `key = "external"`), for ducking and pumping.
- **P1 · One versatile compressor. Built 2026-11-21** (`fx/dynamics.rs`): `detector` (peak ↔
  RMS, `window`), `feedback` (feed-forward ↔ feedback, the dialled ratio kept), `ratio_curve`
  (local ratio rising from 1:1 to `ratio` over 30 dB), `program` (program-dependent release),
  `link`, `saturation` (asymmetric, even harmonics), `lookahead`, plus threshold, ratio (1–100,
  log), attack, release, knee (to 30), makeup/auto, mix. `Compressor`, `FETCompressor`,
  `OptoCompressor`, `VariMuCompressor` are presets. Not yet: M/S mode, upward/∞ modes, a
  release *curve* (only its amount is program-dependent), attack program dependence.
  The original plan: parameters instead of three types: `detector`
  (peak / RMS + window), `topology` (feed-forward / feedback), `knee` up to ~30 dB, `ratio`
  including ∞ and a **ratio curve** (fixed vs. rising with gain reduction: vari-mu), `attack` /
  `release` plus **program-dependent** amount (opto/vari-mu behaviour), `lookahead`, `link`
  (stereo link %, plus M/S mode), `saturation` (the gain stage's colour), `mix`, `makeup` /
  auto. `FETCompressor`, `OptoCompressor` and a vari-mu become presets of it (keep the old names
  as aliases).
- **P1 · Lookahead** on compressors, gate, de-esser, transient (latency reported). **Built
  2026-11-21.**
- **P2 · True-peak limiting** (inter-sample peaks, oversampled detection) on `Limiter`; loudness
  metering (LUFS short-term/integrated, correlation) as meter blocks for the analysis and the UI.
- **P2 · Upward compression / expansion** (bring quiet parts up): a mode or a negative-ratio
  range on the compressor.
- **P3 · Envelope-follower block output** (a block's detector as a modulation source); partly
  covered by the Follow modulator.

### EQ and filters

- **P1 · Linear-phase option** on EQ blocks and `BandSplit` (FFT/FIR, latency reported). **Built
  2026-11-21** (`dsp/src/linear.rs`): `phase = "linear"` on PeakEq, LowShelf, HighShelf, LowPass,
  HighPass, BandSplit. A zero-phase FIR from the minimum-phase block's magnitude (8192 taps at
  48 kHz, Hann), uniformly partitioned FFT convolution (32 partitions); latency ~91 ms at 48 kHz;
  knob moves redesign the FIR (≤ once per 256-sample block, crossfaded), allocation-free. Band
  split bands are differences of LR4 magnitudes, so they reconstruct exactly. Open: a quality
  setting (shorter FIR = less latency), analog-matched (unwarped) magnitudes near Nyquist.
- **P2 · More shapes:** band-pass, notch, all-pass (phase rotation; exists internally as a biquad
  shape), tilt EQ, Baxandall shelves; shelf slope; proportional-Q / analog-style bells; passive
  (Pultec-style) boost/cut interaction.
- **P2 · A multi-band parametric EQ block** (n bands in one block, each with type, freq, gain,
  Q, slope): one block page instead of a chain of five, and the display a patch UI expects.
- **P3 · Dynamic EQ with several bands** and a sidechain; match EQ (needs analysis data).

### Saturation and colour

- **P2 · Tube, transformer, console** models with their own harmonic signatures (even/odd balance,
  frequency-dependent saturation, bias); an **exciter** (harmonics of the highs only). Today
  `Drive`, `Waveshaper` (any curve), `Wavefolder` bias and `Tape` get some of the way.

### Space and time

- **P2 · Convolution** (impulse responses from the patch folder: rooms, cabinets, hardware);
  needs partitioned FFT convolution.
- **P2 · Reverb algorithms** (plate, room, hall, spring) and controls (early reflections, diffusion,
  modulation, low/high decay multipliers, ducking).
- **P2 · Delay:** ducking (by its own input), multi-tap, reverse, tape/analog character (partly
  via `feedback_chain` with `Tape`/filters already), diffusion.
- **P3 · Frequency shifter, vibrato, rotary speaker, ensemble.**

### Pitch and restoration

- **P3 · Formant-preserving pitch shift, harmonizer;** pitch detection (for tuning/correction).
- **P3 · De-noise, de-click, de-hum** (restoration is a different product; only if patches need it).

### Utility and routing

- **P2 · Sample delay** (time alignment), **phase rotation**, auto-gain / gain matching (loudness
  in = out), per-band solo in `BandSplit` (for building, not for patches).
- **P3 · Feedback routing** between chains; crossfades with more than two branches. (Now the
  `Feedback` container in §5.)

## 5. The elementary layer

_Added 2026-11-21 (Flemming asked whether we need more technical, basic blocks)._ §4 is mostly big
"pro" blocks. Just as important is the layer below: small, generic pieces that let a patch (or the
LLM) build what we never made a block for. A modular system is only as good as its primitives;
each of these multiplies what the existing blocks can do.

### Primitives

- **DelayLine:** a short delay (0–50 ms), interpolated, no feedback, no mix. With a modulator on
  `time` it is vibrato, chorus, flanger or doppler; fixed, it is time alignment (also covers the
  "sample delay" of §4). `Delay` is an echo; `Chorus`/`Flanger` are sealed. The interpolated
  delay line exists (`fx::Line`).
- **AllPass / PhaseRotate:** shifts phase, not level (frequency, order). Phase alignment between
  tracks or bands, taming asymmetric peaks before a limiter, a building block for phasers and
  diffusion. The biquad shape exists internally (`EqShape::AllPass`).
- **Noise:** white or pink at a level, optionally shaped by the input's envelope (gated or
  breathing): tape hiss, vinyl, transient enhancement, dither.
- **Oscillator:** sine (and the LFO shapes) at a frequency, optionally tracking the input's pitch:
  sub-bass reinforcement, test tones, ring-mod carriers that follow the note.
- **Slew:** limits how fast the signal changes (rise / fall per ms): softens edges, a smoother
  rectifier or crusher, a simple one-pole smoother on anything.

### Routing containers

- **Feedback { chain, amount, delay }:** feeds a chain's output back into its input (the delay
  ≥ one block, with an optional limiter in the loop). Today only `Delay` has a feedback loop
  (`feedback_chain`). Opens resonators, Karplus-style plucks, custom reverbs, feedback
  distortion.
- **TransientSplit { attack, sustain }:** splits into transient and sustain parts (the
  `Transient` block's fast/slow detector), runs each through its own chain, sums them (they
  reconstruct exactly with the chains empty). Saturate only attacks, reverb only tails. The
  dynamic counterpart to `BandSplit` / `MidSide`: very strong for mixing.
- **LevelSplit { loud, quiet }:** the same by level (threshold, knee): upward compression,
  saturation that only hits loud passages, a gate's "below" branch as its own chain.
- **Mono / Swap:** sum to mono, swap L/R, take one channel. Small utilities; `LeftRight` with
  gains does it today, clumsily.

### Escape hatch

- **Convolution** (an impulse response from the patch folder; P2 in §4) belongs here too: any
  measured filter, cabinet, room or hardware EQ becomes a block, more than any single DSP block
  covers. The partitioned FFT engine exists (`dsp/src/linear.rs`).
- **Not planned: an audio script / expression block.** Lua must never run on the audio thread; the
  `Waveshaper`'s curve (sampled at load) is the safe version of "custom code".

### Order

First **DelayLine**, **TransientSplit**, **Feedback**: they multiply the most and are each small
on what exists (delay line, transient detector, latency alignment of sub-chains). Then AllPass,
Noise, Slew; Oscillator with pitch tracking needs a pitch detector (shared with §4's pitch items).

## 6. Consequences for the engine

- **Latency** already sums over the chain and goes to the host; lookahead and linear phase add to
  it. A patch's latency should show in the shell (block page and chain view).
- **Sidechain input** changes the plugin's bus layout (aux input): test in Reaper, Studio One,
  Fender Studio before relying on it.
- **FFT** (linear-phase EQ, convolution, LUFS gating) needs an allocation-free FFT on the audio
  thread (plan sizes at load, `realfft`/`rustfft` with preallocated scratch).
- **One compressor with many parameters** means more Params per block: the block page needs
  groups (detector, timing, character, output) and choices (switches, pills), which are on the
  STATUS list for block pages already.
