-- A knob that scales with its size: static body + moving value.
local style = require("./style")
local fmt = string.format
local PANEL, EDGE, ACCENT, TEXT, DIM = style.PANEL, style.EDGE, style.ACCENT, style.TEXT, style.DIM

local KnobBody = Component.define {
  name = "KnobBody",
  render = function(p: Props<{ label: string, big: boolean?, bipolar: boolean? }>)
    local w = p.width
    local cx, cy, r = w / 2, w * 0.46, w * 0.28
    local cap, dcap = SVG.radial { cx = 0.38, cy = 0.32, r = 0.8, stops = { { 0, EDGE }, { 1, PANEL } } }
    local parts = {
      dcap,
      SVG.arc { cx = cx, cy = cy, r = r * 1.32, from = -135, to = 135, stroke = "#ffffff", width = w * 0.03, opacity = 0.07 },
      SVG.circle { cx = cx + 1, cy = cy + 2, r = r, fill = "#000000", opacity = 0.4 },
      SVG.circle { cx = cx, cy = cy, r = r, fill = fmt("url(#%s)", cap), stroke = "#07090b", width = 1 },
      SVG.text { x = cx, y = p.height - 22, text = p.label, size = p.big and 12 or 10, weight = "bold", spacing = 1.5, anchor = "middle", fill = TEXT },
    }
    if p.bipolar then
      parts[#parts + 1] = SVG.line { x1 = cx, y1 = cy - r * 1.48, x2 = cx, y2 = cy - r * 1.2, stroke = DIM, width = 1.5 }
    end
    return parts
  end,
}

local KnobValue = Component.define {
  name = "KnobValue",
  render = function(p: Props<{ v: number?, text: string?, color: string, big: boolean?, bipolar: boolean?, active: boolean }>)
    local w = p.width
    local cx, cy, r = w / 2, w * 0.46, w * 0.28
    local v = p.v or 0
    local deg = -135 + 270 * v
    local from = p.bipolar and 0 or -135
    return {
      SVG.arc { cx = cx, cy = cy, r = r * 1.32, from = from, to = deg, stroke = p.color, width = w * 0.03, glow = p.active and 3 or nil },
      SVG.line { x1 = cx + r * 0.35 * math.sin(math.rad(deg)), y1 = cy - r * 0.35 * math.cos(math.rad(deg)),
                 x2 = cx + r * 0.85 * math.sin(math.rad(deg)), y2 = cy - r * 0.85 * math.cos(math.rad(deg)),
                 stroke = TEXT, width = p.big and 3.5 or 2.5 },
      SVG.text { x = cx, y = p.height - 6, text = p.text or "", size = p.big and 11 or 9, anchor = "middle", fill = p.active and p.color or DIM },
    }
  end,
}

local Knob = Component.define {
  name = "Knob",
  kind = "knob",
  render = function(p: Props<{ label: string, color: string?, big: boolean?, bipolar: boolean? }>)
    local color = p.color or ACCENT
    return {
      KnobBody.new { label = p.label, big = p.big, bipolar = p.bipolar },
      KnobValue.new { v = p.value, text = p.display, color = color, big = p.big, bipolar = p.bipolar, active = p.dragging or p.hover },
    }
  end,
}

return Knob
