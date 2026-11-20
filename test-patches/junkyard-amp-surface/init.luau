-- Junkyard Amp: a one-channel amp with three knobs. UI in ui/ (one file per component).


local preamp   = Blocks.Drive.new("preamp", { amount = 14, mix = 100 })
local presence = Blocks.HighShelf.new("presence", { freq = 3500, q = 0.7, gain = 2 })
local master   = Blocks.Gain.new("master", { db = -6 })

local chain = Chain.new {
  Blocks.HighPass.new("tight", { freq = 90, q = 0.7 }),
  preamp,
  presence,
  Blocks.LowPass.new("cab", { freq = 5500, q = 0.8 }),
  master,
}

local macros = {
  gain = Macro.new("gain", "Gain"):drives(preamp.params.amount, { from = 0, to = 36 }),
  presence = Macro.new("presence", "Presence"):drives(presence.params.gain, { from = -6, to = 9 }),
  master = Macro.new("master", "Master"):drives(master.params.db, { from = -24, to = 6 }),
}

return Patch.new("Junkyard Amp (no panel)", {
  author = "sandbox",
  chain = chain,
  macros = { macros.gain, macros.presence, macros.master },
  theme = { colors = { accent = "#d9472b", bg = "#4a2e1a" }, fonts = { title = Font.new("fonts/label-tape.fnt") } },
})
