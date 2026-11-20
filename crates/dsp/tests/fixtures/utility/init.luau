local wide = Blocks.Width.new("wide", { width = 100 })
local spread = Macro.new("spread", "Spread"):drives(wide.params.width)
local level = Blocks.Meter.new("level")

local chain = Chain.new({
  Blocks.DCBlock.new("dc"),
  Blocks.Invert.new("flip", { channels = "left" }),
  Blocks.MidSide.new("ms", {
    oversampling = 2,
    mid = Chain.new({ Blocks.Drive.new("warm", { amount = 6 }) }),
    side = Chain.new({ Blocks.HighShelf.new("air", { freq = 8000, gain = 3 }) }),
  }),
  Blocks.LeftRight.new("lr", { right = Chain.new({ Blocks.Haas.new("haas", { delay = 12, channel = "right" }) }) }),
  wide,
  Blocks.MonoBass.new("mono", { freq = 120 }),
  Blocks.Pan.new("pan", { pan = -10 }),
  Blocks.Parallel.new("par", { mix = 30, branches = {
    Chain.new(),
    Chain.new({ Blocks.Drive.new("grit", { amount = 18, oversampling = 4 }) }),
  } }),
  level,
})

return Patch.new("Utility Check", { chain = chain, macros = { spread } })
