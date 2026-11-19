-- The static background: three colour glows and a glowing frame.
local fmt = string.format

local Background = Component.define {
  name = "Background",
  render = function(p: Props<{}>)
    local w, h = p.width, p.height
    local g1, d1 = SVG.radial { cx = 0.15, cy = 0.05, r = 0.55, stops = { { 0, "#d63cff", 0.55 }, { 1, "#d63cff", 0 } } }
    local g2, d2 = SVG.radial { cx = 0.9, cy = 0.6, r = 0.5, stops = { { 0, "#2878ff", 0.45 }, { 1, "#2878ff", 0 } } }
    local g3, d3 = SVG.radial { cx = 0.5, cy = 1.0, r = 0.6, stops = { { 0, "#781ec8", 0.4 }, { 1, "#781ec8", 0 } } }
    local parts = {
      d1, d2, d3,
      SVG.rect { x = 0, y = 0, w = w, h = h, fill = "#0d0820" },
      SVG.rect { x = 0, y = 0, w = w, h = h, fill = fmt("url(#%s)", g1) },
      SVG.rect { x = 0, y = 0, w = w, h = h, fill = fmt("url(#%s)", g2) },
      SVG.rect { x = 0, y = 0, w = w, h = h, fill = fmt("url(#%s)", g3) },
    }
    -- Glowing frame without a blur filter (a blur over the whole panel is slow):
    -- stacked strokes with falling opacity.
    for i = 6, 1, -1 do
      parts[#parts + 1] = SVG.rect { x = 10, y = 10, w = w - 20, h = h - 20, rx = 14, stroke = "#dc5aff", width = i * 2, opacity = 0.05 }
    end
    parts[#parts + 1] = SVG.rect { x = 10, y = 10, w = w - 20, h = h - 20, rx = 14, stroke = "#e88cff", width = 1.2, opacity = 0.8 }
    return parts
  end,
}

return Background
