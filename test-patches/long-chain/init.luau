-- Test patch: a long chain (snake layout), two macros on one knob (VCA style).
local drive = Blocks.Drive.new("drive", { amount = 6 })
local heat = Macro.new("heat", "Heat"):drives(drive.params.amount, { from = 6, to = 24 })
local dirt = Macro.new("dirt", "Dirt", { default = 0 }):drives(drive.params.amount, { from = 0, to = 12 })
local comp = Blocks.Compressor.new("comp", { threshold = -20, ratio = 3 })
local eq = Blocks.PeakEq.new("eq", { freq = 1500, gain = 2 })
local lp = Blocks.LowPass.new("lp", { freq = 12000 })
local echo = Blocks.Delay.new("echo", { time = 300 })
local fold = Blocks.Wavefolder.new("fold", { amount = 6 })
local verb = Blocks.Reverb.new("verb", { mix = 20 })
local chain = Chain.new {
  Blocks.HighPass.new("hp", { freq = 40 }),
  comp,
  drive,
  Blocks.Waveshaper.new("shape", { curve = function(x) return math.tanh(3 * x) / math.tanh(3) end, drive = 6 }),
  fold,
  eq,
  Blocks.Chorus.new("chorus", {}),
  echo,
  verb,
  lp,
  Blocks.Width.new("wide", { width = 120 }),
  Blocks.Limiter.new("limit", { ceiling = -0.3 }),
}
return Patch.new("Long Chain", { author = "test", chain = chain, macros = { heat, dirt,
  Macro.new("squash", "Squash"):drives(comp.params.threshold, { from = -10, to = -40 }),
  Macro.new("tone", "Tone"):drives(eq.params.gain, { from = -9, to = 9 }),
  Macro.new("dark", "Dark"):drives(lp.params.freq, { from = 16000, to = 800, curve = "exp" }),
  Macro.new("echo", "Echo"):drives(echo.params.feedback, { from = 10, to = 85 }),
  Macro.new("fold", "Fold"):drives(fold.params.amount, { from = 0, to = 24 }),
  Macro.new("space", "Space"):drives(verb.params.mix, { from = 0, to = 60 }) } })
