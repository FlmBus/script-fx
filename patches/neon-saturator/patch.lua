-- Neon Saturator: four saturation stages in series. The control panel is the
-- component UI in ui.lua (no SVG control_panel here).

return {
  name = "Neon Saturator",
  author = "sandbox",

  params = {
    { id = "tape",      name = "Tape",      min = 0, max = 18, default = 9,   unit = " dB", decimals = 1 },
    { id = "tube",      name = "Tube",      min = 0, max = 24, default = 6,   unit = " dB", decimals = 1 },
    { id = "diode",     name = "Diode",     min = 0, max = 30, default = 4,   unit = " dB", decimals = 1 },
    { id = "soft_clip", name = "Soft clip", min = 0, max = 20, default = 4,   unit = " dB", decimals = 1 },
    { id = "tape_tone", name = "Tape tone", min = 3000, max = 18000, default = 12000, unit = " Hz", decimals = 0 },
  },

  macros = {
    { id = "tape", name = "Tape", targets = {
        { param = "tape" },
        { param = "tape_tone", from = 16000, to = 6000, curve = "exp" },   -- more tape = darker
    } },
    { id = "tube",      name = "Tube",      targets = { { param = "tube" } } },
    { id = "diode",     name = "Diode",     targets = { { param = "diode" } } },
    { id = "soft_clip", name = "Soft clip", targets = { { param = "soft_clip" } } },
  },

  chain = {
    drive("tape",      { amount = "param.tape", mix = 100 }),
    eq("tape_tone",    { type = "lowpass", freq = "param.tape_tone", q = 0.6 }),
    drive("tube",      { amount = "param.tube", mix = 60 }),
    drive("diode",     { amount = "param.diode", mix = 40 }),
    drive("soft_clip", { amount = "param.soft_clip", mix = 100 }),
    gain("trim",       { db = -3 }),
  },

  theme = { accent = "#ff6fbf", bg = "#0d0820" },
}
