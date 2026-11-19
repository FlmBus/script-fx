-- Clean Gain: one big fader between an input and an output meter.
-- The patch's init.luau passes the gain macro in: ui.Panel.new({ gain = gain })

local Background = require("@self/background")
local Fader = require("@self/fader")
local Meter = require("@self/meter")

local Panel = Component.define {
  name = "Panel",
  render = function(p: Props<{ gain: Macro }>)
    return {
      Background.new {},
      Meter.new { key = "in", label = "IN", source = Meters.IN, x = 250, y = 130 },
      Fader.new { bind = p.gain, x = 350, y = 120 },
      Meter.new { key = "out", label = "OUT", source = Meters.OUT, x = 540, y = 130 },
    }
  end,
}

return { Panel = Panel }
