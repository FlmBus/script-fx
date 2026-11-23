# Block modal: displays, live data, knob groups

_2026-11-23. Worked out with Flemming. Mockups: [`mockups/block-modal.html`](mockups/block-modal.html)
(the modal and its transition), [`mockups/block-displays.html`](mockups/block-displays.html) (one
display per block type). Built: the modal and the transition (`shell/block_modal.rs`,
`shell/motion.rs`); the displays are next ([`../STATUS.md`](../STATUS.md), next steps)._

## 1. The displays

One display per block type, on the left of the block modal (fixed 626 × 266; the display is 190 px
high and as wide as the knob column leaves: 8 knobs → 300 px, 3 → 370, none → 590).

- **White and grey only.** The theme's ink (white in dark mode, near-black in light mode) for what
  the block does or its output; grey for the input, references, a second channel; dashed for
  thresholds, targets, the neutral line. No block or modulator colours.
- **Handles** (EQ frequency/gain, cutoff, crossovers, Waveshaper points) are drawn now and become
  draggable later (dragging = turning the knobs).
- **Accuracy:** displays from `dsp::preview` show what the DSP computes. The others are
  illustrations from the knob values (reverb decay per band, shimmer layers, tape colour); they say
  so in the code, not in the UI.

**Build order**, by the tag each mockup card carries:

1. **`preview.rs`** (done 2026-11-23, `shell/block_display.rs`): restyle what exists (EQs, low/high pass, Drive, Waveshaper, Wavefolder, Clipper,
   compressors, Limiter) to the mockup look: handles on the curves, a test sine next to transfer
   curves, gain reduction over time where the meter allows.
2. **`params`** (done 2026-11-23, `shell/block_display/params.rs`; also Gate, DeEsser, Rider,
   DynamicEq as static curves, the modulation blocks at LFO phase 0, the containers' routing, and
   the Meter block's level history from its existing meter): displays from the knob values alone (filters, Transient, Bitcrusher, RingMod,
   Rectifier, Delay with ping-pong and feedback chain, Reverb, Shimmer, PitchShift, Pan, Haas,
   MonoBass, Invert). No audio-thread work.
3. **`live`**: displays that need data from the audio thread (§2): the waveform (Gain, Tremolo,
   Stutter), the analyser behind the EQs (optional in the mockup, comes here), the vectorscope
   (Width), LFO phases (Chorus, Flanger, Phaser, AutoPan), lane levels in containers.

**Axes (2026-11-23):** every display has labelled axes that never move; knobs change only the
content (delay 0–4 s, reverbs 0–10 s, LFOs 0–2 s, Haas 0–40 ms, frequency always 20 Hz–20 kHz).
Morph, Ladder, Comb, Formant, Resonator, Flanger and Phaser draw the DSP's real transfer functions
(`dsp::preview`, checked against audio by `preview_curves_match_the_audio`). Bitcrusher and RingMod
show the wet signal whatever the mix. Synced times use the host tempo (`Shared::tempo`).

## 2. Live data from the audio thread

The audio thread processes the chain in blocks (the host decides the size and the rate), the UI
draws at about 60 fps; they never run in step and the audio thread must never wait, lock or
allocate. So the audio thread **writes**, the UI **reads when it draws**, and a buffer sits between
them. Nothing schedules or pumps; if the UI lags, old data is overwritten (a display only has to
look right, not be complete). The codebase already does this: meters, gain reduction and modulator
phases are atomics (`Shared::meter_in`, `Graph::modulator_phases`), graphs travel through `rtrb`,
Listen's capture through a mutex the audio thread only `try_lock`s.

- **Taps record only while subscribed.** A tap belongs to a position in the graph (a block inside a
  band split sees only its band). The UI switches on the taps of the open modal, one flag each; the
  audio thread checks the flag per block (a few nanoseconds) and records nothing otherwise. Closing
  the modal or walking to another block moves the flag. Most of the time nothing records. What
  always runs is what runs today: the meters behind the LEDs (gain reduction is computed anyway).
- **What crosses over** depends on the display: one number (levels, gain reduction, phase; exists),
  min/max per ~5 ms for a scrolling waveform (reduced on the audio thread), raw samples of one tap for
  an analyser (FFT on the UI side, never on the audio thread), thinned L/R pairs for the vectorscope.
- **Time, not samples.** The host sets the sample rate (44.1 … 192 kHz, fixed while active) and a
  maximum block size; actual blocks vary (Reaper splits at automation points, some hosts send 0 or 1
  samples). Reductions are in milliseconds, turned into sample counts at activation; tap buffers are
  allocated at activation for the worst case; a tap accumulates samples however the host slices them.
- **Rebuilds:** every route edit rebuilds the graph (`Graph::adopt`); taps survive it like meters do.
- **Oversampled blocks** run at 2–8× the rate; their tap sits at the normal rate.

## 3. Knob groups and choices

The modal holds 8 knobs (4 × 2). Bigger blocks split their knobs into **groups, shown as pages**:

- Each block type declares its groups, in order, at most 8 controls each; the first group is what's
  reached for most. More than one group: a pill row above the knob grid switches (`Main | Detector |
  Character`); the display stays, only the grid swaps. One group: no pills (most blocks).
- Compressor: Main (threshold, ratio, attack, release, knee, makeup, mix), Detector (detector,
  window, feedback, link, lookahead), Character (ratio curve, program, saturation).
- **Choices** (settings that aren't knobs: `phase = linear`, `pingpong`, a gate's mode) are cells of
  a knob's size inside a group (a switch or a pick list), not an extra row.
- Later: a patch may override a type's groups.

Until groups exist, more than 8 knobs scroll in the knob column.
