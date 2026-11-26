-- Tape Glow: one "Character" macro drives drive + tone at once; mix and output
-- are 1:1 macros. UI in ui/ (reels, VU meter, glow).

local ui = require("@self/ui")

local tape   = Blocks.Drive.new("tape", { amount = 6, mix = 100 })
local tone   = Blocks.LowPass.new("tone", { freq = 8000, q = 0.707 })
local output = Blocks.Gain.new("output", { db = 0 })
local chain = Chain.new { tape, tone, output }

-- darker as it gets dirtier
local character = Macro.new("character", "Character", { default = 0.3 })
  :drives(tape.params.amount, { from = 0,     to = 18 })
  :drives(tone.params.freq,   { from = 16000, to = 2500, curve = "exp" })

local mix_macro = Macro.new("mix", "Mix"):drives(tape.params.mix)
local output_macro = Macro.new("output", "Output"):drives(output.params.db, { from = -24, to = 12 })

return Patch.new("Tape Glow", {
  author = "sandbox",
  chain = chain,
  macros = { character, mix_macro, output_macro },
  theme = { colors = { accent = "#ff8a3d", bg = "#17141c" }, fonts = { title = Font.new("fonts/PlayfairDisplay-SemiBoldItalic.ttf") } },
  surface = ui.Surface.new({ character = character, mix = mix_macro, output = output_macro, drive = tape.params.amount, tone = tone.params.freq }),
})
