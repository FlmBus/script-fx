-- Junkyard Amp: a one-channel amp with three knobs. UI in ui.lua.

return {
  name = "Junkyard Amp",
  author = "sandbox",

  params = {
    { id = "gain",     name = "Gain",     min = 0,   max = 36, default = 14, unit = " dB", decimals = 1 },
    { id = "presence", name = "Presence", min = -6,  max = 9,  default = 2,  unit = " dB", decimals = 1 },
    { id = "master",   name = "Master",   min = -24, max = 6,  default = -6, unit = " dB", decimals = 1 },
  },

  macros = {
    { id = "gain",     name = "Gain",     targets = { { param = "gain" } } },
    { id = "presence", name = "Presence", targets = { { param = "presence" } } },
    { id = "master",   name = "Master",   targets = { { param = "master" } } },
  },

  chain = {
    eq("tight",    { type = "highpass", freq = 90, q = 0.7 }),
    drive("preamp", { amount = "param.gain", mix = 100 }),
    eq("presence", { type = "highshelf", freq = 3500, q = 0.7, gain = "param.presence" }),
    eq("cab",      { type = "lowpass", freq = 5500, q = 0.8 }),
    gain("master", { db = "param.master" }),
  },

  theme = { accent = "#d9472b", bg = "#4a2e1a" },
}
