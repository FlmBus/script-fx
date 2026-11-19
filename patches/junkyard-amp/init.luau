-- Junkyard Amp: a one-channel amp with three knobs. UI in ui/ (one file per component).

local ui = require("@self/ui")

local gain     = Param.new("gain",     "Gain",     { min = 0,   max = 36, unit = "dB", decimals = 1, default = 14 })
local presence = Param.new("presence", "Presence", { min = -6,  max = 9,  unit = "dB", decimals = 1, default = 2 })
local master   = Param.new("master",   "Master",   { min = -24, max = 6,  unit = "dB", decimals = 1, default = -6 })

local chain = Chain.new {
  Blocks.HighPass.new("tight",    { freq = 90, q = 0.7 }),
  Blocks.Drive.new("preamp",      { amount = gain, mix = 100 }),
  Blocks.HighShelf.new("presence", { freq = 3500, q = 0.7, gain = presence }),
  Blocks.LowPass.new("cab",       { freq = 5500, q = 0.8 }),
  Blocks.Gain.new("master",       { db = master }),
}

local macros = {
  gain = Macro.new("gain", "Gain"):drives(gain),
  presence = Macro.new("presence", "Presence"):drives(presence),
  master = Macro.new("master", "Master"):drives(master),
}

return Patch.new("Junkyard Amp", {
  author = "sandbox",
  chain = chain,
  macros = { macros.gain, macros.presence, macros.master },
  theme = { colors = { accent = "#d9472b", bg = "#4a2e1a" }, fonts = { title = Font.new("fonts/label-tape.fnt") } },
  panel = ui.Panel.new(macros),
})
