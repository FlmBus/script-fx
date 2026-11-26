-- ScriptFX API as Luau type definitions (for luau-lsp / `luau-lsp analyze`).
-- Also the compact API description to hand to an LLM.
--
-- A patch folder has:
--   init.luau   the entry point (or init.lua): the patch folder as a Luau module, returning
--               Patch.new(name, { chain, macros, theme, surface }). Everything else is up to the
--               author: one file for a simple patch, or parts in their own modules. Luau's rule
--               for init files: inside init.luau the patch's own files are "@self/...":
--               `theme = require("@self/theme")`, `require("@self/ui")`. Other files use "./x".
--
-- Conventions: constructors take the identity positionally (id, display name)
-- and every setting by name in an options table. Unknown options, missing
-- required options and wrong types are also errors at runtime, pointing at the line.
-- Components annotate their props: render = function(p: Props<{ ... }>) ... end

---------------------------------------------------------------- patch

-- A block's knob ("<block id>.<setting>"; no Param.new: every param belongs to a block).
-- Drive it with a macro (macro:drives(echo.params.feedback)) or bind it on the surface; otherwise
-- it's free: its value is saved with the project.
declare extern type Param with
  id: string
  name: string
  -- Narrows the knob to lo..hi inside the block type's range (knob, panel, typed values; a
  -- macro's from/to default to it). The start value must be inside.
  function range(self, lo: number, hi: number): Param
end

declare extern type Macro with
  id: string
  name: string
  -- from/to: the param's plain value at macro 0 / 1 (default: param min / max; to < from inverts).
  -- curve "exp" = from * (to/from)^v (frequencies); needs non-zero from/to with the same sign.
  function drives(self, param: Param, o: { from: number?, to: number?, curve: ("lin" | "exp")? }?): Macro
end

-- A chain block. Ids are unique per patch. Units: dB, Hz, ms, % (0..100).
declare extern type Block with
  id: string
end

-- Metered blocks (compressors, Limiter, Gate, DeEsser, DynamicEq, Rider, Transient, Meter) report a
-- meter: usable as a component's `source`.
declare extern type MeteredBlock extends Block with
end

-- The blocks' knobs, per type (block.params.<setting>).
declare extern type GainBlock extends Block with
  params: { db: Param }
end
declare extern type PanBlock extends Block with
  params: { pan: Param }
end
declare extern type WidthBlock extends Block with
  params: { width: Param }
end
declare extern type MonoBassBlock extends Block with
  params: { freq: Param, slope: Param }
end
declare extern type HaasBlock extends Block with
  params: { delay: Param }
end
declare extern type InvertBlock extends Block with
  params: {}
end
declare extern type DCBlockBlock extends Block with
  params: {}
end
declare extern type MeterBlock extends MeteredBlock with
  params: {}
end
declare extern type PeakEqBlock extends Block with
  params: { freq: Param, gain: Param, q: Param }
end
declare extern type LowShelfBlock extends Block with
  params: { freq: Param, gain: Param, q: Param }
end
declare extern type HighShelfBlock extends Block with
  params: { freq: Param, gain: Param, q: Param }
end
declare extern type LowPassBlock extends Block with
  params: { freq: Param, q: Param, slope: Param }
end
declare extern type HighPassBlock extends Block with
  params: { freq: Param, q: Param, slope: Param }
end
declare extern type DriveBlock extends Block with
  params: { amount: Param, mix: Param }
end
declare extern type WaveshaperBlock extends Block with
  params: { drive: Param, output: Param, mix: Param }
end
declare extern type WavefolderBlock extends Block with
  params: { amount: Param, bias: Param, mix: Param }
end
declare extern type ClipperBlock extends Block with
  params: { ceiling: Param, softness: Param, drive: Param }
end
declare extern type RectifierBlock extends Block with
  params: { mix: Param }
end
declare extern type BitcrusherBlock extends Block with
  params: { bits: Param, rate: Param, mix: Param }
end
declare extern type RingModBlock extends Block with
  params: { freq: Param, mix: Param }
end
declare extern type TapeBlock extends Block with
  params: { drive: Param, wow: Param, flutter: Param, mix: Param }
end
declare extern type MorphFilterBlock extends Block with
  params: { freq: Param, morph: Param, resonance: Param }
end
declare extern type LadderFilterBlock extends Block with
  params: { freq: Param, resonance: Param, drive: Param }
end
declare extern type CombFilterBlock extends Block with
  params: { freq: Param, feedback: Param, mix: Param }
end
declare extern type FormantBlock extends Block with
  params: { vowel: Param, shift: Param, resonance: Param, mix: Param }
end
declare extern type ResonatorBlock extends Block with
  params: { decay: Param, mix: Param }
end
declare extern type DynamicEqBlock extends MeteredBlock with
  params: { freq: Param, q: Param, threshold: Param, ratio: Param, range: Param, attack: Param, release: Param }
end
declare extern type ChorusBlock extends Block with
  params: { rate: Param, depth: Param, mix: Param }
end
declare extern type FlangerBlock extends Block with
  params: { rate: Param, depth: Param, feedback: Param, mix: Param }
end
declare extern type PhaserBlock extends Block with
  params: { rate: Param, depth: Param, feedback: Param, freq: Param, mix: Param }
end
declare extern type TremoloBlock extends Block with
  params: { rate: Param, phase_offset: Param, depth: Param }
end
declare extern type AutoPanBlock extends Block with
  params: { rate: Param, phase_offset: Param, depth: Param }
end
declare extern type DelayBlock extends Block with
  params: { time: Param, feedback: Param, mix: Param }
end
declare extern type ReverbBlock extends Block with
  params: { size: Param, decay: Param, damping: Param, predelay: Param, width: Param, mix: Param, freeze: Param }
end
declare extern type ShimmerBlock extends Block with
  params: { size: Param, decay: Param, damping: Param, predelay: Param, width: Param, mix: Param, pitch: Param, amount: Param }
end
declare extern type PitchShiftBlock extends Block with
  params: { semitones: Param, window: Param, mix: Param }
end
declare extern type GranularBlock extends Block with
  params: { grain: Param, density: Param, spread: Param, pitch: Param, jitter: Param, reverse: Param, width: Param, mix: Param }
end
declare extern type StutterBlock extends Block with
  params: { active: Param, length: Param, mix: Param }
end
export type CompParams = {
  threshold: Param, ratio: Param, attack: Param, release: Param, knee: Param, makeup: Param, mix: Param,
  detector: Param, window: Param, feedback: Param, ratio_curve: Param, program: Param, link: Param, saturation: Param,
}
declare extern type CompressorBlock extends MeteredBlock with
  params: CompParams
end
declare extern type FETCompressorBlock extends MeteredBlock with
  params: CompParams
end
declare extern type OptoCompressorBlock extends MeteredBlock with
  params: CompParams
end
declare extern type VariMuCompressorBlock extends MeteredBlock with
  params: CompParams
end
declare extern type RiderBlock extends MeteredBlock with
  params: { target: Param, range: Param, speed: Param }
end
declare extern type TransientBlock extends MeteredBlock with
  params: { attack: Param, sustain: Param }
end
declare extern type LimiterBlock extends MeteredBlock with
  params: { ceiling: Param, release: Param }
end
declare extern type GateBlock extends MeteredBlock with
  params: { threshold: Param, ratio: Param, range: Param, attack: Param, hold: Param, release: Param }
end
declare extern type DeEsserBlock extends MeteredBlock with
  params: { freq: Param, threshold: Param, range: Param }
end
declare extern type MidSideBlock extends Block with
  params: {}
end
declare extern type LeftRightBlock extends Block with
  params: {}
end
declare extern type BandSplitBlock extends Block with
  params: { crossovers: { Param }, slope: Param }
end
declare extern type ParallelBlock extends Block with
  params: { mix: Param }
end

declare extern type Chain with
  function add(self, block: Block): Chain
end

declare extern type Patch with
  name: string
end

-- The plugin's own input/output level, usable as a component's `source`.
declare extern type LevelMeter with
end

-- A knob's start value. Every numeric block setting is a knob: the block makes it a Param
-- ("<block id>.<setting>", saved with the project), reachable as block.params.<setting>.
-- Not given = the block type's default; out of range = an error.
export type Setting = number
export type Sync = string                                                        -- "1/8", "1/8.", "1/16t", "1/1"
export type LfoShape = "sine" | "triangle" | "square"

-- phase: "minimum" (default; IIR, no latency) or "linear" (FIR: no phase shift, ~90 ms latency, pre-ringing)
export type Phase = "minimum" | "linear"
export type EqOptions = { freq: Setting?, gain: Setting?, q: Setting?, phase: Phase?, oversampling: number? }          -- Hz, dB; q default 0.707
export type FilterOptions = { freq: Setting?, q: Setting?, slope: Setting?, phase: Phase?, oversampling: number? }      -- Hz, resonance, dB/oct (12, 24, 36, 48)
export type CompOptions = {
  threshold: Setting?,                                                           -- dB
  ratio: Setting?, attack: Setting?, release: Setting?,                         -- :1, ms, ms
  knee: Setting?, makeup: Setting?,                                             -- dB, dB
  auto_makeup: boolean?,                                                        -- gives back ~80 % of the average reduction
  mix: Setting?,                                                                -- % wet (parallel compression), default 100
  detector: Setting?, window: Setting?,                                         -- % 0 = peak .. 100 = RMS; RMS window ms
  feedback: Setting?,                                                           -- % 0 = feed-forward .. 100 = feedback
  ratio_curve: Setting?,                                                        -- % the ratio rises with level (vari-mu)
  program: Setting?,                                                            -- % program-dependent release
  link: Setting?,                                                               -- % stereo link (default 100)
  saturation: Setting?,                                                         -- % gain-stage colour (even harmonics)
  lookahead: number?,                                                           -- ms 0..20, fixed (adds latency)
  oversampling: number?,
}

declare Macro: {
  -- host-automatable (max 16 per patch); default 0..1 (default: from the first target's param default)
  new: (id: string, name: string, o: { default: number? }?) -> Macro,
}

declare Chain: {
  -- blocks processed in series
  new: ((blocks: { Block }) -> Chain) & (() -> Chain),
}

-- Every block is documented with `--- <Category> · <what it does>`. Categories:
-- Utility, EQ, Filter, Dynamics, Distortion, Modulation, Space, Pitch, Rhythm, Routing.
-- (Search by category, e.g. "Routing ·".) Percent settings are 0..100 unless noted.
-- `sync` (LFO and time blocks): a note length at the host tempo ("1/4", "1/8." dotted,
-- "1/16t" triplet, "2/1" two bars); overrides `rate` / `time` / `length`.
--
-- Every block also takes `oversampling` (1, 2, 4 or 8): the block, and everything inside
-- it, runs at that multiple of the rate. Default: Distortion blocks automatically get
-- enough for >= 88 kHz (2x at 44.1/48 kHz), others none. Oversampling adds latency
-- (~0.7 ms at 2x); parallel paths are delay-compensated, the plugin reports the total.
-- Nested oversampling multiplies (max 8x in total).
declare Blocks: {
  --- Utility · volume in dB
  Gain: { new: (id: string, o: { db: Setting?, oversampling: number? }) -> GainBlock },
  --- Utility · stereo balance: pan -100 (left) .. 100 (right); the side panned away from is attenuated
  Pan: { new: (id: string, o: { pan: Setting?, oversampling: number? }) -> PanBlock },
  --- Utility · stereo width via mid/side, %: 0 = mono, 100 = unchanged, 200 = twice the side signal
  Width: { new: (id: string, o: { width: Setting?, oversampling: number? }) -> WidthBlock },
  --- Utility · mono below `freq` Hz (side signal high-passed, `slope` 24 or 48 dB/oct, phase-aligned)
  MonoBass: { new: (id: string, o: { freq: Setting?, slope: Setting?, oversampling: number? }) -> MonoBassBlock },
  --- Utility · delays one channel by `delay` ms (-40..40): > 0 delays `channel` (default "right"), < 0 the other: Haas widening
  Haas: { new: (id: string, o: { delay: Setting?, channel: ("left" | "right")?, oversampling: number? }) -> HaasBlock },
  --- Utility · polarity inversion (default "both")
  Invert: { new: (id: string, o: { channels: ("both" | "left" | "right")?, oversampling: number? }?) -> InvertBlock },
  --- Utility · removes DC offset (high-pass at 5 Hz)
  DCBlock: { new: (id: string, o: { oversampling: number? }?) -> DCBlockBlock },
  --- Utility · passive peak meter at this point of the chain (a component `source`, level like Meters.IN)
  Meter: { new: (id: string, o: { oversampling: number? }?) -> MeterBlock },

  --- EQ · bell boost/cut (q default 0.707); phase "minimum" (default) or "linear"
  PeakEq: { new: (id: string, o: EqOptions) -> PeakEqBlock },
  --- EQ · low shelf
  LowShelf: { new: (id: string, o: EqOptions) -> LowShelfBlock },
  --- EQ · high shelf
  HighShelf: { new: (id: string, o: EqOptions) -> HighShelfBlock },

  --- Filter · low-pass; slope 12/24/36/48 dB/oct (default 12), q = resonance (0.707 = none)
  LowPass: { new: (id: string, o: FilterOptions) -> LowPassBlock },
  --- Filter · high-pass; slope 12/24/36/48 dB/oct (default 12), q = resonance (0.707 = none)
  HighPass: { new: (id: string, o: FilterOptions) -> HighPassBlock },

  --- Distortion · tanh saturator; amount = pre-gain dB (0..36), mix % (default 100)
  Drive: { new: (id: string, o: { amount: Setting?, mix: Setting?, oversampling: number? }) -> DriveBlock },

  --- Distortion · custom transfer curve: `curve` = function(x) -> y for x in -1..1 (sampled once at
  --- load), or a list of y values evenly spaced over -1..1. drive / output dB (default 0), mix %
  Waveshaper: { new: (id: string, o: { curve: ((x: number) -> number) | { number }, drive: Setting?, output: Setting?, mix: Setting?, oversampling: number? }) -> WaveshaperBlock },
  --- Distortion · sine wavefolder: amount = pre-gain dB (0..36, more folds), bias % (-100..100, asymmetry
  --- = even harmonics), mix %
  Wavefolder: { new: (id: string, o: { amount: Setting?, bias: Setting?, mix: Setting?, oversampling: number? }) -> WavefolderBlock },
  --- Distortion · clipper: ceiling dB (default 0), softness % (0 = hard, 100 = round), drive dB (default 0)
  Clipper: { new: (id: string, o: { ceiling: Setting?, softness: Setting?, drive: Setting?, oversampling: number? }?) -> ClipperBlock },
  --- Distortion · rectifier: mode "full" (default, octave up) or "half"; DC removed; mix %
  Rectifier: { new: (id: string, o: { mode: ("half" | "full")?, mix: Setting?, oversampling: number? }?) -> RectifierBlock },
  --- Distortion · bitcrusher: bits 1..24 (default 8), rate = sample-and-hold Hz (default: off), mix %.
  --- Aliases on purpose (not oversampled automatically)
  Bitcrusher: { new: (id: string, o: { bits: Setting?, rate: Setting?, mix: Setting?, oversampling: number? }?) -> BitcrusherBlock },
  --- Distortion · ring modulator with a sine at freq Hz (metallic, bell-like), mix %
  RingMod: { new: (id: string, o: { freq: Setting?, mix: Setting?, oversampling: number? }) -> RingModBlock },
  --- Distortion · tape: drive dB (default 6, asymmetric saturation + head bump + HF loss), wow % and
  --- flutter % (pitch wobble, default 0), mix %
  Tape: { new: (id: string, o: { drive: Setting?, wow: Setting?, flutter: Setting?, mix: Setting?, oversampling: number? }?) -> TapeBlock },

  --- Filter · state-variable filter: morph % 0 = low-pass, 50 = band-pass, 100 = high-pass (default 0);
  --- resonance % (default 0)
  MorphFilter: { new: (id: string, o: { freq: Setting?, morph: Setting?, resonance: Setting?, oversampling: number? }) -> MorphFilterBlock },
  --- Filter · 24 dB/oct ladder low-pass with saturating stages: resonance % (100 = self-oscillation),
  --- drive dB (default 0)
  LadderFilter: { new: (id: string, o: { freq: Setting?, resonance: Setting?, drive: Setting?, oversampling: number? }) -> LadderFilterBlock },
  --- Filter · feedback comb tuned to freq Hz: feedback % -99..99 (default 70), mix % (default 50)
  CombFilter: { new: (id: string, o: { freq: Setting?, feedback: Setting?, mix: Setting?, oversampling: number? }) -> CombFilterBlock },
  --- Filter · vowel filter: vowel 0..4 = a, e, i, o, u (in between morphs), shift semitones (-12..12),
  --- resonance % (default 50), mix % (default 100)
  Formant: { new: (id: string, o: { vowel: Setting?, shift: Setting?, resonance: Setting?, mix: Setting?, oversampling: number? }) -> FormantBlock },
  --- Filter · resonator bank: notes = Hz or names ({ "C2", "G2", 220 }, max 16), decay ms (default 500),
  --- mix % (default 50)
  Resonator: { new: (id: string, o: { notes: { number | string }, decay: Setting?, mix: Setting?, oversampling: number? }) -> ResonatorBlock },
  --- Filter · dynamic EQ band: a peak at freq (q default 1) cut (mode "cut", default) or boosted ("boost")
  --- by the band's level above threshold dB (ratio default 3, at most range dB, default 12; attack/release
  --- ms 5/100). Meter: gain change
  DynamicEq: { new: (id: string, o: { freq: Setting?, q: Setting?, threshold: Setting?, ratio: Setting?, range: Setting?, attack: Setting?, release: Setting?, mode: ("cut" | "boost")?, oversampling: number? }) -> DynamicEqBlock },

  --- Modulation · chorus: rate Hz (default 0.8) or sync, depth % (default 50), mix % (default 50)
  Chorus: { new: (id: string, o: { rate: Setting?, sync: Sync?, depth: Setting?, mix: Setting?, oversampling: number? }?) -> ChorusBlock },
  --- Modulation · flanger: rate Hz (default 0.3) or sync, depth % (default 70), feedback % -99..99
  --- (default 50), mix % (default 50)
  Flanger: { new: (id: string, o: { rate: Setting?, sync: Sync?, depth: Setting?, feedback: Setting?, mix: Setting?, oversampling: number? }?) -> FlangerBlock },
  --- Modulation · phaser: rate Hz (default 0.5) or sync, depth % (default 70), feedback % -99..99
  --- (default 30), freq = sweep center Hz (default 800), stages 2..12 even (default 6), mix % (default 50)
  Phaser: { new: (id: string, o: { rate: Setting?, sync: Sync?, depth: Setting?, feedback: Setting?, freq: Setting?, stages: number?, mix: Setting?, oversampling: number? }?) -> PhaserBlock },
  --- Modulation · tremolo (volume LFO): rate Hz (default 5) or sync, depth % (default 50), shape,
  --- phase_offset = LFO phase shift in degrees (default 0). Synced + host playing: locked to the beat
  --- grid (a "1/4" square tremolo is loud on the first half of every beat; phase_offset 180 flips it)
  Tremolo: { new: (id: string, o: { rate: Setting?, sync: Sync?, phase_offset: Setting?, depth: Setting?, shape: LfoShape?, oversampling: number? }?) -> TremoloBlock },
  --- Modulation · auto-pan (balance LFO): rate Hz (default 1) or sync, depth % (default 100), shape,
  --- phase_offset in degrees (default 0; synced + host playing: locked to the beat grid, like Tremolo)
  AutoPan: { new: (id: string, o: { rate: Setting?, sync: Sync?, phase_offset: Setting?, depth: Setting?, shape: LfoShape?, oversampling: number? }?) -> AutoPanBlock },

  --- Space · echo: time ms (1..4000, default 375) or sync, feedback % (default 35), mix % (default 30),
  --- pingpong (bounce L/R); feedback_chain processes every repeat (e.g. a filter, a crusher, a pitch shift)
  Delay: { new: (id: string, o: { time: Setting?, sync: Sync?, feedback: Setting?, mix: Setting?, pingpong: boolean?, feedback_chain: Chain?, oversampling: number? }?) -> DelayBlock },
  --- Space · reverb (8-line feedback delay network): size % (default 50), decay s (RT60, default 2),
  --- damping % (default 50), predelay ms (default 0), width % (default 100), mix % (default 30),
  --- freeze (> 0.5 = infinite sustain, no new input; drive it with a Param for a toggle)
  Reverb: { new: (id: string, o: { size: Setting?, decay: Setting?, damping: Setting?, predelay: Setting?, width: Setting?, mix: Setting?, freeze: Setting?, oversampling: number? }?) -> ReverbBlock },
  --- Space · shimmer reverb: the reverb's tail is pitch-shifted back into it (pitch semitones, default 12;
  --- amount %, default 50); other settings as Reverb
  Shimmer: { new: (id: string, o: { size: Setting?, decay: Setting?, damping: Setting?, predelay: Setting?, width: Setting?, mix: Setting?, pitch: Setting?, amount: Setting?, oversampling: number? }?) -> ShimmerBlock },

  --- Pitch · pitch shifter: semitones -24..24, window ms (default 60; shorter = tighter, grainier),
  --- mix % (default 100)
  PitchShift: { new: (id: string, o: { semitones: Setting?, window: Setting?, mix: Setting?, oversampling: number? }) -> PitchShiftBlock },
  --- Pitch · granular cloud from the last 2 s: grain ms (default 80), density grains/s (default 20),
  --- spread = how far back grains start, ms (default 300), pitch semitones (default 0), jitter = random
  --- pitch cents (default 0), reverse % of grains (default 0), width % (default 100), mix % (default 50)
  Granular: { new: (id: string, o: { grain: Setting?, density: Setting?, spread: Setting?, pitch: Setting?, jitter: Setting?, reverse: Setting?, width: Setting?, mix: Setting?, oversampling: number? }?) -> GranularBlock },

  --- Rhythm · stutter / beat repeat: while active > 0.5 (drive it with a Param/Macro), the slice of
  --- length ms (default 125) or sync that started then repeats; mix % (default 100)
  Stutter: { new: (id: string, o: { active: Setting?, length: Setting?, sync: Sync?, mix: Setting?, oversampling: number? }) -> StutterBlock },

  --- Dynamics · versatile compressor, every behaviour a knob (detector, topology, ratio curve, program-
  --- dependent release, link, saturation, lookahead); defaults: clean feed-forward, peak, 2:1, 5/100 ms,
  --- knee 4. The other compressors are presets of it. Meter: gain reduction
  Compressor: { new: (id: string, o: CompOptions) -> CompressorBlock },
  --- Dynamics · compressor preset FET (1176-ish): peak, feedback, fast (4:1, 0.3/80 ms, knee 2, saturation 15).
  --- Meter: gain reduction
  FETCompressor: { new: (id: string, o: CompOptions) -> FETCompressorBlock },
  --- Dynamics · compressor preset opto (LA-2A-ish): RMS 30 ms, feedback, program-dependent release (3:1,
  --- 10/250 ms, knee 8). Meter: gain reduction
  OptoCompressor: { new: (id: string, o: CompOptions) -> OptoCompressorBlock },
  --- Dynamics · compressor preset vari-mu (Fairchild-ish): ratio rising with level (6:1 at the top), feedback,
  --- slow program-dependent timing (25/400 ms), soft knee 12, tube saturation 30. Meter: gain reduction
  VariMuCompressor: { new: (id: string, o: CompOptions) -> VariMuCompressorBlock },
  --- Dynamics · slow leveler towards target dBFS within ±range dB (speed ms, default 300). Meter: gain
  Rider: { new: (id: string, o: { target: Setting?, range: Setting?, speed: Setting?, oversampling: number? }) -> RiderBlock },
  --- Dynamics · transient shaper, dB boost/cut of attacks / sustain; lookahead ms (0..20, fixed). Meter: gain
  Transient: { new: (id: string, o: { attack: Setting?, sustain: Setting?, lookahead: number?, oversampling: number? }) -> TransientBlock },
  --- Dynamics · lookahead peak limiter: ceiling dB (default -0.3), release ms (default 80), lookahead ms
  --- (fixed number 0..20, default 1.5; adds that much latency). Meter: gain reduction
  Limiter: { new: (id: string, o: { ceiling: Setting?, release: Setting?, lookahead: number?, oversampling: number? }?) -> LimiterBlock },
  --- Dynamics · gate / downward expander below threshold dB: ratio (default 20 ≈ gate, 2 = gentle
  --- expander), range = max attenuation dB (default 60), attack/hold/release ms (1/20/150), lookahead ms
  --- (0..20, fixed: opens before the hit). Meter: attenuation
  Gate: { new: (id: string, o: { threshold: Setting?, ratio: Setting?, range: Setting?, attack: Setting?, hold: Setting?, release: Setting?, lookahead: number?, oversampling: number? }) -> GateBlock },
  --- Dynamics · de-esser: reduces the band above freq Hz (default 6000) when it exceeds threshold dB, by
  --- at most range dB (default 12); lookahead ms (0..20, fixed). Meter: gain reduction
  DeEsser: { new: (id: string, o: { freq: Setting?, threshold: Setting?, range: Setting?, lookahead: number?, oversampling: number? }) -> DeEsserBlock },

  --- Routing · mid/side: runs `mid` and `side` (each as mono), then decodes back to L/R
  MidSide: { new: (id: string, o: { mid: Chain?, side: Chain?, oversampling: number? }) -> MidSideBlock },
  --- Routing · runs `left` and `right` on their own channel (each as mono)
  LeftRight: { new: (id: string, o: { left: Chain?, right: Chain?, oversampling: number? }) -> LeftRightBlock },
  --- Routing · Linkwitz-Riley split (`slope` 24 = LR4 or 48 = LR8 dB/oct) into #crossovers+1 bands (ascending Hz, phase-aligned, sums flat);
  --- each band is processed by its Chain, then all are summed. phase "linear": complementary linear-phase
  --- bands (no phase shift between them, reconstructs exactly; ~90 ms latency)
  BandSplit: { new: (id: string, o: { crossovers: { Setting }, slope: Setting?, bands: { Chain }, phase: Phase?, oversampling: number? }) -> BandSplitBlock },
  --- Routing · every branch gets the full signal, outputs summed; with mix (exactly 2 branches) a
  --- crossfade instead: 0 % = first, 100 % = second. Chain.new() = a dry branch
  Parallel: { new: (id: string, o: { branches: { Chain }, mix: Setting?, oversampling: number? }) -> ParallelBlock },
}

declare Meters: { IN: LevelMeter, OUT: LevelMeter }

-- A font file from the patch folder: .ttf/.otf, .svg (SVG 1.1 `<font>`: glyphs as paths or shapes,
-- drawn in `currentColor`) or .fnt (BMFont image font + PNG atlas).
declare extern type Font with
  path: string
end
declare Font: { new: (path: string) -> Font }

-- An image file from the patch folder: .svg or .png.
declare extern type Image with
  path: string
end
declare Image: { new: (path: string) -> Image }

--- The patch's design tokens, shared by the library card and the patch's surface (`p.theme`).
--- `colors` / `fonts` / `images`: the patch's own names. The shell uses `colors.accent`, `.bg`,
--- `.ink`, `fonts.title` and `images.logo` (the card shows the logo instead of the title);
--- `library` overrides them for the library card only. Colours are "#rrggbb".
export type Theme = {
  colors: { [string]: string }?,
  fonts: { [string]: Font }?,
  images: { [string]: Image }?,
  library: { accent: string?, bg: string?, ink: string?, font: Font?, logo: Image? }?,
}

export type ModulatorId = "follow" | "lfo1" | "lfo2" | "lfo3" | "step"
export type ModulatorLfoShape = "sine" | "triangle" | "square"

export type ModulatorConfig =
  | { id: "follow", attack_ms: number?, release_ms: number? }
  | { id: "lfo1" | "lfo2" | "lfo3", rate_hz: number?, sync: string?, phase: number?, shape: ModulatorLfoShape? }  -- sync: "1/8" overrides rate_hz
  | { id: "step", rate_hz: number?, sync: string?, values: { number }? }  -- sync: one step's length

export type ModAssignment = {
  source: ModulatorId,
  destination: Param | Macro,
  amount: number?,      -- −1..1: the size of the range it moves the knob in; negative = the other way
  bipolar: boolean?,    -- swing ±amount/2 around the knob instead of 0..amount
  curve: number?,
  bypass: boolean?,
}

declare Patch: {
  -- macros: order = host automation slots 1..16; surface: the patch surface's root instance
  new: (name: string, o: { author: string?, chain: Chain?, macros: { Macro }?,
                           modulators: { ModulatorConfig }?, assignments: { ModAssignment }?,
                           theme: Theme?, surface: ComponentInstance? }?) -> Patch,
}

---------------------------------------------------------------- UI

declare extern type ComponentInstance with
end

-- Something render() may return: SVG markup, a child instance, or a list of those.
export type Part = string | ComponentInstance | { Part }

-- What a component reads (the shell feeds the values in):
export type Bindable = Macro | Param           -- bind: p.value 0..1, p.display, p.locked (a macro-driven param drags its macro)
export type Source = MeteredBlock | LevelMeter -- source: p.level 0..1 (compressors, limiter, de-esser, dynamic EQ, gate: gain reduction 0..24 dB; rider/transient: gain, 0.5 = unity ±12 dB; in/out and Meter: -60..0 dBFS)

-- Props every instance accepts, besides the component's own.
export type Common = {
  key: (string | number)?,                     -- stable identity among siblings (keeps local state)
  x: number?, y: number?,                      -- position relative to the parent
  w: number?, h: number?,                      -- size; default: the definition's size, else the parent's
  bind: Bindable?,
  source: Source?,
  series: string?,                             -- p.series (number list), e.g. "spectrum"
  animated: boolean?,                          -- re-render every frame (p.time)
  theme: any,                                  -- inherited by children as p.theme
}

-- What render(p) receives besides the component's own props. A component's own
-- props must not reuse these names (pass e.g. `lvl` to a child, not `level`).
export type Inputs = {
  width: number, height: number,
  value: number?, display: string?, locked: boolean?, -- from bind
  level: number?,                                     -- from source
  series: { number }?,
  time: number?,                                      -- seconds (only when animated)
  hover: boolean,                                     -- pointer over it (interactive kinds only)
  dragging: boolean,                                  -- being dragged (knob kind only)
  state: { [string]: any },                           -- local state (survives re-renders and hot reload)
  theme: any,
}

export type Props<P> = P & Inputs

export type Component<P> = { new: (props: P & Common) -> ComponentInstance }

declare Component: {
  -- Each instance becomes its own cached layer, re-rendered only when its inputs change
  -- (props, bind value, meter, hover/drag, state, time if animated). Split static and
  -- moving parts into separate components.
  -- kind "knob": vertical drag edits `bind` (shift = fine, double-click = reset);
  -- kind "button": calls on_click(state, props).
  define: <P>(def: {
    name: string,
    kind: ("knob" | "button")?,
    size: { number }?,                         -- { w, h }
    animated: boolean?,
    on_click: ((state: { [string]: any }, props: P) -> ())?,
    render: (p: Props<P>) -> Part,
  }) -> Component<P>,
}

-- The patch surface's fixed logical size (the root always covers it).
declare UI: { WIDTH: number, HEIGHT: number }

export type Stop = { any }                    -- { offset 0..1, colour, opacity? }
-- Every shape also takes: fill, stroke, width (stroke width), opacity, cap ("round" | "butt" | "square"),
-- rotate ({ degrees, cx, cy }), transform (raw SVG transform, applied before rotate), filter (id from SVG.blur).
-- SVG helpers. Shapes return SVG markup; gradients and blur return (id, defs): put
-- `defs` into the output and use "url(#" .. id .. ")" as the fill.
declare SVG: {
  rect: (o: { x: number, y: number, w: number, h: number, rx: number?, fill: string?, stroke: string?, width: number?, opacity: number?, cap: ("round" | "butt" | "square")?, rotate: { number }?, transform: string?, filter: string? }) -> Part,
  circle: (o: { cx: number, cy: number, r: number, fill: string?, stroke: string?, width: number?, opacity: number?, cap: ("round" | "butt" | "square")?, rotate: { number }?, transform: string?, filter: string? }) -> Part,
  line: (o: { x1: number, y1: number, x2: number, y2: number, fill: string?, stroke: string?, width: number?, opacity: number?, cap: ("round" | "butt" | "square")?, rotate: { number }?, transform: string?, filter: string? }) -> Part,
  path: (o: { d: string, fill: string?, stroke: string?, width: number?, opacity: number?, cap: ("round" | "butt" | "square")?, rotate: { number }?, transform: string?, filter: string? }) -> Part,
  text: (o: { x: number, y: number, text: string, size: number?, weight: ("normal" | "bold")?,
              anchor: ("start" | "middle" | "end")?, spacing: number?, fill: string?, opacity: number?,
              family: string?, transform: string?, rotate: { number }?, filter: string? }) -> Part,
  -- arc from `from` to `to` degrees (0 = up, clockwise); glow = blur radius
  arc: (o: { cx: number, cy: number, r: number, from: number, to: number, glow: number?, fill: string?, stroke: string?, width: number?, opacity: number?, cap: ("round" | "butt" | "square")?, rotate: { number }?, transform: string?, filter: string? }) -> Part,
  -- image or SVG file from the patch folder
  image: (href: string, o: { x: number, y: number, w: number, h: number, fit: string?, opacity: number? }?) -> Part,
  raw: (s: string) -> Part,
  group: (list: { Part }) -> Part,
  -- from/to as fractions (default top → bottom)
  linear: (o: { stops: { Stop }, from: { number }?, to: { number }?, id: string? }) -> (string, Part),
  radial: (o: { stops: { Stop }, cx: number?, cy: number?, r: number?, id: string? }) -> (string, Part),
  -- pad = extra filter region as a fraction of the element (default 0.5); keep it small
  blur: (std: number, pad: number?) -> (string, Part),
}

declare Util: {
  lerp: (a: number, b: number, t: number) -> number,
  clamp: (v: number, lo: number, hi: number) -> number,
  iff: <T>(c: any, a: T, b: T) -> T,
}
