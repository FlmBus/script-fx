-- An animated LED.

local Pulse = Component.define {
  name = "Pulse",
  animated = true,
  size = { 24, 24 },
  render = function(p: Props<{}>)
    local t = p.time or 0
    local a = 0.55 + 0.45 * math.sin(t * 4)
    local glow, d = SVG.blur(2 + 2 * a)
    return { d,
      SVG.circle { cx = 12, cy = 12, r = 6, fill = "#3fd7ff", opacity = a, filter = glow },
      SVG.circle { cx = 12, cy = 12, r = 3.5, fill = "#d9f7ff" } }
  end,
}

return Pulse
