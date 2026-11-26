-- Kitchen Sink: every block in the catalogue (51), wired as one plausible chain, plus every
-- modulator. With the macros at their defaults it is a gentle mix-bus chain; the macros bring the
-- wilder sections in. No surface of its own: the shell shows the macros as the default surface.
--
--   input → tone → dynamics → band-split colour → mangle (crossfade) → movement → stereo →
--   space → master
--
-- Latency: the linear-phase air shelf (~91 ms at 48 kHz) plus the lookaheads (~6 ms) and the
-- oversampled distortion; the host compensates.

-- ---- input --------------------------------------------------------------------------------------
local input: { Block } = {
  Blocks.DCBlock.new("dc"),
  Blocks.Meter.new("in_meter"),
  Blocks.Gate.new("gate", { threshold = -70, ratio = 2, range = 20, lookahead = 1 }),
  Blocks.HighPass.new("rumble", { freq = 30, slope = 24 }),
}

-- ---- tone ---------------------------------------------------------------------------------------
local low = Blocks.LowShelf.new("low", { freq = 120, gain = 1 })
local presence = Blocks.PeakEq.new("presence", { freq = 3000, q = 0.8, gain = 0 })
local tone: { Block } = {
  low,
  presence,
  Blocks.HighShelf.new("air", { freq = 10000, gain = 1.5, phase = "linear" }),
  Blocks.DynamicEq.new("mud", { freq = 300, q = 1.2, threshold = -28, range = 6 }),
  Blocks.DeEsser.new("deess", { threshold = -26, range = 8, lookahead = 1 }),
}

-- ---- dynamics -----------------------------------------------------------------------------------
-- New York compression: a crossfade from dry to a hard FET compressor.
local smash = Blocks.Parallel.new("smash", {
  branches = { Chain.new(), Chain.new { Blocks.FETCompressor.new("fet", { threshold = -32, ratio = 8 }) } },
  mix = 0,
})
local dynamics: { Block } = {
  Blocks.Compressor.new("glue", { threshold = -20, ratio = 2, knee = 6, lookahead = 2, link = 100, auto_makeup = true }),
  smash,
  Blocks.OptoCompressor.new("level", { threshold = -26, ratio = 2 }),
  Blocks.Transient.new("punch", { attack = 2, sustain = 0, lookahead = 2 }),
  Blocks.Rider.new("rider", { target = -18, range = 3, speed = 800 }),
}

-- ---- colour: three bands, each with its own character -------------------------------------------
local mids_drive = Blocks.Drive.new("mids_drive", { amount = 2, mix = 60 })
local colour = Blocks.BandSplit.new("colour", {
  crossovers = { 150, 2500 },
  bands = {
    Chain.new {
      Blocks.VariMuCompressor.new("lows_vm", { threshold = -22 }),
      Blocks.Tape.new("lows_tape", { drive = 3, wow = 0, flutter = 5, mix = 50 }),
    },
    Chain.new {
      mids_drive,
      Blocks.Waveshaper.new("mids_shape", { curve = function(x) return x - 0.15 * x * x * x end, mix = 30 }),
      Blocks.Wavefolder.new("mids_fold", { amount = 3, bias = 10, mix = 5 }),
    },
    Chain.new {
      Blocks.Clipper.new("highs_clip", { ceiling = -3, softness = 80 }),
      Blocks.Bitcrusher.new("highs_crush", { bits = 12, mix = 3 }),
      Blocks.Rectifier.new("highs_rect", { mode = "full", mix = 4 }),
    },
  },
})

-- ---- mangle: a crossfade from clean to a chain of creative filters ------------------------------
local morph = Blocks.MorphFilter.new("morph", { freq = 1200, morph = 30, resonance = 35 })
local vowel = Blocks.Formant.new("vowel", { vowel = 1, resonance = 50, mix = 60 })
local mangle = Blocks.Parallel.new("mangle", {
  branches = {
    Chain.new(),
    Chain.new {
      Blocks.RingMod.new("ring", { freq = 110, mix = 20 }),
      vowel,
      Blocks.Resonator.new("reso", { notes = { "C2", "G2", "D3", "A3" }, decay = 700, mix = 25 }),
      Blocks.CombFilter.new("comb", { freq = 220, feedback = 55, mix = 25 }),
      morph,
      Blocks.LadderFilter.new("ladder", { freq = 5000, resonance = 25, drive = 3 }),
    },
  },
  mix = 0,
})

-- ---- movement -----------------------------------------------------------------------------------
local trem = Blocks.Tremolo.new("trem", { sync = "1/8", depth = 0, shape = "triangle" })
local phaser = Blocks.Phaser.new("phaser", { sync = "2/1", stages = 6, mix = 10 })
local movement: { Block } = {
  Blocks.Chorus.new("chorus", { rate = 0.6, depth = 30, mix = 15 }),
  Blocks.Flanger.new("flanger", { rate = 0.15, depth = 40, feedback = 30, mix = 8 }),
  phaser,
  trem,
  Blocks.AutoPan.new("autopan", { sync = "1/1", depth = 15 }),
}

-- ---- stereo -------------------------------------------------------------------------------------
local width = Blocks.Width.new("width", { width = 110 })
local stereo: { Block } = {
  Blocks.MidSide.new("ms", {
    mid = Chain.new { Blocks.LowPass.new("mid_lp", { freq = 19000, slope = 12 }) },
    side = Chain.new { Blocks.HighPass.new("side_hp", { freq = 120, slope = 24 }) },
  }),
  width,
  Blocks.MonoBass.new("mono_bass", { freq = 100 }),
  Blocks.Haas.new("haas", { delay = 0.3, channel = "right" }),
  Blocks.LeftRight.new("lr", {
    left = Chain.new { Blocks.Gain.new("trim_l", { db = 0 }) },
    right = Chain.new { Blocks.Gain.new("trim_r", { db = 0 }) },
  }),
  Blocks.Pan.new("balance", { pan = 0 }),
  Blocks.Invert.new("polarity", { channels = "both" }),
}

-- ---- space --------------------------------------------------------------------------------------
local echo = Blocks.Delay.new("echo", {
  sync = "1/8.", feedback = 35, mix = 0, pingpong = true,
  feedback_chain = Chain.new {
    Blocks.HighPass.new("echo_hp", { freq = 250 }),
    Blocks.LowPass.new("echo_lp", { freq = 6000 }),
    Blocks.PitchShift.new("echo_pitch", { semitones = 12, mix = 15 }),
  },
})
local verb = Blocks.Reverb.new("verb", { size = 60, decay = 2.2, predelay = 15, damping = 55, mix = 0 })
local shimmer = Blocks.Shimmer.new("shimmer", { decay = 6, pitch = 12, amount = 50, mix = 0 })
local grains = Blocks.Granular.new("grains", { density = 25, pitch = 12, jitter = 15, reverse = 25, mix = 0 })
local stutter = Blocks.Stutter.new("stutter", { active = 0, sync = "1/16" })
local space: { Block } = { echo, verb, shimmer, grains, stutter }

-- ---- master -------------------------------------------------------------------------------------
local out = Blocks.Gain.new("out", { db = 0 })
local master: { Block } = {
  out,
  Blocks.Limiter.new("limit", { ceiling = -0.5, release = 60, lookahead = 1.5 }),
  Blocks.Meter.new("out_meter"),
}

local character: { Block } = { colour, mangle }
local chain: { Block } = {}
for _, part in ipairs({ input, tone, dynamics, character, movement, stereo, space, master }) do
  for _, b in ipairs(part) do chain[#chain + 1] = b end
end

-- ---- macros -------------------------------------------------------------------------------------
local macros = {
  Macro.new("drive", "Drive", { default = 0 }):drives(mids_drive.params.amount, { from = 2, to = 18 }),
  Macro.new("smash", "Smash", { default = 0 }):drives(smash.params.mix, { from = 0, to = 70 }),
  Macro.new("mangle", "Mangle", { default = 0 }):drives(mangle.params.mix, { from = 0, to = 100 }),
  Macro.new("tone", "Tone", { default = 0.5 })
    :drives(low.params.gain, { from = 4, to = -2 })
    :drives(presence.params.gain, { from = -3, to = 3 }),
  Macro.new("width", "Width", { default = 0.25 }):drives(width.params.width, { from = 80, to = 180 }),
  Macro.new("space", "Space", { default = 0 })
    :drives(verb.params.mix, { from = 0, to = 35 })
    :drives(echo.params.mix, { from = 0, to = 25 }),
  Macro.new("shimmer", "Shimmer", { default = 0 }):drives(shimmer.params.mix, { from = 0, to = 40 }),
  Macro.new("grains", "Grains", { default = 0 }):drives(grains.params.mix, { from = 0, to = 50 }),
  Macro.new("pulse", "Pulse", { default = 0 }):drives(trem.params.depth, { from = 0, to = 80 }),
  Macro.new("freeze", "Freeze", { default = 0 }):drives(verb.params.freeze),
  Macro.new("stutter", "Stutter", { default = 0 }):drives(stutter.params.active),
  Macro.new("output", "Output", { default = 0.5 }):drives(out.params.db, { from = -12, to = 12 }),
}
local space_macro = macros[6]

-- ---- modulators ---------------------------------------------------------------------------------
-- Every modulator: LFO 1 sweeps the mangle filter, LFO 2 breathes the width, LFO 3 pumps the
-- phaser on quarter notes, Step walks the vowel, Follow ducks the space macro when it's loud.
local modulators: { ModulatorConfig } = {
  { id = "follow", attack_ms = 5, release_ms = 300 },
  { id = "lfo1", sync = "2/1", shape = "sine" },
  { id = "lfo2", rate_hz = 0.13, shape = "triangle" },
  { id = "lfo3", sync = "1/4", shape = "square" },
  { id = "step", sync = "1/8", values = { 0.1, 0.4, 0.8, 0.5, 0.95, 0.3, 0.6, 0.2 } },
}
local assignments: { ModAssignment } = {
  { source = "lfo1", destination = morph.params.freq, amount = 0.4, bipolar = true },
  { source = "lfo2", destination = width.params.width, amount = 0.1, bipolar = true },
  { source = "lfo3", destination = phaser.params.mix, amount = 0.2 },
  { source = "step", destination = vowel.params.vowel, amount = 0.8, bipolar = true },
  { source = "follow", destination = space_macro, amount = -0.4 },
}

return Patch.new("Kitchen Sink", {
  author = "sandbox",
  chain = Chain.new(chain),
  macros = macros,
  modulators = modulators,
  assignments = assignments,
  theme = { colors = { accent = "#f59e0b", bg = "#14161a" } },
})
