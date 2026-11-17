-- 120 knobs (12 x 10), each bound to its own parameter.
local Knob = component {
  name = "Knob", kind = "knob", size = { 64, 60 },
  render = function(p)
    local v = p.value or 0
    return {
      svg.arc { cx = 32, cy = 26, r = 22, from = -135, to = 135, stroke = "#ffffff", width = 3, opacity = 0.1 },
      svg.arc { cx = 32, cy = 26, r = 22, from = -135, to = -135 + 270 * v, stroke = "#4da3ff", width = 3 },
      svg.circle { cx = 32, cy = 26, r = 15, fill = "#2a2d33" },
      svg.rect { x = 31, y = 12, w = 2, h = 8, fill = "#ffffff", rotate = { -135 + 270 * v, 32, 26 } },
      svg.text { x = 32, y = 57, text = p.display or "", size = 9, anchor = "middle", fill = "#c3c8d0" },
    }
  end,
}

local Grid = component {
  name = "Grid",
  render = function(p)
    local items = { svg.rect { x = 0, y = 0, w = p.width, h = p.height, fill = "#1d1f23" } }
    for i = 0, 119 do
      items[#items + 1] = Knob { key = i, bind = "p" .. i, x = 6 + (i % 12) * 69, y = 6 + (i // 12) * 64 }
    end
    return items
  end,
}

return { width = 840, height = 650, root = Grid {} }
