-- A toggle button with local state (on/off).

local Toggle = Component.define {
  name = "Toggle",
  kind = "button",
  size = { 140, 32 },
  on_click = function(state) state.on = not state.on end,
  render = function(p: Props<{ label: string, on_text: string?, default_on: boolean?, color: string? }>)
    local on = p.state.on
    if on == nil then on = p.default_on end
    local c = p.color or "#3fd7ff"
    local glow, d = SVG.blur(3)
    return { d,
      SVG.rect { x = 0.5, y = 0.5, w = p.width - 1, h = p.height - 1, rx = 7, fill = "#0a0616", stroke = p.hover and c or "#ffffff", width = 1, opacity = 0.9 },
      on and SVG.circle { cx = 18, cy = 16, r = 5, fill = c, filter = glow } or "",
      SVG.circle { cx = 18, cy = 16, r = 4, fill = on and c or "#3a3448" },
      SVG.text { x = 32, y = 20, text = p.label, size = 10, weight = "bold", spacing = 1, fill = "#f2e9ff" },
      SVG.text { x = p.width - 12, y = 20, text = on and (p.on_text or "ON") or "OFF", size = 10, weight = "bold", anchor = "end", fill = on and c or "#8a80a0" },
    }
  end,
}

return Toggle
