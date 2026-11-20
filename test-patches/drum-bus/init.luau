-- Test patch: the mockup's drum bus (band split with a parallel inside), no panel.
local glue = Blocks.Compressor.new("glue", { threshold = -18, ratio = 2 })
local grit = Blocks.Drive.new("grit", { amount = 4 })
local punch = Blocks.Transient.new("punch", { attack = 3 })
local wide = Blocks.Width.new("wide", { width = 120 })
local chain = Chain.new {
  Blocks.BandSplit.new("split", {
    crossovers = { 200, 3000 },
    bands = {
      Chain.new { glue, grit },
      Chain.new { Blocks.Parallel.new("mid_split", { mix = 50, branches = { Chain.new(), Chain.new {
        Blocks.Delay.new("echo", { time = 250, feedback = 30, mix = 100 }),
        Blocks.PeakEq.new("sweep", { freq = 1200, gain = 3 }) } } }) },
      Chain.new { punch, Blocks.Reverb.new("verb", { mix = 20 }), Blocks.LowPass.new("tail", { freq = 9000 }) },
    },
  }),
  wide,
  Blocks.Limiter.new("limit", { ceiling = -0.3 }),
}
return Patch.new("Drum Bus", { author = "test", chain = chain, macros = {
  Macro.new("punch", "Punch"):drives(punch.params.attack, { from = 0, to = 12 }),
  Macro.new("grit", "Grit"):drives(grit.params.amount, { from = 0, to = 18 }),
  Macro.new("width", "Width"):drives(wide.params.width, { from = 100, to = 200 }),
  Macro.new("glue", "Glue"):drives(glue.params.threshold, { from = -6, to = -30 }) } })
