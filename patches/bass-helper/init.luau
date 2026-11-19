-- Bass Helper: transient shaping → FET → opto → two-band split
--   low:  fader-riding leveler
--   high: parallel drive + fizz filter
-- → boom / boxy / click EQ → output
--
-- Three "big" macros (Transients, Stabilize, Drive) each drive several params;
-- Boom / Boxy / Click / Output map 1:1 onto bipolar EQ gains / output level.
-- The crossover is a free param (not automatable, saved with the project).

local ui = require("@self/ui")

---------------------------------------------------------------- params
-- dynamics
local punch       = Param.new("punch",       "Punch",          { min = -9,  max = 9,  unit = "dB", decimals = 1 })
local fet_attack  = Param.new("fet_attack",  "FET attack",     { min = 0.1, max = 10, unit = "ms", decimals = 1, default = 1 })
local fet_thresh  = Param.new("fet_thresh",  "FET threshold",  { min = -40, max = 0,  unit = "dB", decimals = 1, default = -6 })
local fet_ratio   = Param.new("fet_ratio",   "FET ratio",      { min = 1,   max = 20, unit = ":1", decimals = 1, default = 4 })
local opto_thresh = Param.new("opto_thresh", "Opto threshold", { min = -40, max = 0,  unit = "dB", decimals = 1, default = -6 })
local opto_ratio  = Param.new("opto_ratio",  "Opto ratio",     { min = 1,   max = 10, unit = ":1", decimals = 1, default = 3 })
local ride_range  = Param.new("ride_range",  "Ride range",     { min = 0,   max = 12, unit = "dB", decimals = 1 })
local ride_target = Param.new("ride_target", "Ride target",    { min = -36, max = -6, unit = "dB", decimals = 0, default = -16 })
-- bands
local xover       = Param.new("xover",       "Crossover",      { min = 80,   max = 600,   unit = "Hz", decimals = 0, default = 250 })
local grit        = Param.new("grit",        "Grit",           { min = 0,    max = 30,    unit = "dB", decimals = 1 })
local grit_mix    = Param.new("grit_mix",    "Grit mix",       { min = 0,    max = 100,   unit = "%",  decimals = 0 })
local fizz        = Param.new("fizz",        "Fizz filter",    { min = 2000, max = 16000, unit = "Hz", decimals = 0, default = 12000 })
-- output
local output      = Param.new("output",      "Output",         { min = -24, max = 12, unit = "dB", decimals = 1, default = 0 })

---------------------------------------------------------------- macros
-- more attack: transient boost + let the FET's attack through
local transients = Macro.new("transients", "Transients", { default = 0.5 })
  :drives(punch,      { from = -9,  to = 9 })
  :drives(fet_attack, { from = 0.1, to = 8, curve = "exp" })

-- more even: both compressors work harder, the low band gets ridden
local stabilize = Macro.new("stabilize", "Stabilize", { default = 0.3 })
  :drives(fet_thresh,  { from = -4, to = -30 })
  :drives(fet_ratio,   { from = 2,  to = 8 })
  :drives(opto_thresh, { from = -4, to = -26 })
  :drives(opto_ratio,  { from = 2,  to = 6 })
  :drives(ride_range,  { from = 0,  to = 9 })

-- grit on the upper band only, darker fizz filter as it gets dirtier
local drive = Macro.new("drive", "Drive", { default = 0.2 })
  :drives(grit,     { from = 0,     to = 28 })
  :drives(grit_mix, { from = 0,     to = 85 })
  :drives(fizz,     { from = 12000, to = 3500, curve = "exp" })

---------------------------------------------------------------- chain
-- metered blocks get a name: the UI shows their meters
local shaper = Blocks.Transient.new("punch", { attack = punch })
local fet = Blocks.FETCompressor.new("fet", { threshold = fet_thresh, ratio = fet_ratio, attack = fet_attack, release = 90, auto_makeup = true })
local opto = Blocks.OptoCompressor.new("opto", { threshold = opto_thresh, ratio = opto_ratio, auto_makeup = true })
local rider = Blocks.Rider.new("rider", { target = ride_target, range = ride_range, speed = 250 })

local chain = Chain.new {
  shaper,
  fet,
  opto,
  Blocks.BandSplit.new("bands", {
    crossovers = { xover },
    bands = {
      Chain.new { rider },
      Chain.new {
        Blocks.Parallel.new("grit_bus", {
          mix = grit_mix,
          branches = { Chain.new(), Chain.new { Blocks.Drive.new("grit", { amount = grit }) } },
        }),
        Blocks.LowPass.new("fizz", { freq = fizz }),
      },
    },
  }),
}

-- bipolar band lifts/scoops (centre = flat), one macro each
local tone: { [string]: Macro } = {}
for _, band in {
  { id = "boom",  name = "Boom",  freq = 80,   q = 0.9 },
  { id = "boxy",  name = "Boxy",  freq = 450,  q = 1.3 },
  { id = "click", name = "Click", freq = 2800, q = 1.0 },
} do
  local id, name, freq, q = band.id, band.name, band.freq, band.q
  local g = Param.new(id, name, { min = -9, max = 9, unit = "dB", decimals = 1 })
  tone[id] = Macro.new(id, name):drives(g)
  chain:add(Blocks.PeakEq.new(id, { freq = freq, q = q, gain = g }))
end

chain:add(Blocks.Gain.new("out", { db = output }))

local output_macro = Macro.new("output", "Output"):drives(output)

return Patch.new("Bass Helper", {
  author = "sandbox",
  chain  = chain,
  -- order = host automation slots
  macros = { transients, stabilize, drive, tone.boom, tone.boxy, tone.click, output_macro },
  theme  = { colors = { accent = "#f2c14e", bg = "#101418" }, images = { logo = Image.new("logo.svg") } },
  panel  = ui.Panel.new({
    transients = transients, stabilize = stabilize, drive = drive,
    boom = tone.boom, boxy = tone.boxy, click = tone.click, output = output_macro,
    xover = xover, grit_mix = grit_mix,
    punch = shaper, fet = fet, opto = opto, rider = rider,
  }),
})
