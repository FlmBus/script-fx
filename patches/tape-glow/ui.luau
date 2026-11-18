-- Tape Glow: a tape deck. Reels spin faster with more drive, the VU needle
-- and the glow at the tape head follow the output level.
-- patch.luau passes the macros and params in (see PanelProps).

local fmt = string.format

local BG, PANEL, EDGE = "#17141c", "#221c27", "#3a3140"
local ACCENT, TEXT, DIM = "#ff8a3d", "#f3e6d8", "#8c7f94"
local REELS = { { 200, 205 }, { 640, 205 } }
local REEL_R = 95

---------------------------------------------------------------- background (static)
local Background = Component.define {
  name = "Background",
  render = function(p: Props<{}>)
    local w, h = p.width, p.height
    local bg, dbg = SVG.linear { stops = { { 0, "#2a2230" }, { 1, BG } } }
    local parts = {
      dbg,
      SVG.rect { x = 0, y = 0, w = w, h = h, fill = fmt("url(#%s)", bg) },
      SVG.rect { x = 16, y = 16, w = w - 32, h = h - 32, rx = 18, fill = PANEL, stroke = EDGE, width = 1.5 },
      SVG.text { x = 48, y = 64, text = "TAPE GLOW", size = 26, weight = "bold", spacing = 6, fill = TEXT },
      SVG.text { x = 50, y = 80, text = "SATURATION · TONE · TAPE", size = 9, spacing = 2.5, fill = DIM },
      -- deck window
      SVG.rect { x = 40, y = 96, w = w - 80, h = 226, rx = 14, fill = "#120f15", stroke = EDGE, width = 1 },
    }
    -- wound tape packs (round, so they don't need to rotate)
    local pack, dpack = SVG.radial { stops = { { 0.55, "#4a3424" }, { 1, "#2a1c13" } } }
    parts[#parts + 1] = dpack
    for _, c in ipairs(REELS) do
      parts[#parts + 1] = SVG.circle { cx = c[1], cy = c[2], r = REEL_R + 4, fill = "#0b090d" }
      parts[#parts + 1] = SVG.circle { cx = c[1], cy = c[2], r = 78, fill = fmt("url(#%s)", pack) }
      for r = 40, 76, 6 do
        parts[#parts + 1] = SVG.circle { cx = c[1], cy = c[2], r = r, stroke = "#000000", width = 0.6, opacity = 0.25 }
      end
    end
    -- tape path over the guides to the head
    parts[#parts + 1] = SVG.path { d = "M 200 283 L 330 296 L 420 288 L 510 296 L 640 283", stroke = "#3d2a1c", width = 3 }
    for _, x in ipairs({ 330, 510 }) do
      parts[#parts + 1] = SVG.circle { cx = x, cy = 300, r = 7, fill = "#8d8696", stroke = "#2b2530", width = 1.5 }
    end
    -- the head
    local metal, dmetal = SVG.linear { from = { 0, 0 }, to = { 0, 1 }, stops = { { 0, "#b9b2c2" }, { 1, "#57505f" } } }
    parts[#parts + 1] = dmetal
    parts[#parts + 1] = SVG.rect { x = 396, y = 288, w = 48, h = 24, rx = 4, fill = fmt("url(#%s)", metal), stroke = "#1a161d", width = 1 }
    parts[#parts + 1] = SVG.rect { x = 418, y = 288, w = 4, h = 24, fill = "#2b2530" }
    -- knob section divider
    parts[#parts + 1] = SVG.line { x1 = 40, y1 = 344, x2 = w - 40, y2 = 344, stroke = EDGE, width = 1, opacity = 0.7 }
    return parts
  end,
}

---------------------------------------------------------------- reel (animated, speed follows drive)
local Reel = Component.define {
  name = "Reel",
  animated = true,
  size = { 2 * REEL_R + 8, 2 * REEL_R + 8 },
  render = function(p: Props<{ phase: number }>)
    -- Integrate the angle so changing the speed never makes the reel jump.
    local s, t = p.state, p.time or 0
    local speed = 50 + 220 * (p.value or 0)          -- degrees per second
    s.angle = ((s.angle or 0) + (t - (s.t or t)) * speed) % 360
    s.t = t
    local c = REEL_R + 4
    local parts = {
      SVG.circle { cx = c, cy = c, r = REEL_R, stroke = "#cfc8d6", width = 2, opacity = 0.55 },
      SVG.circle { cx = c, cy = c, r = REEL_R - 6, stroke = "#cfc8d6", width = 1, opacity = 0.2 },
    }
    for i = 0, 2 do
      local a = s.angle + i * 120 + p.phase
      parts[#parts + 1] = SVG.rect { x = c - 5, y = c - REEL_R + 4, w = 10, h = REEL_R - 26, rx = 3,
                                     fill = "#cfc8d6", opacity = 0.75, rotate = { a, c, c } }
    end
    parts[#parts + 1] = SVG.circle { cx = c, cy = c, r = 22, fill = "#cfc8d6" }
    parts[#parts + 1] = SVG.circle { cx = c, cy = c, r = 8, fill = "#1a161d" }
    return parts
  end,
}

---------------------------------------------------------------- VU meter: static face + moving needle
local VU = { x = 330, y = 110, w = 180, h = 128 }
local PIVOT = { 90, 150 }   -- relative to the meter, below the face

local function vu_point(deg: number, r: number): (number, number)
  local a = math.rad(deg)
  return PIVOT[1] + r * math.sin(a), PIVOT[2] - r * math.cos(a)
end

local VuFace = Component.define {
  name = "VuFace",
  render = function(p: Props<{}>)
    local face, dface = SVG.linear { stops = { { 0, "#fbf1e2" }, { 1, "#e3d2b9" } } }
    local parts = {
      dface,
      SVG.rect { x = 0, y = 0, w = p.width, h = p.height, rx = 8, fill = fmt("url(#%s)", face), stroke = "#0b090d", width = 3 },
      SVG.arc { cx = PIVOT[1], cy = PIVOT[2], r = 104, from = -38, to = 17, stroke = "#2b2530", width = 1.5 },
      SVG.arc { cx = PIVOT[1], cy = PIVOT[2], r = 104, from = 17, to = 38, stroke = "#d6452a", width = 4, cap = "butt" },
      SVG.text { x = p.width / 2, y = 92, text = "VU", size = 14, weight = "bold", anchor = "middle", fill = "#2b2530" },
    }
    for i, db in ipairs({ -20, -10, -7, -5, -3, -1, 0, 1, 3 }) do
      local deg = -38 + 76 * (i - 1) / 8
      local x0, y0 = vu_point(deg, 98)
      local x1, y1 = vu_point(deg, 110)
      parts[#parts + 1] = SVG.line { x1 = x0, y1 = y0, x2 = x1, y2 = y1, stroke = db > 0 and "#d6452a" or "#2b2530", width = 1.5 }
      local tx, ty = vu_point(deg, 120)
      parts[#parts + 1] = SVG.text { x = tx, y = ty + 3, text = tostring(math.abs(db)), size = 8, anchor = "middle",
                                     fill = db > 0 and "#d6452a" or "#2b2530" }
    end
    return parts
  end,
}

local VuNeedle = Component.define {
  name = "VuNeedle",
  render = function(p: Props<{ lvl: number? }>)
    local deg = -40 + 80 * Util.clamp(p.lvl or 0, 0, 1)
    local x, y = vu_point(deg, 114)
    local x0, y0 = vu_point(deg, 40)
    return {
      SVG.line { x1 = x0, y1 = y0, x2 = x, y2 = y, stroke = "#1a161d", width = 1.6 },
    }
  end,
}

local Vu = Component.define {
  name = "Vu",
  render = function(p: Props<{}>)
    return { VuFace.new {}, VuNeedle.new { lvl = p.level } }
  end,
}

---------------------------------------------------------------- glow at the tape head (follows the output)
local HeadGlow = Component.define {
  name = "HeadGlow",
  render = function(p: Props<{}>)
    local lvl = Util.clamp((p.level or 0) * 1.3 - 0.2, 0, 1)
    local g, dg = SVG.radial { stops = { { 0, ACCENT, 0.85 * lvl }, { 1, ACCENT, 0 } } }
    return { dg, SVG.circle { cx = p.width / 2, cy = p.height / 2, r = p.width / 2, fill = fmt("url(#%s)", g) } }
  end,
}

---------------------------------------------------------------- knob: static body + moving value
local KnobBody = Component.define {
  name = "KnobBody",
  render = function(p: Props<{ label: string }>)
    local cap, dcap = SVG.radial { cx = 0.38, cy = 0.32, r = 0.75, stops = { { 0, "#4e4456" }, { 1, "#1d1821" } } }
    return {
      dcap,
      SVG.arc { cx = 60, cy = 58, r = 46, from = -135, to = 135, stroke = "#ffffff", width = 4, opacity = 0.08 },
      SVG.circle { cx = 61, cy = 61, r = 34, fill = "#000000", opacity = 0.35 },
      SVG.circle { cx = 60, cy = 58, r = 34, fill = fmt("url(#%s)", cap), stroke = "#0b090d", width = 1 },
      SVG.text { x = 60, y = 128, text = p.label, size = 11, weight = "bold", spacing = 2, anchor = "middle", fill = TEXT },
    }
  end,
}

local KnobValue = Component.define {
  name = "KnobValue",
  render = function(p: Props<{ v: number?, text: string?, active: boolean }>)
    local v = p.v or 0
    local deg = -135 + 270 * v
    return {
      SVG.arc { cx = 60, cy = 58, r = 46, from = -135, to = deg, stroke = ACCENT, width = 4, glow = p.active and 4 or nil },
      SVG.rect { x = 58, y = 28, w = 4, h = 16, rx = 2, fill = TEXT, rotate = { deg, 60, 58 } },
      SVG.text { x = 60, y = 148, text = p.text or "", size = 10, anchor = "middle", fill = p.active and ACCENT or DIM },
    }
  end,
}

local Knob = Component.define {
  name = "Knob",
  kind = "knob",
  size = { 120, 160 },
  render = function(p: Props<{ label: string }>)
    return {
      KnobBody.new { label = p.label },
      KnobValue.new { v = p.value, text = p.display, active = p.dragging or p.hover },
    }
  end,
}

---------------------------------------------------------------- readout (shows a param the Character knob moves)
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

---------------------------------------------------------------- panel
export type PanelProps = { character: Macro, mix: Macro, output: Macro, drive: Param, tone: Param }

local Panel = Component.define {
  name = "Panel",
  render = function(p: Props<PanelProps>)
    local items = {
      Background.new {},
      HeadGlow.new { source = Meters.OUT, x = 360, y = 240, w = 120, h = 110 },
    }
    for i, c in ipairs(REELS) do
      items[#items + 1] = Reel.new { key = "reel" .. i, bind = p.drive, phase = i * 37,
                                     x = c[1] - REEL_R - 4, y = c[2] - REEL_R - 4 }
    end
    items[#items + 1] = Vu.new { source = Meters.OUT, x = VU.x, y = VU.y, w = VU.w, h = VU.h }
    items[#items + 1] = Knob.new { key = "mix", label = "MIX", bind = p.mix, x = 70, y = 362 }
    items[#items + 1] = Readout.new { key = "drive", label = "DRIVE", bind = p.drive, x = 210, y = 392 }
    items[#items + 1] = Knob.new { key = "character", label = "CHARACTER", bind = p.character, x = 360, y = 362 }
    items[#items + 1] = Readout.new { key = "tone", label = "TONE", bind = p.tone, x = 510, y = 392 }
    items[#items + 1] = Knob.new { key = "output", label = "OUTPUT", bind = p.output, x = 650, y = 362 }
    return items
  end,
}

return { Panel = Panel }
