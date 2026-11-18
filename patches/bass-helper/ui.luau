-- Bass Helper: signal-flow strip with live block meters on top, the three big
-- macros in the middle, tone/output/crossover below, in/out meters at the sides.
-- patch.luau passes the macros, params and metered blocks in (see PanelProps).

local fmt = string.format

local BG, BG2, PANEL, EDGE = "#101418", "#1b2229", "#26303a", "#3b4a57"
local ACCENT, COOL, HOT, TEXT, DIM = "#f2c14e", "#5fb3d9", "#ef6f50", "#e8edf2", "#7d8b98"

---------------------------------------------------------------- signal-flow strip geometry
local STRIP = { y = 100, h = 96 }
local BOXES = {
  punch = { x = 50,  y = 104, w = 96,  h = 88, label = "PUNCH" },
  fet   = { x = 166, y = 104, w = 104, h = 88, label = "FET" },
  opto  = { x = 290, y = 104, w = 104, h = 88, label = "OPTO" },
  low   = { x = 432, y = 104, w = 196, h = 41, label = "LOW · RIDER" },
  high  = { x = 432, y = 151, w = 196, h = 41, label = "HIGH · GRIT · FIZZ" },
  eq    = { x = 666, y = 104, w = 56,  h = 88, label = "EQ" },
  out   = { x = 742, y = 104, w = 48,  h = 88, label = "OUT" },
}

type Box = { x: number, y: number, w: number, h: number, label: string }

local function box(b: Box, active: boolean?): Part
  return {
    SVG.rect { x = b.x, y = b.y, w = b.w, h = b.h, rx = 6, fill = PANEL, stroke = active and ACCENT or EDGE, width = 1 },
    SVG.text { x = b.x + 8, y = b.y + 15, text = b.label, size = 9, weight = "bold", spacing = 1.5, fill = DIM },
  }
end

local function arrow(x1: number, y1: number, x2: number, y2: number): Part
  return {
    SVG.line { x1 = x1, y1 = y1, x2 = x2 - 4, y2 = y2, stroke = EDGE, width = 1.5 },
    SVG.path { d = fmt("M %d %d L %d %d L %d %d Z", x2, y2, x2 - 6, y2 - 4, x2 - 6, y2 + 4), fill = EDGE },
  }
end

---------------------------------------------------------------- background (static)
local Background = Component.define {
  name = "Background",
  render = function(p: Props<{}>)
    local w, h = p.width, p.height
    local bg, dbg = SVG.linear { stops = { { 0, BG2 }, { 1, BG } } }
    local cy = STRIP.y + STRIP.h / 2
    local parts = {
      dbg,
      SVG.rect { x = 0, y = 0, w = w, h = h, fill = fmt("url(#%s)", bg) },
      SVG.rect { x = 16, y = 16, w = w - 32, h = h - 32, rx = 16, stroke = EDGE, width = 1.5 },
      SVG.text { x = 48, y = 62, text = "BASS HELPER", size = 24, weight = "bold", spacing = 5, fill = TEXT },
      SVG.text { x = 50, y = 78, text = "PUNCH · CONTROL · GRIT", size = 9, spacing = 2.5, fill = DIM },
      SVG.rect { x = 40, y = STRIP.y - 4, w = w - 80, h = STRIP.h + 4, rx = 10, fill = "#0b0e11", stroke = EDGE, width = 1 },
    }
    for _, k in ipairs({ "punch", "fet", "opto", "low", "high", "eq", "out" }) do
      parts[#parts + 1] = box(BOXES[k])
    end
    -- arrows: punch → fet → opto → split → (low|high) → eq → out
    parts[#parts + 1] = arrow(146, cy, 166, cy)
    parts[#parts + 1] = arrow(270, cy, 290, cy)
    parts[#parts + 1] = SVG.line { x1 = 394, y1 = cy, x2 = 412, y2 = cy, stroke = EDGE, width = 1.5 }
    parts[#parts + 1] = SVG.path { d = "M 412 124 L 412 172", stroke = EDGE, width = 1.5 }
    parts[#parts + 1] = arrow(412, 124, 432, 124)
    parts[#parts + 1] = arrow(412, 172, 432, 172)
    parts[#parts + 1] = SVG.path { d = "M 628 124 L 646 124 L 646 172 L 628 172", stroke = EDGE, width = 1.5 }
    parts[#parts + 1] = arrow(646, cy, 666, cy)
    parts[#parts + 1] = arrow(722, cy, 742, cy)
    -- section labels
    parts[#parts + 1] = SVG.text { x = 420, y = 224, text = "MACROS", size = 9, spacing = 3, anchor = "middle", fill = DIM }
    parts[#parts + 1] = SVG.line { x1 = 110, y1 = 404, x2 = 730, y2 = 404, stroke = EDGE, width = 1, opacity = 0.6 }
    return parts
  end,
}

---------------------------------------------------------------- live block meters (inside the strip)
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

---------------------------------------------------------------- knob (scales with its size)
local KnobBody = Component.define {
  name = "KnobBody",
  render = function(p: Props<{ label: string, big: boolean?, bipolar: boolean? }>)
    local w = p.width
    local cx, cy, r = w / 2, w * 0.46, w * 0.28
    local cap, dcap = SVG.radial { cx = 0.38, cy = 0.32, r = 0.8, stops = { { 0, EDGE }, { 1, PANEL } } }
    local parts = {
      dcap,
      SVG.arc { cx = cx, cy = cy, r = r * 1.32, from = -135, to = 135, stroke = "#ffffff", width = w * 0.03, opacity = 0.07 },
      SVG.circle { cx = cx + 1, cy = cy + 2, r = r, fill = "#000000", opacity = 0.4 },
      SVG.circle { cx = cx, cy = cy, r = r, fill = fmt("url(#%s)", cap), stroke = "#07090b", width = 1 },
      SVG.text { x = cx, y = p.height - 22, text = p.label, size = p.big and 12 or 10, weight = "bold", spacing = 1.5, anchor = "middle", fill = TEXT },
    }
    if p.bipolar then
      parts[#parts + 1] = SVG.line { x1 = cx, y1 = cy - r * 1.48, x2 = cx, y2 = cy - r * 1.2, stroke = DIM, width = 1.5 }
    end
    return parts
  end,
}

local KnobValue = Component.define {
  name = "KnobValue",
  render = function(p: Props<{ v: number?, text: string?, color: string, big: boolean?, bipolar: boolean?, active: boolean }>)
    local w = p.width
    local cx, cy, r = w / 2, w * 0.46, w * 0.28
    local v = p.v or 0
    local deg = -135 + 270 * v
    local from = p.bipolar and 0 or -135
    return {
      SVG.arc { cx = cx, cy = cy, r = r * 1.32, from = from, to = deg, stroke = p.color, width = w * 0.03, glow = p.active and 3 or nil },
      SVG.line { x1 = cx + r * 0.35 * math.sin(math.rad(deg)), y1 = cy - r * 0.35 * math.cos(math.rad(deg)),
                 x2 = cx + r * 0.85 * math.sin(math.rad(deg)), y2 = cy - r * 0.85 * math.cos(math.rad(deg)),
                 stroke = TEXT, width = p.big and 3.5 or 2.5 },
      SVG.text { x = cx, y = p.height - 6, text = p.text or "", size = p.big and 11 or 9, anchor = "middle", fill = p.active and p.color or DIM },
    }
  end,
}

local Knob = Component.define {
  name = "Knob",
  kind = "knob",
  render = function(p: Props<{ label: string, color: string?, big: boolean?, bipolar: boolean? }>)
    local color = p.color or ACCENT
    return {
      KnobBody.new { label = p.label, big = p.big, bipolar = p.bipolar },
      KnobValue.new { v = p.value, text = p.display, color = color, big = p.big, bipolar = p.bipolar, active = p.dragging or p.hover },
    }
  end,
}

---------------------------------------------------------------- in/out meters
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

---------------------------------------------------------------- panel
local function meter_at(b: Box, comp: Component<{}>, source: MeteredBlock): ComponentInstance
  return comp.new { key = b.label, source = source, x = b.x + 8, y = b.y + 44, w = b.w - 16, h = 32 }
end

export type PanelProps = {
  transients: Macro, stabilize: Macro, drive: Macro,
  boom: Macro, boxy: Macro, click: Macro, output: Macro,
  xover: Param, grit_mix: Param,
  punch: MeteredBlock, fet: MeteredBlock, opto: MeteredBlock, rider: MeteredBlock,
}

local Panel = Component.define {
  name = "Panel",
  render = function(p: Props<PanelProps>)
    local items = {
      Background.new {},
      meter_at(BOXES.punch, GainMeter, p.punch),
      meter_at(BOXES.fet, GrMeter, p.fet),
      meter_at(BOXES.opto, GrMeter, p.opto),
      GainMeter.new { key = "rider", source = p.rider, x = BOXES.low.x + 110, y = BOXES.low.y + 6, w = 78, h = 32 },
      GritLed.new { bind = p.grit_mix, x = BOXES.high.x + 160, y = BOXES.high.y + 9 },
      LevelMeter.new { key = "in", label = "IN", source = Meters.IN, x = 48, y = 228 },
      LevelMeter.new { key = "out", label = "OUT", source = Meters.OUT, x = 762, y = 228 },
    }
    local big: { { label: string, bind: Bindable, color: string? } } = {
      { label = "TRANSIENTS", bind = p.transients },
      { label = "STABILIZE",  bind = p.stabilize },
      { label = "DRIVE",      bind = p.drive, color = HOT },
    }
    for i, k in big do
      items[#items + 1] = Knob.new { key = k.label, label = k.label, bind = k.bind, color = k.color, big = true,
                                     x = 140 + (i - 1) * 205, y = 232, w = 150, h = 166 }
    end
    local small: { { label: string, bind: Bindable, bipolar: boolean?, color: string? } } = {
      { label = "BOOM",   bind = p.boom, bipolar = true },
      { label = "BOXY",   bind = p.boxy, bipolar = true },
      { label = "CLICK",  bind = p.click, bipolar = true },
      { label = "OUTPUT", bind = p.output },
      { label = "XOVER",  bind = p.xover, color = COOL },
    }
    for i, k in small do
      items[#items + 1] = Knob.new { key = k.label, label = k.label, bind = k.bind, bipolar = k.bipolar, color = k.color,
                                     x = 140 + (i - 1) * 116, y = 414, w = 100, h = 118 }
    end
    return items
  end,
}

return { Panel = Panel }
