-- 120 knobs (12 x 10), each bound to its own parameter.
local params = {}
for i = 0, 119 do params[i] = Blocks.Gain.new("g" .. i).params.db end

local Knob = Component.define {
  name = "Knob", kind = "knob", size = { 64, 54 },
  render = function(p)
    local v = p.value or 0
    return {
      SVG.arc { cx = 32, cy = 26, r = 22, from = -135, to = 135, stroke = "#ffffff", width = 3, opacity = 0.1 },
      SVG.arc { cx = 32, cy = 26, r = 22, from = -135, to = -135 + 270 * v, stroke = "#4da3ff", width = 3 },
      SVG.circle { cx = 32, cy = 26, r = 15, fill = "#2a2d33" },
      SVG.rect { x = 31, y = 12, w = 2, h = 8, fill = "#ffffff", rotate = { -135 + 270 * v, 32, 26 } },
      SVG.text { x = 32, y = 52, text = p.display or "", size = 9, anchor = "middle", fill = "#c3c8d0" },
    }
  end,
}

local Grid = Component.define {
  name = "Grid",
  render = function(p)
    local items = { SVG.rect { x = 0, y = 0, w = p.width, h = p.height, fill = "#1d1f23" } }
    for i = 0, 119 do
      items[#items + 1] = Knob.new { key = i, bind = params[i], x = 6 + (i % 12) * 69, y = 6 + (i // 12) * 55 }
    end
    return items
  end,
}

return Patch.new("fixture", { surface = Grid.new {} })
