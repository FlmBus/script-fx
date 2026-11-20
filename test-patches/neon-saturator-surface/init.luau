-- Neon Saturator: four saturation stages in series. The control panel is the
-- component UI in ui/ (one file per component).


local tape      = Blocks.Drive.new("tape", { amount = 9, mix = 100 })
local tape_tone = Blocks.LowPass.new("tape_tone", { freq = 12000, q = 0.6 })
local tube      = Blocks.Drive.new("tube", { amount = 6, mix = 60 })
local diode     = Blocks.Drive.new("diode", { amount = 4, mix = 40 })
local soft_clip = Blocks.Drive.new("soft_clip", { amount = 4, mix = 100 })

local chain = Chain.new { tape, tape_tone, tube, diode, soft_clip, Blocks.Gain.new("trim", { db = -3 }) }

local macros = {
  tape = Macro.new("tape", "Tape")
    :drives(tape.params.amount, { from = 0, to = 18 })
    :drives(tape_tone.params.freq, { from = 16000, to = 6000, curve = "exp" }),   -- more tape = darker
  tube = Macro.new("tube", "Tube"):drives(tube.params.amount, { from = 0, to = 24 }),
  diode = Macro.new("diode", "Diode"):drives(diode.params.amount, { from = 0, to = 30 }),
  soft_clip = Macro.new("soft_clip", "Soft clip"):drives(soft_clip.params.amount, { from = 0, to = 20 }),
}

return Patch.new("Neon Saturator (no panel)", {
  author = "sandbox",
  chain = chain,
  macros = { macros.tape, macros.tube, macros.diode, macros.soft_clip },
  theme = { colors = { accent = "#ff6fbf", bg = "#0d0820" }, fonts = { title = Font.new("fonts/Orbitron-ExtraBold.ttf") } },
})
