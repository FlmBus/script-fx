-- The logo (static).
local fmt = string.format

local Logo = Component.define {
  name = "Logo",
  render = function(p: Props<{}>)
    local cx = p.width / 2
    local a, da = SVG.linear { stops = { { 0, "#ffd27a" }, { 1, "#ff5fa8" } } }
    local b, db = SVG.linear { stops = { { 0, "#ff9ad5" }, { 1, "#c04dff" } } }
    local glow, dglow = SVG.blur(5, 0.1)
    return {
      da, db, dglow,
      SVG.text { x = cx, y = 60, text = "NEON", size = 52, weight = "bold", anchor = "middle", spacing = 6, fill = "#ff5fa8", opacity = 0.7, filter = glow },
      SVG.text { x = cx, y = 60, text = "NEON", size = 52, weight = "bold", anchor = "middle", spacing = 6, fill = fmt("url(#%s)", a) },
      SVG.text { x = cx, y = 104, text = "SATURATOR", size = 36, weight = "bold", anchor = "middle", spacing = 5, fill = fmt("url(#%s)", b) },
      SVG.rect { x = cx - 120, y = 124, w = 240, h = 28, rx = 14, fill = "#140828", stroke = "#dc78ff", width = 1, opacity = 0.9 },
      SVG.text { x = cx, y = 143, text = "HARMONIC MULTI-FX", size = 11, anchor = "middle", spacing = 3, fill = "#f2e9ff" },
      SVG.circle { cx = cx - 102, cy = 138, r = 3.5, fill = "#ff4fb0" },
      SVG.circle { cx = cx + 102, cy = 138, r = 3.5, fill = "#3fd7ff" },
    }
  end,
}

return Logo
