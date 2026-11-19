-- A reel, animated: its speed follows the bound value (drive).
local style = require("./style")
local REEL_R = style.REEL_R

local Reel = Component.define {
  name = "Reel",
  animated = true,
  size = { 2 * REEL_R + 8, 2 * REEL_R + 8 },
  render = function(p: Props<{ phase: number }>)
    -- Integrate the angle so changing the speed never makes the reel jump.
    local s, t = p.state, p.time or 0
    local speed = 50 + 220 * (p.value or 0)          -- degrees per second
    s.angle = ((s.angle or 0) + (t - (s.t or t)) * speed) % 360
    s.t = t
    local c = REEL_R + 4
    local parts = {
      SVG.circle { cx = c, cy = c, r = REEL_R, stroke = "#cfc8d6", width = 2, opacity = 0.55 },
      SVG.circle { cx = c, cy = c, r = REEL_R - 6, stroke = "#cfc8d6", width = 1, opacity = 0.2 },
    }
    for i = 0, 2 do
      local a = s.angle + i * 120 + p.phase
      parts[#parts + 1] = SVG.rect { x = c - 5, y = c - REEL_R + 4, w = 10, h = REEL_R - 26, rx = 3,
                                     fill = "#cfc8d6", opacity = 0.75, rotate = { a, c, c } }
    end
    parts[#parts + 1] = SVG.circle { cx = c, cy = c, r = 22, fill = "#cfc8d6" }
    parts[#parts + 1] = SVG.circle { cx = c, cy = c, r = 8, fill = "#1a161d" }
    return parts
  end,
}

return Reel
