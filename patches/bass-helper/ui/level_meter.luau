-- The in/out level meters.
local style = require("./style")
local fmt = string.format
local ACCENT, COOL, HOT, EDGE, DIM = style.ACCENT, style.COOL, style.HOT, style.EDGE, style.DIM

local LevelMeter = Component.define {
  name = "LevelMeter",
  size = { 30, 300 },
  render = function(p: Props<{ label: string }>)
    local lvl = Util.clamp(p.level or 0, 0, 1)
    local top, bottom = 22, p.height - 4
    local hgt = (bottom - top) * lvl
    local grad, dgrad = SVG.linear { from = { 0, 1 }, to = { 0, 0 }, stops = { { 0, COOL }, { 0.75, ACCENT }, { 1, HOT } } }
    return { dgrad,
      SVG.text { x = 15, y = 12, text = p.label, size = 9, weight = "bold", spacing = 1, anchor = "middle", fill = DIM },
      SVG.rect { x = 8, y = top, w = 14, h = bottom - top, rx = 4, fill = "#0b0e11", stroke = EDGE, width = 1 },
      SVG.rect { x = 10, y = bottom - 2 - hgt, w = 10, h = hgt, rx = 2, fill = fmt("url(#%s)", grad) } }
  end,
}

return LevelMeter
