-- Clean Gain: the smallest possible patch. UI in ui.luau (a fader + meters).

local ui = require("./ui")

local gain_db = Param.new("gain", "Gain", { min = -24, max = 12, unit = "dB", decimals = 1, default = 0 })
local gain = Macro.new("gain", "Gain"):drives(gain_db)

return Patch.new("Clean Gain", {
  author = "sandbox",
  chain = Chain.new({ Blocks.Gain.new("out", { db = gain_db }) }),
  macros = { gain },
  theme = { accent = "#4fd1c5", bg = "#0f1a1d" },
  panel = ui.Panel.new({ gain = gain }),
})
