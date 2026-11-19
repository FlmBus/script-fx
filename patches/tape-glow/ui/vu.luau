-- The VU meter: static face + moving needle (follows the output level).
local fmt = string.format

local PIVOT = { 90, 150 }   -- relative to the meter, below the face

local function vu_point(deg: number, r: number): (number, number)
  local a = math.rad(deg)
  return PIVOT[1] + r * math.sin(a), PIVOT[2] - r * math.cos(a)
end

local VuFace = Component.define {
  name = "VuFace",
  render = function(p: Props<{}>)
    local face, dface = SVG.linear { stops = { { 0, "#fbf1e2" }, { 1, "#e3d2b9" } } }
    local parts = {
      dface,
      SVG.rect { x = 0, y = 0, w = p.width, h = p.height, rx = 8, fill = fmt("url(#%s)", face), stroke = "#0b090d", width = 3 },
      SVG.arc { cx = PIVOT[1], cy = PIVOT[2], r = 104, from = -38, to = 17, stroke = "#2b2530", width = 1.5 },
      SVG.arc { cx = PIVOT[1], cy = PIVOT[2], r = 104, from = 17, to = 38, stroke = "#d6452a", width = 4, cap = "butt" },
      SVG.text { x = p.width / 2, y = 92, text = "VU", size = 14, weight = "bold", anchor = "middle", fill = "#2b2530" },
    }
    for i, db in ipairs({ -20, -10, -7, -5, -3, -1, 0, 1, 3 }) do
      local deg = -38 + 76 * (i - 1) / 8
      local x0, y0 = vu_point(deg, 98)
      local x1, y1 = vu_point(deg, 110)
      parts[#parts + 1] = SVG.line { x1 = x0, y1 = y0, x2 = x1, y2 = y1, stroke = db > 0 and "#d6452a" or "#2b2530", width = 1.5 }
      local tx, ty = vu_point(deg, 120)
      parts[#parts + 1] = SVG.text { x = tx, y = ty + 3, text = tostring(math.abs(db)), size = 8, anchor = "middle",
                                     fill = db > 0 and "#d6452a" or "#2b2530" }
    end
    return parts
  end,
}

local VuNeedle = Component.define {
  name = "VuNeedle",
  render = function(p: Props<{ lvl: number? }>)
    local deg = -40 + 80 * Util.clamp(p.lvl or 0, 0, 1)
    local x, y = vu_point(deg, 114)
    local x0, y0 = vu_point(deg, 40)
    return {
      SVG.line { x1 = x0, y1 = y0, x2 = x, y2 = y, stroke = "#1a161d", width = 1.6 },
    }
  end,
}

local Vu = Component.define {
  name = "Vu",
  render = function(p: Props<{}>)
    return { VuFace.new {}, VuNeedle.new { lvl = p.level } }
  end,
}

return Vu
