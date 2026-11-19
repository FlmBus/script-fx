-- The static background: frame, title, the signal-flow strip with its boxes and arrows.
local style = require("./style")
local fmt = string.format
local BG, BG2, PANEL, EDGE, ACCENT, TEXT, DIM, STRIP, BOXES = style.BG, style.BG2, style.PANEL, style.EDGE, style.ACCENT, style.TEXT, style.DIM, style.STRIP, style.BOXES

type Box = style.Box

local function box(b: Box, active: boolean?): Part
  return {
    SVG.rect { x = b.x, y = b.y, w = b.w, h = b.h, rx = 6, fill = PANEL, stroke = active and ACCENT or EDGE, width = 1 },
    SVG.text { x = b.x + 8, y = b.y + 15, text = b.label, size = 9, weight = "bold", spacing = 1.5, fill = DIM },
  }
end

local function arrow(x1: number, y1: number, x2: number, y2: number): Part
  return {
    SVG.line { x1 = x1, y1 = y1, x2 = x2 - 4, y2 = y2, stroke = EDGE, width = 1.5 },
    SVG.path { d = fmt("M %d %d L %d %d L %d %d Z", x2, y2, x2 - 6, y2 - 4, x2 - 6, y2 + 4), fill = EDGE },
  }
end

local Background = Component.define {
  name = "Background",
  render = function(p: Props<{}>)
    local w, h = p.width, p.height
    local bg, dbg = SVG.linear { stops = { { 0, BG2 }, { 1, BG } } }
    local cy = STRIP.y + STRIP.h / 2
    local parts = {
      dbg,
      SVG.rect { x = 0, y = 0, w = w, h = h, fill = fmt("url(#%s)", bg) },
      SVG.rect { x = 16, y = 16, w = w - 32, h = h - 32, rx = 16, stroke = EDGE, width = 1.5 },
      SVG.text { x = 48, y = 62, text = "BASS HELPER", size = 24, weight = "bold", spacing = 5, fill = TEXT },
      SVG.text { x = 50, y = 78, text = "PUNCH · CONTROL · GRIT", size = 9, spacing = 2.5, fill = DIM },
      SVG.rect { x = 40, y = STRIP.y - 4, w = w - 80, h = STRIP.h + 4, rx = 10, fill = "#0b0e11", stroke = EDGE, width = 1 },
    }
    for _, k in ipairs({ "punch", "fet", "opto", "low", "high", "eq", "out" }) do
      parts[#parts + 1] = box(BOXES[k])
    end
    -- arrows: punch → fet → opto → split → (low|high) → eq → out
    parts[#parts + 1] = arrow(146, cy, 166, cy)
    parts[#parts + 1] = arrow(270, cy, 290, cy)
    parts[#parts + 1] = SVG.line { x1 = 394, y1 = cy, x2 = 412, y2 = cy, stroke = EDGE, width = 1.5 }
    parts[#parts + 1] = SVG.path { d = "M 412 124 L 412 172", stroke = EDGE, width = 1.5 }
    parts[#parts + 1] = arrow(412, 124, 432, 124)
    parts[#parts + 1] = arrow(412, 172, 432, 172)
    parts[#parts + 1] = SVG.path { d = "M 628 124 L 646 124 L 646 172 L 628 172", stroke = EDGE, width = 1.5 }
    parts[#parts + 1] = arrow(646, cy, 666, cy)
    parts[#parts + 1] = arrow(722, cy, 742, cy)
    -- section labels
    parts[#parts + 1] = SVG.text { x = 420, y = 224, text = "MACROS", size = 9, spacing = 3, anchor = "middle", fill = DIM }
    parts[#parts + 1] = SVG.line { x1 = 110, y1 = 404, x2 = 730, y2 = 404, stroke = EDGE, width = 1, opacity = 0.6 }
    return parts
  end,
}

return Background
