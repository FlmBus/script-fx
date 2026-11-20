-- Panel -> Section -> Strip -> Knob (4 levels). Dragging a knob must not
-- re-render any of its ancestors.
local params = {}
for _, k in { "low1", "low2", "low3", "low4", "high1", "high2", "high3", "high4" } do
  params[k] = Blocks.Gain.new(k).params.db
end
local Knob = Component.define {
  name = "Knob", kind = "knob", size = { 60, 60 },
  render = function(p)
    local v = p.value or 0
    return {
      SVG.arc { cx = 30, cy = 30, r = 22, from = -135, to = -135 + 270 * v, stroke = p.theme.accent, width = 4 },
      SVG.circle { cx = 30, cy = 30, r = 15, fill = "#2a2d33" },
    }
  end,
}

local Strip = Component.define {
  name = "Strip",
  render = function(p)
    return {
      SVG.rect { x = 0, y = 0, w = p.width, h = p.height, rx = 6, fill = "#24272c" },
      SVG.text { x = 8, y = 16, text = p.title, size = 10, fill = "#c3c8d0" },
      Knob.new { bind = p.binds[1], x = 8, y = 24 },
      Knob.new { bind = p.binds[2], x = 74, y = 24 },
    }
  end,
}

local Section = Component.define {
  name = "Section",
  render = function(p)
    return {
      SVG.rect { x = 0, y = 0, w = p.width, h = p.height, rx = 10, stroke = p.theme.accent, width = 1 },
      SVG.text { x = 12, y = 20, text = p.title, size = 12, weight = "bold", fill = p.theme.accent },
      Strip.new { title = "A", binds = { params[p.prefix .. "1"], params[p.prefix .. "2"] }, x = 10, y = 30, w = 142, h = 92 },
      Strip.new { title = "B", binds = { params[p.prefix .. "3"], params[p.prefix .. "4"] }, x = 162, y = 30, w = 142, h = 92 },
    }
  end,
}

local Panel = Component.define {
  name = "Panel",
  render = function(p)
    return {
      SVG.rect { x = 0, y = 0, w = p.width, h = p.height, fill = "#1d1f23" },
      Section.new { title = "LOW", prefix = "low", x = 20, y = 20, w = 314, h = 132 },
      Section.new { title = "HIGH", prefix = "high", x = 354, y = 20, w = 314, h = 132 },
    }
  end,
}

return Patch.new("fixture", { panel = Panel.new { theme = { accent = "#f2c14e" } } })
