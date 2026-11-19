-- A neon knob. Split into a static body (rendered once) and the moving parts (re-rendered
-- on every value change). The knob itself only composes the two.
local fmt = string.format

local KnobBody = Component.define {
  name = "KnobBody",
  render = function(p: Props<{ label: string? }>)
    local body, dbody = SVG.radial { cx = 0.35, cy = 0.3, r = 0.75, stops = { { 0, "#4a3f62" }, { 1, "#1b1426" } } }
    return {
      dbody,
      SVG.arc { cx = 55, cy = 52, r = 40, from = -135, to = 135, stroke = "#ffffff", width = 5, opacity = 0.08 },
      SVG.circle { cx = 55, cy = 52, r = 30, fill = fmt("url(#%s)", body), stroke = "#000000", width = 1, opacity = 0.95 },
      SVG.rect { x = 6, y = 106, w = 98, h = 24, rx = 6, fill = "#0a0616", stroke = "#ffffff", width = 1, opacity = 0.85 },
      SVG.text { x = 14, y = 122, text = p.label or "", size = 9, weight = "bold", spacing = 1, fill = "#f2e9ff" },
    }
  end,
}

local KnobValue = Component.define {
  name = "KnobValue",
  render = function(p: Props<{ v: number?, text: string?, color: string, glow: number }>)
    local v, c = p.v or 0, p.color
    return {
      SVG.arc { cx = 55, cy = 52, r = 40, from = -135, to = -135 + 270 * v, stroke = c, width = 5, glow = p.glow, },
      SVG.rect { x = 53, y = 25, w = 4, h = 14, rx = 2, fill = c, rotate = { -135 + 270 * v, 55, 52 } },
      SVG.text { x = 96, y = 122, text = p.text or "", size = 9, weight = "bold", anchor = "end", fill = c },
    }
  end,
}

local NeonKnob = Component.define {
  name = "NeonKnob",
  kind = "knob",
  size = { 110, 140 },
  render = function(p: Props<{ label: string, color: string? }>)
    local glow = p.dragging and 7 or (p.hover and 5 or 3)
    return {
      KnobBody.new { label = p.label },
      KnobValue.new { v = p.value, text = p.display, color = p.color or "#ff4fb0", glow = glow },
    }
  end,
}

return NeonKnob
