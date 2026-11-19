-- Live block meters inside the strip, and the grit LED.
local style = require("./style")
local fmt = string.format
local COOL, HOT, DIM = style.COOL, style.HOT, style.DIM

-- Gain reduction (compressor meters: 0..1 = 0..24 dB).
local GrMeter = Component.define {
  name = "GrMeter",
  render = function(p: Props<{}>)
    local gr = Util.clamp(p.level or 0, 0, 1)
    local w = p.width
    return {
      SVG.text { x = w, y = 10, text = fmt("-%.1f dB", gr * 24), size = 11, weight = "bold", anchor = "end", fill = gr > 0.01 and HOT or DIM },
      SVG.rect { x = 0, y = 20, w = w, h = 6, rx = 3, fill = "#ffffff", opacity = 0.06 },
      SVG.rect { x = w * (1 - gr), y = 20, w = w * gr, h = 6, rx = 3, fill = HOT },
    }
  end,
}

-- Gain change around unity (rider/transient meters: 0.5 = unity, 0..1 = ±12 dB).
local GainMeter = Component.define {
  name = "GainMeter",
  render = function(p: Props<{}>)
    local lvl = Util.clamp(p.level or 0.5, 0, 1)
    local w, cx = p.width, p.width / 2
    local x = lvl * w
    local db = (lvl - 0.5) * 24
    return {
      SVG.text { x = w, y = 10, text = fmt("%+.1f dB", db), size = 11, weight = "bold", anchor = "end", fill = math.abs(db) > 0.1 and COOL or DIM },
      SVG.rect { x = 0, y = 20, w = w, h = 6, rx = 3, fill = "#ffffff", opacity = 0.06 },
      SVG.rect { x = math.min(cx, x), y = 20, w = math.abs(x - cx), h = 6, fill = COOL },
      SVG.line { x1 = cx, y1 = 17, x2 = cx, y2 = 29, stroke = DIM, width = 1 },
    }
  end,
}

-- The high band's grit lights up with the grit mix.
local GritLed = Component.define {
  name = "GritLed",
  size = { 24, 24 },
  render = function(p: Props<{}>)
    local v = p.value or 0
    local g, dg = SVG.radial { stops = { { 0, HOT, 0.9 * v }, { 1, HOT, 0 } } }
    return { dg,
      SVG.circle { cx = 12, cy = 12, r = 12, fill = fmt("url(#%s)", g) },
      SVG.circle { cx = 12, cy = 12, r = 4, fill = v > 0.02 and HOT or "#3a2a28" } }
  end,
}

return { GrMeter = GrMeter, GainMeter = GainMeter, GritLed = GritLed }
