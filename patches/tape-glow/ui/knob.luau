-- A knob: static body + moving value arc, pointer and readout.
local style = require("./style")
local fmt = string.format
local ACCENT, TEXT, DIM = style.ACCENT, style.TEXT, style.DIM

local KnobBody = Component.define {
  name = "KnobBody",
  render = function(p: Props<{ label: string }>)
    local cap, dcap = SVG.radial { cx = 0.38, cy = 0.32, r = 0.75, stops = { { 0, "#4e4456" }, { 1, "#1d1821" } } }
    return {
      dcap,
      SVG.arc { cx = 60, cy = 58, r = 46, from = -135, to = 135, stroke = "#ffffff", width = 4, opacity = 0.08 },
      SVG.circle { cx = 61, cy = 61, r = 34, fill = "#000000", opacity = 0.35 },
      SVG.circle { cx = 60, cy = 58, r = 34, fill = fmt("url(#%s)", cap), stroke = "#0b090d", width = 1 },
      SVG.text { x = 60, y = 128, text = p.label, size = 11, weight = "bold", spacing = 2, anchor = "middle", fill = TEXT },
    }
  end,
}

local KnobValue = Component.define {
  name = "KnobValue",
  render = function(p: Props<{ v: number?, text: string?, active: boolean }>)
    local v = p.v or 0
    local deg = -135 + 270 * v
    return {
      SVG.arc { cx = 60, cy = 58, r = 46, from = -135, to = deg, stroke = ACCENT, width = 4, glow = p.active and 4 or nil },
      SVG.rect { x = 58, y = 28, w = 4, h = 16, rx = 2, fill = TEXT, rotate = { deg, 60, 58 } },
      SVG.text { x = 60, y = 148, text = p.text or "", size = 10, anchor = "middle", fill = p.active and ACCENT or DIM },
    }
  end,
}

local Knob = Component.define {
  name = "Knob",
  kind = "knob",
  size = { 120, 160 },
  render = function(p: Props<{ label: string }>)
    return {
      KnobBody.new { label = p.label },
      KnobValue.new { v = p.value, text = p.display, active = p.dragging or p.hover },
    }
  end,
}

return Knob
