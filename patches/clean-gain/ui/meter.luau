-- A level meter: static frame + scale, moving bar.
local style = require("./style")
local fmt = string.format
local EDGE, ACCENT, HOT, DIM = style.EDGE, style.ACCENT, style.HOT, style.DIM

local METER_TOP, METER_BOTTOM = 40, 360

local MeterFrame = Component.define {
  name = "MeterFrame",
  render = function(p: Props<{ label: string }>)
    local parts = {
      SVG.text { x = 20, y = 20, text = p.label, size = 11, weight = "bold", spacing = 2, anchor = "middle", fill = DIM },
      SVG.rect { x = 10, y = METER_TOP, w = 20, h = METER_BOTTOM - METER_TOP, rx = 4, fill = "#081012", stroke = EDGE, width = 1 },
    }
    for _, db in ipairs({ 0, -12, -24, -36, -48 }) do
      local y = METER_BOTTOM - (METER_BOTTOM - METER_TOP) * (db + 60) / 60
      parts[#parts + 1] = SVG.text { x = 38, y = y + 3, text = tostring(db), size = 8, fill = DIM }
    end
    return parts
  end,
}

local MeterBar = Component.define {
  name = "MeterBar",
  render = function(p: Props<{ lvl: number? }>)
    local lvl = Util.clamp(p.lvl or 0, 0, 1)
    local h = (METER_BOTTOM - METER_TOP - 4) * lvl
    local grad, dgrad = SVG.linear { from = { 0, 1 }, to = { 0, 0 }, stops = { { 0, ACCENT }, { 0.8, ACCENT }, { 1, HOT } } }
    return { dgrad, SVG.rect { x = 13, y = METER_BOTTOM - 2 - h, w = 14, h = h, rx = 2, fill = fmt("url(#%s)", grad) } }
  end,
}

local Meter = Component.define {
  name = "Meter",
  size = { 60, 380 },
  render = function(p: Props<{ label: string }>)
    return { MeterFrame.new { label = p.label }, MeterBar.new { lvl = p.level } }
  end,
}

return Meter
