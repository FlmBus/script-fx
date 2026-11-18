-- Clean Gain: one big fader between an input and an output meter.
-- patch.luau passes the gain macro in: ui.Panel.new({ gain = gain })

local fmt = string.format

local BG, EDGE = "#0f1a1d", "#35505a"
local ACCENT, HOT, TEXT, DIM = "#4fd1c5", "#ff5d73", "#e6f4f1", "#6f8f97"
local MIN_DB, MAX_DB = -24, 12            -- the gain param's range

-- Fader geometry (inside the Fader component).
local TOP, BOTTOM = 40, 360               -- cap centre at value 1 / 0
local function y_of(v: number): number return BOTTOM - (BOTTOM - TOP) * v end
local function v_of_db(db: number): number return (db - MIN_DB) / (MAX_DB - MIN_DB) end

---------------------------------------------------------------- background (static)
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

---------------------------------------------------------------- fader: static track + moving cap
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

---------------------------------------------------------------- meter: static frame + moving bar
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

---------------------------------------------------------------- panel
local Panel = Component.define {
  name = "Panel",
  render = function(p: Props<{ gain: Macro }>)
    return {
      Background.new {},
      Meter.new { key = "in", label = "IN", source = Meters.IN, x = 250, y = 130 },
      Fader.new { bind = p.gain, x = 350, y = 120 },
      Meter.new { key = "out", label = "OUT", source = Meters.OUT, x = 540, y = 130 },
    }
  end,
}

return { Panel = Panel }
