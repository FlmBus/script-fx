-- Bass Helper: transient shaping → FET → opto → two-band split
--   low:  fader-riding leveler
--   high: parallel drive + fizz filter
-- → boom / boxy / click EQ → output
--
-- Three "big" macros (Transients, Stabilize, Drive) each drive several params;
-- Boom / Boxy / Click / Output map 1:1 onto bipolar EQ gains / output level.
-- The crossover is a free knob (not automatable, saved with the project).


---------------------------------------------------------------- chain
-- Every block setting is a knob (block.params.<name>); the numbers here are start values.
-- metered blocks get a name: the UI shows their meters
local shaper = Blocks.Transient.new("punch", { attack = 0 })
local fet = Blocks.FETCompressor.new("fet", { threshold = -6, ratio = 4, attack = 1, release = 90, auto_makeup = true })
local opto = Blocks.OptoCompressor.new("opto", { threshold = -6, ratio = 3, auto_makeup = true })
local rider = Blocks.Rider.new("rider", { target = -16, range = 0, speed = 250 })
local grit = Blocks.Drive.new("grit", { amount = 0 })
local grit_bus = Blocks.Parallel.new("grit_bus", { mix = 0, branches = { Chain.new(), Chain.new { grit } } })
local fizz = Blocks.LowPass.new("fizz", { freq = 12000 })
-- the crossover is a free knob (not automatable, saved with the project)
local bands = Blocks.BandSplit.new("bands", {
  crossovers = { 250 },
  bands = { Chain.new { rider }, Chain.new { grit_bus, fizz } },
})
bands.params.crossovers[1]:range(80, 600)

local chain = Chain.new { shaper, fet, opto, bands }

-- bipolar band lifts/scoops (centre = flat), one macro each
local tone: { [string]: Macro } = {}
for _, band in {
  { id = "boom",  name = "Boom",  freq = 80,   q = 0.9 },
  { id = "boxy",  name = "Boxy",  freq = 450,  q = 1.3 },
  { id = "click", name = "Click", freq = 2800, q = 1.0 },
} do
  local eq = Blocks.PeakEq.new(band.id, { freq = band.freq, q = band.q, gain = 0 })
  tone[band.id] = Macro.new(band.id, band.name):drives(eq.params.gain, { from = -9, to = 9 })
  chain:add(eq)
end

local out = Blocks.Gain.new("out", { db = 0 })
chain:add(out)

---------------------------------------------------------------- macros
-- more attack: transient boost + let the FET's attack through
local transients = Macro.new("transients", "Transients", { default = 0.5 })
  :drives(shaper.params.attack, { from = -9,  to = 9 })
  :drives(fet.params.attack,    { from = 0.1, to = 8, curve = "exp" })

-- more even: both compressors work harder, the low band gets ridden
local stabilize = Macro.new("stabilize", "Stabilize", { default = 0.3 })
  :drives(fet.params.threshold,  { from = -4, to = -30 })
  :drives(fet.params.ratio,      { from = 2,  to = 8 })
  :drives(opto.params.threshold, { from = -4, to = -26 })
  :drives(opto.params.ratio,     { from = 2,  to = 6 })
  :drives(rider.params.range,    { from = 0,  to = 9 })

-- grit on the upper band only, darker fizz filter as it gets dirtier
local drive = Macro.new("drive", "Drive", { default = 0.2 })
  :drives(grit.params.amount,  { from = 0,     to = 28 })
  :drives(grit_bus.params.mix, { from = 0,     to = 85 })
  :drives(fizz.params.freq,    { from = 12000, to = 3500, curve = "exp" })

local output_macro = Macro.new("output", "Output"):drives(out.params.db, { from = -24, to = 12 })

return Patch.new("Bass Helper (no panel)", {
  author = "sandbox",
  chain  = chain,
  -- order = host automation slots
  macros = { transients, stabilize, drive, tone.boom, tone.boxy, tone.click, output_macro },
  theme  = { colors = { accent = "#f2c14e", bg = "#101418" }, images = { logo = Image.new("logo.svg") } },
})
