-- A level meter: static frame + scale, moving bars and readout.
local fmt = string.format

local MeterFrame = Component.define {
  name = "MeterFrame",
  render = function(p: Props<{ label: string, color: string }>)
    local w, h = p.width, p.height
    local top, bar = 56, h - 86
    -- No box: label, scale and grooves sit straight on the background (like Clean Gain).
    local parts = {
      SVG.text { x = w / 2, y = 20, text = p.label, size = 11, weight = "bold", anchor = "middle", fill = p.color },
    }
    for _, db in ipairs({ 0, -6, -12, -18, -24, -40 }) do
      local y = top + bar * (-db / 60)
      parts[#parts + 1] = SVG.text { x = 8, y = y + 3, text = tostring(db), size = 9, fill = "#c9b8e8", opacity = 0.8 }
      parts[#parts + 1] = SVG.rect { x = 26, y = y, w = 5, h = 1, fill = "#c9b8e8", opacity = 0.35 }
    end
    for _, x in ipairs({ 36, 46 }) do
      parts[#parts + 1] = SVG.rect { x = x, y = top, w = 6, h = bar, rx = 3, fill = "#000000", opacity = 0.35 }
      parts[#parts + 1] = SVG.rect { x = x, y = top, w = 6, h = bar, rx = 3, fill = "none", stroke = "#ffffff", width = 0.5, opacity = 0.12 }
    end
    return parts
  end,
}

local MeterBars = Component.define {
  name = "MeterBars",
  render = function(p: Props<{ lvl: number? }>)
    local lvl, h = p.lvl or 0, p.height
    local top, bottom = 56, h - 30
    local fh = (bottom - top) * lvl
    local fill, dfill = SVG.linear { from = { 0, 1 }, to = { 0, 0 }, stops = { { 0, "#3fd7ff" }, { 1, "#ff4fb0" } } }
    return { dfill,
      SVG.text { x = p.width / 2, y = 38, text = fmt("%.1f", -60 + 60 * lvl), size = 11, anchor = "middle", fill = "#f2e9ff" },
      SVG.rect { x = 36, y = bottom - fh, w = 6, h = fh, rx = 3, fill = fmt("url(#%s)", fill) },
      SVG.rect { x = 46, y = bottom - fh, w = 6, h = fh, rx = 3, fill = fmt("url(#%s)", fill) } }
  end,
}

local Meter = Component.define {
  name = "Meter",
  render = function(p: Props<{ label: string, color: string }>)
    return {
      MeterFrame.new { label = p.label, color = p.color },
      MeterBars.new { lvl = p.level },
    }
  end,
}

return Meter
