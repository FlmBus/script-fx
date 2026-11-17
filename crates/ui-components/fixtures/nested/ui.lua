-- Panel -> Section -> Strip -> Knob (4 levels). Dragging a knob must not
-- re-render any of its ancestors.
local Knob = component {
  name = "Knob", kind = "knob", size = { 60, 60 },
  render = function(p)
    local v = p.value or 0
    return {
      svg.arc { cx = 30, cy = 30, r = 22, from = -135, to = -135 + 270 * v, stroke = p.theme.accent, width = 4 },
      svg.circle { cx = 30, cy = 30, r = 15, fill = "#2a2d33" },
    }
  end,
}

local Strip = component {
  name = "Strip",
  render = function(p)
    return {
      svg.rect { x = 0, y = 0, w = p.width, h = p.height, rx = 6, fill = "#24272c" },
      svg.text { x = 8, y = 16, text = p.title, size = 10, fill = "#c3c8d0" },
      Knob { bind = p.binds[1], x = 8, y = 24 },
      Knob { bind = p.binds[2], x = 74, y = 24 },
    }
  end,
}

local Section = component {
  name = "Section",
  render = function(p)
    return {
      svg.rect { x = 0, y = 0, w = p.width, h = p.height, rx = 10, stroke = p.theme.accent, width = 1 },
      svg.text { x = 12, y = 20, text = p.title, size = 12, weight = "bold", fill = p.theme.accent },
      Strip { title = "A", binds = { p.prefix .. "1", p.prefix .. "2" }, x = 10, y = 30, w = 142, h = 92 },
      Strip { title = "B", binds = { p.prefix .. "3", p.prefix .. "4" }, x = 162, y = 30, w = 142, h = 92 },
    }
  end,
}

local Panel = component {
  name = "Panel",
  render = function(p)
    return {
      svg.rect { x = 0, y = 0, w = p.width, h = p.height, fill = "#1d1f23" },
      Section { title = "LOW", prefix = "low", x = 20, y = 20, w = 314, h = 132 },
      Section { title = "HIGH", prefix = "high", x = 354, y = 20, w = 314, h = 132 },
    }
  end,
}

return { width = 690, height = 172, root = Panel { theme = { accent = "#f2c14e" } } }
