-- No-alloc fixture: the versatile compressor (every preset, lookahead), lookahead on the other
-- dynamics blocks, and linear-phase EQ, filter and band split with a macro moving the EQ (so the
-- FIRs are redesigned on the audio thread while it runs).
local eq = Blocks.PeakEq.new("eq", { freq = 1000, gain = 6, phase = "linear" })
local hp = Blocks.HighPass.new("hp", { freq = 80, slope = 24, phase = "linear" })
local chain = Chain.new {
  Blocks.Compressor.new("c", { threshold = -30, lookahead = 3, feedback = 50, saturation = 20, link = 50 }),
  Blocks.FETCompressor.new("fet", {}),
  Blocks.OptoCompressor.new("opto", {}),
  Blocks.VariMuCompressor.new("vm", { lookahead = 1 }),
  Blocks.Gate.new("g", { lookahead = 2 }),
  Blocks.DeEsser.new("d", { lookahead = 2 }),
  Blocks.Transient.new("t", { attack = 6, lookahead = 2 }),
  eq,
  hp,
  Blocks.BandSplit.new("s", { crossovers = { 200, 2000 }, phase = "linear",
    bands = { Chain.new(), Chain.new { Blocks.Gain.new("mid", { db = -3 }) }, Chain.new() } }),
}
local m = Macro.new("m", "M"):drives(eq.params.freq, { from = 200, to = 5000, curve = "exp" })
return Patch.new("Dynamics + linear phase", { chain = chain, macros = { m } })
