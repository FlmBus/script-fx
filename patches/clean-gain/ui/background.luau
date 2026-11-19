-- The static background: glow, frame, title.
local style = require("./style")
local fmt = string.format
local BG, EDGE, ACCENT, TEXT, DIM = style.BG, style.EDGE, style.ACCENT, style.TEXT, style.DIM

local Background = Component.define {
  name = "Background",
  render = function(p: Props<{}>)
    local w, h = p.width, p.height
    local glow, dglow = SVG.radial { cx = 0.5, cy = 0.55, r = 0.6, stops = { { 0, ACCENT, 0.10 }, { 1, ACCENT, 0 } } }
    return {
      dglow,
      SVG.rect { x = 0, y = 0, w = w, h = h, fill = BG },
      SVG.rect { x = 0, y = 0, w = w, h = h, fill = fmt("url(#%s)", glow) },
      SVG.rect { x = 16, y = 16, w = w - 32, h = h - 32, rx = 18, stroke = EDGE, width = 1.5 },
      SVG.text { x = 48, y = 64, text = "CLEAN GAIN", size = 24, weight = "bold", spacing = 5, fill = TEXT },
      SVG.text { x = 50, y = 80, text = "NOTHING BUT LEVEL", size = 9, spacing = 2.5, fill = DIM },
    }
  end,
}

return Background
