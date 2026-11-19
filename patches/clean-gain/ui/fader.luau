-- The gain fader: a static track (scale) and the moving cap, as separate layers.
local style = require("./style")
local fmt = string.format
local EDGE, ACCENT, TEXT, DIM = style.EDGE, style.ACCENT, style.TEXT, style.DIM
local MIN_DB, MAX_DB = style.MIN_DB, style.MAX_DB

-- Fader geometry (inside the Fader component).
local TOP, BOTTOM = 40, 360               -- cap centre at value 1 / 0
local function y_of(v: number): number return BOTTOM - (BOTTOM - TOP) * v end
local function v_of_db(db: number): number return (db - MIN_DB) / (MAX_DB - MIN_DB) end

local FaderTrack = Component.define {
  name = "FaderTrack",
  render = function(p: Props<{}>)
    local cx = p.width / 2
    local parts = {
      SVG.rect { x = cx - 5, y = TOP - 8, w = 10, h = BOTTOM - TOP + 16, rx = 5, fill = "#081012", stroke = EDGE, width = 1 },
    }
    for db = MAX_DB, MIN_DB, -6 do
      local y = y_of(v_of_db(db))
      local unity = db == 0
      parts[#parts + 1] = SVG.line { x1 = cx - 34, y1 = y, x2 = cx - 16, y2 = y, stroke = unity and ACCENT or DIM, width = unity and 2 or 1 }
      parts[#parts + 1] = SVG.line { x1 = cx + 16, y1 = y, x2 = cx + 34, y2 = y, stroke = unity and ACCENT or DIM, width = unity and 2 or 1 }
      parts[#parts + 1] = SVG.text { x = cx + 42, y = y + 4, text = (db > 0 and "+" or "") .. db, size = 10, fill = unity and ACCENT or DIM }
    end
    return parts
  end,
}

local FaderCap = Component.define {
  name = "FaderCap",
  render = function(p: Props<{ v: number?, label: string?, active: boolean }>)
    local cx, y = p.width / 2, y_of(p.v or 0)
    local cap, dcap = SVG.linear { stops = { { 0, "#3b5560" }, { 0.5, "#24363d" }, { 1, "#162327" } } }
    return {
      dcap,
      -- value fill from unity, so boost and cut read at a glance
      SVG.rect { x = cx - 2, y = math.min(y, y_of(v_of_db(0))), w = 4, h = math.abs(y - y_of(v_of_db(0))), fill = ACCENT, opacity = 0.8 },
      SVG.rect { x = cx - 30, y = y - 15, w = 60, h = 32, rx = 5, fill = "#000000", opacity = 0.35 },
      SVG.rect { x = cx - 30, y = y - 18, w = 60, h = 32, rx = 5, fill = fmt("url(#%s)", cap), stroke = p.active and ACCENT or "#06090a", width = 1.5 },
      SVG.line { x1 = cx - 22, y1 = y - 2, x2 = cx + 22, y2 = y - 2, stroke = p.active and ACCENT or TEXT, width = 2 },
      SVG.text { x = cx, y = TOP - 22, text = p.label or "", size = 20, weight = "bold", anchor = "middle", fill = TEXT },
    }
  end,
}

local Fader = Component.define {
  name = "Fader",
  kind = "knob",              -- vertical drag, double-click resets
  size = { 140, 390 },
  render = function(p: Props<{}>)
    return {
      FaderTrack.new {},
      FaderCap.new { v = p.value, label = p.display, active = p.dragging or p.hover },
    }
  end,
}

return Fader
