-- Neon Saturator: four saturation stages in series. The control panel is the
-- component UI in ui/ (one file per component).

local ui = require("@self/ui")

local tape      = Param.new("tape",      "Tape",      { min = 0, max = 18, unit = "dB", decimals = 1, default = 9 })
local tape_tone = Param.new("tape_tone", "Tape tone", { min = 3000, max = 18000, unit = "Hz", decimals = 0, default = 12000 })
local tube      = Param.new("tube",      "Tube",      { min = 0, max = 24, unit = "dB", decimals = 1, default = 6 })
local diode     = Param.new("diode",     "Diode",     { min = 0, max = 30, unit = "dB", decimals = 1, default = 4 })
local soft_clip = Param.new("soft_clip", "Soft clip", { min = 0, max = 20, unit = "dB", decimals = 1, default = 4 })

local chain = Chain.new {
  Blocks.Drive.new("tape",      { amount = tape, mix = 100 }),
  Blocks.LowPass.new("tape_tone", { freq = tape_tone, q = 0.6 }),
  Blocks.Drive.new("tube",      { amount = tube, mix = 60 }),
  Blocks.Drive.new("diode",     { amount = diode, mix = 40 }),
  Blocks.Drive.new("soft_clip", { amount = soft_clip, mix = 100 }),
  Blocks.Gain.new("trim",       { db = -3 }),
}

local macros = {
  tape = Macro.new("tape", "Tape")
    :drives(tape)
    :drives(tape_tone, { from = 16000, to = 6000, curve = "exp" }),   -- more tape = darker
  tube = Macro.new("tube", "Tube"):drives(tube),
  diode = Macro.new("diode", "Diode"):drives(diode),
  soft_clip = Macro.new("soft_clip", "Soft clip"):drives(soft_clip),
}

return Patch.new("Neon Saturator", {
  author = "sandbox",
  chain = chain,
  macros = { macros.tape, macros.tube, macros.diode, macros.soft_clip },
  theme = { colors = { accent = "#ff6fbf", bg = "#0d0820" }, fonts = { title = Font.new("fonts/Orbitron-ExtraBold.ttf") } },
  panel = ui.Panel.new(macros),
})
