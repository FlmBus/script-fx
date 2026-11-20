-- Clean Gain: the smallest possible patch. UI in ui/ (a fader + meters).

local ui = require("@self/ui")

local out = Blocks.Gain.new("out", { db = 0 })
local gain = Macro.new("gain", "Gain"):drives(out.params.db, { from = -24, to = 12 })

return Patch.new("Clean Gain", {
  author = "sandbox",
  chain = Chain.new({ out }),
  macros = { gain },
  theme = { colors = { accent = "#4fd1c5", bg = "#0f1a1d" }, fonts = { title = Font.new("fonts/clean-line.svg") } },
  panel = ui.Panel.new({ gain = gain }),
})
