-- A small readout box for a param the Character knob moves.
local style = require("./style")
local ACCENT, EDGE, DIM = style.ACCENT, style.EDGE, style.DIM

local Readout = Component.define {
  name = "Readout",
  size = { 120, 52 },
  render = function(p: Props<{ label: string }>)
    return {
      SVG.rect { x = 0.5, y = 0.5, w = p.width - 1, h = p.height - 1, rx = 8, fill = "#120f15", stroke = EDGE, width = 1 },
      SVG.text { x = p.width / 2, y = 20, text = p.label, size = 9, spacing = 2, anchor = "middle", fill = DIM },
      SVG.text { x = p.width / 2, y = 40, text = p.display or "", size = 14, weight = "bold", anchor = "middle", fill = ACCENT },
    }
  end,
}

return Readout
