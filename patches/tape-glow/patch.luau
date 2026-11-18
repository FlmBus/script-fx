-- Tape Glow: one "Character" macro drives drive + tone at once; mix and output
-- are 1:1 macros. UI in ui.luau (reels, VU meter, glow).

local ui = require("./ui")

local drive  = Param.new("drive",  "Drive",  { min = 0,   max = 24,    unit = "dB", decimals = 1, default = 6 })
local tone   = Param.new("tone",   "Tone",   { min = 500, max = 20000, unit = "Hz", decimals = 0, default = 8000 })
local mix    = Param.new("mix",    "Mix",    { min = 0,   max = 100,   unit = "%",  decimals = 0, default = 100 })
local output = Param.new("output", "Output", { min = -24, max = 12,    unit = "dB", decimals = 1, default = 0 })

-- darker as it gets dirtier
local character = Macro.new("character", "Character", { default = 0.3 })
  :drives(drive, { from = 0,     to = 18 })
  :drives(tone,  { from = 16000, to = 2500, curve = "exp" })

local chain = Chain.new {
  Blocks.Drive.new("tape",    { amount = drive, mix = mix }),
  Blocks.LowPass.new("tone",  { freq = tone, q = 0.707 }),
  Blocks.Gain.new("output",   { db = output }),
}

local mix_macro = Macro.new("mix", "Mix"):drives(mix)
local output_macro = Macro.new("output", "Output"):drives(output)

return Patch.new("Tape Glow", {
  author = "sandbox",
  chain = chain,
  macros = { character, mix_macro, output_macro },
  theme = { accent = "#ff8a3d", bg = "#17141c" },
  panel = ui.Panel.new({ character = character, mix = mix_macro, output = output_macro, drive = drive, tone = tone }),
})
