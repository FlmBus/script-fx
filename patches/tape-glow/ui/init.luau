-- Tape Glow: a tape deck. Reels spin faster with more drive, the VU needle
-- and the glow at the tape head follow the output level.
-- The patch's init.luau passes the macros and params in (see SurfaceProps).

local style = require("@self/style")
local Background = require("@self/background")
local Reel = require("@self/reel")
local Vu = require("@self/vu")
local HeadGlow = require("@self/head_glow")
local Knob = require("@self/knob")
local Readout = require("@self/readout")
local REELS, REEL_R, VU = style.REELS, style.REEL_R, style.VU

export type SurfaceProps = { character: Macro, mix: Macro, output: Macro, drive: Param, tone: Param }

local Surface = Component.define {
  name = "Surface",
  render = function(p: Props<SurfaceProps>)
    local items = {
      Background.new {},
      HeadGlow.new { source = Meters.OUT, x = 360, y = 240, w = 120, h = 110 },
    }
    for i, c in ipairs(REELS) do
      items[#items + 1] = Reel.new { key = "reel" .. i, bind = p.drive, phase = i * 37,
                                     x = c[1] - REEL_R - 4, y = c[2] - REEL_R - 4 }
    end
    items[#items + 1] = Vu.new { source = Meters.OUT, x = VU.x, y = VU.y, w = VU.w, h = VU.h }
    items[#items + 1] = Knob.new { key = "mix", label = "MIX", bind = p.mix, x = 70, y = 362 }
    items[#items + 1] = Readout.new { key = "drive", label = "DRIVE", bind = p.drive, x = 210, y = 392 }
    items[#items + 1] = Knob.new { key = "character", label = "CHARACTER", bind = p.character, x = 360, y = 362 }
    items[#items + 1] = Readout.new { key = "tone", label = "TONE", bind = p.tone, x = 510, y = 392 }
    items[#items + 1] = Knob.new { key = "output", label = "OUTPUT", bind = p.output, x = 650, y = 362 }
    return items
  end,
}

return { Surface = Surface }
