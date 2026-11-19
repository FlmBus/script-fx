-- Junkyard Amp: skeuomorphic, grungy, improvised. Everything is generated
-- procedurally (seeded randomness, so every render is identical).

local Wood = require("@self/wood")
local Logo = require("@self/logo")
local Lamp = require("@self/lamp")
local ChickenKnob = require("@self/chicken_knob")

-- The patch's init.luau passes the three macros in.
export type PanelProps = { gain: Macro, presence: Macro, master: Macro }

local Panel = Component.define {
  name = "Panel",
  render = function(p: Props<PanelProps>)
    return {
      Wood.new {},
      Logo.new { x = 30, y = 26 },
      Lamp.new { source = Meters.OUT, x = 718, y = 40 },
      ChickenKnob.new { key = "gain",     label = "GAIN",     bind = p.gain,     seed = 11, tilt = -3, x = 130, y = 230 },
      ChickenKnob.new { key = "presence", label = "PRESENCE", bind = p.presence, seed = 23, tilt = 2,  x = 330, y = 230 },
      ChickenKnob.new { key = "master",   label = "MASTER",   bind = p.master,   seed = 37, tilt = -1, x = 530, y = 230 },
    }
  end,
}

return { Panel = Panel }
