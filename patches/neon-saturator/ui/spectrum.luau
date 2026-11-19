-- The output spectrum display (`series`).
local fmt = string.format

local Spectrum = Component.define {
  name = "Spectrum",
  render = function(p: Props<{}>)
    local s: { number } = p.series or {}
    local w, h = p.width, p.height
    local pts = {}
    for i, v in ipairs(s) do
      pts[#pts + 1] = fmt("%s%.1f %.1f", i == 1 and "M" or "L", (i - 1) / math.max(1, #s - 1) * w, h - 6 - v * (h - 16))
    end
    local line = table.concat(pts, " ")
    local area, darea = SVG.linear { stops = { { 0, "#ff4fb0", 0.45 }, { 1, "#3fd7ff", 0.05 } } }
    return { darea,
      SVG.rect { x = 0, y = 0, w = w, h = h, rx = 8, fill = "#080414", stroke = "#ffffff", width = 1, opacity = 0.7 },
      #s > 1 and SVG.path { d = line .. fmt(" L%.1f %.1f L0 %.1f Z", w, h, h), fill = fmt("url(#%s)", area) } or "",
      #s > 1 and SVG.path { d = line, stroke = "#ff9ad5", width = 1.5 } or "",
      SVG.text { x = 10, y = 18, text = "OUTPUT SPECTRUM", size = 9, spacing = 2, fill = "#c9b8e8" },
    }
  end,
}

return Spectrum
