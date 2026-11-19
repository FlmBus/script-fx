-- Neon Saturator: the whole control panel as reactive components.
-- Each component's render(p) returns SVG; the shell caches one image per
-- component and re-renders only when that component's inputs change.

local Background = require("@self/background")
local Logo = require("@self/logo")
local Pulse = require("@self/pulse")
local NeonKnob = require("@self/knob")
local Meter = require("@self/meter")
local Toggle = require("@self/toggle")
local TabBar = require("@self/tabs")
local Spectrum = require("@self/spectrum")

-- The patch's init.luau passes the four macros in.
export type PanelProps = { tape: Macro, tube: Macro, diode: Macro, soft_clip: Macro }

local Panel = Component.define {
  name = "Panel",
  render = function(p: Props<PanelProps>)
    local tab = p.state.tab or 1
    local items = {
      Background.new {},
      Logo.new { x = 220, y = 104, w = 400, h = 170 },
      Pulse.new { x = 652, y = 210 },
      Meter.new { source = Meters.IN, label = "IN", color = "#3fd7ff", x = 26, y = 96, w = 62, h = 440 },
      Meter.new { source = Meters.OUT, label = "OUT", color = "#ffc14d", x = 752, y = 96, w = 62, h = 440 },
      TabBar.new { tabs = { "MAIN", "ADVANCED" }, group = p.state, x = 330, y = 290, w = 180, h = 28 },
      Toggle.new { key = "os", label = "OVERSAMPLE", on_text = "4X", default_on = true, color = "#3fd7ff", x = 270, y = 496 },
      Toggle.new { key = "ag", label = "AUTO-GAIN", default_on = true, color = "#ffc14d", x = 430, y = 496 },
    }
    if tab == 1 then
      for _, k in {
        { label = "TAPE",      macro = p.tape,      color = "#ffb43d", x = 110 },
        { label = "TUBE",      macro = p.tube,      color = "#ff4fb0", x = 230 },
        { label = "DIODE",     macro = p.diode,     color = "#e05bff", x = 500 },
        { label = "SOFT-CLIP", macro = p.soft_clip, color = "#3fd7ff", x = 620 },
      } do
        items[#items + 1] = NeonKnob.new { key = k.label, label = k.label, bind = k.macro, color = k.color, x = k.x, y = 336 }
      end
    else
      items[#items + 1] = Spectrum.new { series = "spectrum", x = 120, y = 336, w = 600, h = 140 }
    end
    return items
  end,
}

return { Panel = Panel }
