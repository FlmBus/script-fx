-- Every block from the distortion, filter, modulation, space, pitch, rhythm and
-- dynamics catalogue at least once (allocation-free test, type check, bench).
local cutoff = Param.new("cutoff", "Cutoff", { min = 200, max = 8000, default = 1200, unit = "Hz" })
local stutter = Param.new("stutter", "Stutter", { min = 0, max = 1, default = 1 })
local freeze = Param.new("freeze", "Freeze", { min = 0, max = 1, default = 0 })
local sweep = Macro.new("sweep", "Sweep"):drives(cutoff, { from = 300, to = 6000, curve = "exp" })
local hold = Macro.new("hold", "Hold"):drives(stutter)

local chain = Chain.new({
  Blocks.Waveshaper.new("shape", { curve = function(x) return math.tanh(3 * x) / math.tanh(3) end, drive = 3 }),
  Blocks.Wavefolder.new("fold", { amount = 6, bias = 20, mix = 30 }),
  Blocks.Clipper.new("clip", { ceiling = -1, softness = 50 }),
  Blocks.Rectifier.new("rect", { mode = "half", mix = 10 }),
  Blocks.Bitcrusher.new("crush", { bits = 10, rate = 22050, mix = 20 }),
  Blocks.RingMod.new("ring", { freq = 220, mix = 10 }),
  Blocks.Tape.new("tape", { drive = 4, wow = 30, flutter = 30 }),
  Blocks.MorphFilter.new("morph", { freq = cutoff, morph = 30, resonance = 40 }),
  Blocks.LadderFilter.new("ladder", { freq = 6000, resonance = 30, drive = 3 }),
  Blocks.CombFilter.new("comb", { freq = 110, feedback = 60, mix = 20 }),
  Blocks.Formant.new("vowel", { vowel = 1.5, mix = 30 }),
  Blocks.Resonator.new("reso", { notes = { "C2", "G2", 196 }, decay = 800, mix = 20 }),
  Blocks.DynamicEq.new("deq", { freq = 300, threshold = -24 }),
  Blocks.Chorus.new("chorus", { sync = "1/2" }),
  Blocks.Flanger.new("flanger", { rate = 0.2, mix = 30 }),
  Blocks.Phaser.new("phaser", { stages = 8, mix = 30 }),
  Blocks.Tremolo.new("trem", { sync = "1/8t", depth = 20, shape = "triangle" }),
  Blocks.AutoPan.new("pan", { rate = 0.5, depth = 30 }),
  Blocks.Delay.new("echo", {
    sync = "1/8.", feedback = 45, mix = 25, pingpong = true,
    feedback_chain = Chain.new({
      Blocks.HighPass.new("echo_hp", { freq = 300 }),
      Blocks.PitchShift.new("echo_pitch", { semitones = 7, mix = 40 }),
      Blocks.Drive.new("echo_drive", { amount = 6 }),
    }),
  }),
  Blocks.Reverb.new("verb", { size = 70, decay = 3, predelay = 20, mix = 20, freeze = freeze }),
  Blocks.Shimmer.new("shimmer", { decay = 5, mix = 15, amount = 60 }),
  Blocks.PitchShift.new("pitch", { semitones = -5, mix = 20 }),
  Blocks.Granular.new("grains", { density = 30, pitch = 12, jitter = 20, reverse = 30, mix = 20 }),
  Blocks.Stutter.new("stutter", { active = stutter, sync = "1/16" }),
  Blocks.Gate.new("gate", { threshold = -70, ratio = 3 }),
  Blocks.DeEsser.new("deess", { threshold = -30 }),
  Blocks.Limiter.new("limit", { ceiling = -0.5, lookahead = 1.5 }),
})

return Patch.new("Everything", { chain = chain, macros = { sweep, hold } })
