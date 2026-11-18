-- Neon Saturator: the whole control panel as reactive components.
-- Each component's render(p) returns SVG; the shell caches one image per
-- component and re-renders only when that component's inputs change.

local fmt = string.format

---------------------------------------------------------------- background (static)
local Background = Component.define {
  name = "Background",
  render = function(p: Props<{}>)
    local w, h = p.width, p.height
    local g1, d1 = SVG.radial { cx = 0.15, cy = 0.05, r = 0.55, stops = { { 0, "#d63cff", 0.55 }, { 1, "#d63cff", 0 } } }
    local g2, d2 = SVG.radial { cx = 0.9, cy = 0.6, r = 0.5, stops = { { 0, "#2878ff", 0.45 }, { 1, "#2878ff", 0 } } }
    local g3, d3 = SVG.radial { cx = 0.5, cy = 1.0, r = 0.6, stops = { { 0, "#781ec8", 0.4 }, { 1, "#781ec8", 0 } } }
    local parts = {
      d1, d2, d3,
      SVG.rect { x = 0, y = 0, w = w, h = h, fill = "#0d0820" },
      SVG.rect { x = 0, y = 0, w = w, h = h, fill = fmt("url(#%s)", g1) },
      SVG.rect { x = 0, y = 0, w = w, h = h, fill = fmt("url(#%s)", g2) },
      SVG.rect { x = 0, y = 0, w = w, h = h, fill = fmt("url(#%s)", g3) },
    }
    -- Glowing frame without a blur filter (a blur over the whole panel is slow):
    -- stacked strokes with falling opacity.
    for i = 6, 1, -1 do
      parts[#parts + 1] = SVG.rect { x = 10, y = 10, w = w - 20, h = h - 20, rx = 14, stroke = "#dc5aff", width = i * 2, opacity = 0.05 }
    end
    parts[#parts + 1] = SVG.rect { x = 10, y = 10, w = w - 20, h = h - 20, rx = 14, stroke = "#e88cff", width = 1.2, opacity = 0.8 }
    return parts
  end,
}

---------------------------------------------------------------- logo (static)
local Logo = Component.define {
  name = "Logo",
  render = function(p: Props<{}>)
    local cx = p.width / 2
    local a, da = SVG.linear { stops = { { 0, "#ffd27a" }, { 1, "#ff5fa8" } } }
    local b, db = SVG.linear { stops = { { 0, "#ff9ad5" }, { 1, "#c04dff" } } }
    local glow, dglow = SVG.blur(5, 0.1)
    return {
      da, db, dglow,
      SVG.text { x = cx, y = 60, text = "NEON", size = 52, weight = "bold", anchor = "middle", spacing = 6, fill = "#ff5fa8", opacity = 0.7, filter = glow },
      SVG.text { x = cx, y = 60, text = "NEON", size = 52, weight = "bold", anchor = "middle", spacing = 6, fill = fmt("url(#%s)", a) },
      SVG.text { x = cx, y = 104, text = "SATURATOR", size = 36, weight = "bold", anchor = "middle", spacing = 5, fill = fmt("url(#%s)", b) },
      SVG.rect { x = cx - 120, y = 124, w = 240, h = 28, rx = 14, fill = "#140828", stroke = "#dc78ff", width = 1, opacity = 0.9 },
      SVG.text { x = cx, y = 143, text = "HARMONIC MULTI-FX", size = 11, anchor = "middle", spacing = 3, fill = "#f2e9ff" },
      SVG.circle { cx = cx - 102, cy = 138, r = 3.5, fill = "#ff4fb0" },
      SVG.circle { cx = cx + 102, cy = 138, r = 3.5, fill = "#3fd7ff" },
    }
  end,
}

---------------------------------------------------------------- animated LED
local Pulse = Component.define {
  name = "Pulse",
  animated = true,
  size = { 24, 24 },
  render = function(p: Props<{}>)
    local t = p.time or 0
    local a = 0.55 + 0.45 * math.sin(t * 4)
    local glow, d = SVG.blur(2 + 2 * a)
    return { d,
      SVG.circle { cx = 12, cy = 12, r = 6, fill = "#3fd7ff", opacity = a, filter = glow },
      SVG.circle { cx = 12, cy = 12, r = 3.5, fill = "#d9f7ff" } }
  end,
}

---------------------------------------------------------------- knob
-- Split into a static body (rendered once) and the moving parts (re-rendered
-- on every value change). The knob itself only composes the two.
local KnobBody = Component.define {
  name = "KnobBody",
  render = function(p: Props<{ label: string? }>)
    local body, dbody = SVG.radial { cx = 0.35, cy = 0.3, r = 0.75, stops = { { 0, "#4a3f62" }, { 1, "#1b1426" } } }
    return {
      dbody,
      SVG.arc { cx = 55, cy = 52, r = 40, from = -135, to = 135, stroke = "#ffffff", width = 5, opacity = 0.08 },
      SVG.circle { cx = 55, cy = 52, r = 30, fill = fmt("url(#%s)", body), stroke = "#000000", width = 1, opacity = 0.95 },
      SVG.rect { x = 6, y = 106, w = 98, h = 24, rx = 6, fill = "#0a0616", stroke = "#ffffff", width = 1, opacity = 0.85 },
      SVG.text { x = 14, y = 122, text = p.label or "", size = 9, weight = "bold", spacing = 1, fill = "#f2e9ff" },
    }
  end,
}

local KnobValue = Component.define {
  name = "KnobValue",
  render = function(p: Props<{ v: number?, text: string?, color: string, glow: number }>)
    local v, c = p.v or 0, p.color
    return {
      SVG.arc { cx = 55, cy = 52, r = 40, from = -135, to = -135 + 270 * v, stroke = c, width = 5, glow = p.glow, },
      SVG.rect { x = 53, y = 25, w = 4, h = 14, rx = 2, fill = c, rotate = { -135 + 270 * v, 55, 52 } },
      SVG.text { x = 96, y = 122, text = p.text or "", size = 9, weight = "bold", anchor = "end", fill = c },
    }
  end,
}

local NeonKnob = Component.define {
  name = "NeonKnob",
  kind = "knob",
  size = { 110, 140 },
  render = function(p: Props<{ label: string, color: string? }>)
    local glow = p.dragging and 7 or (p.hover and 5 or 3)
    return {
      KnobBody.new { label = p.label },
      KnobValue.new { v = p.value, text = p.display, color = p.color or "#ff4fb0", glow = glow },
    }
  end,
}

---------------------------------------------------------------- meter
local MeterFrame = Component.define {
  name = "MeterFrame",
  render = function(p: Props<{ label: string, color: string }>)
    local w, h = p.width, p.height
    local top, bar = 56, h - 86
    local parts = {
      SVG.rect { x = 0, y = 0, w = w, h = h, rx = 8, fill = "#080414", stroke = "#ffffff", width = 1, opacity = 0.85 },
      SVG.text { x = w / 2, y = 20, text = p.label, size = 11, weight = "bold", anchor = "middle", fill = p.color },
    }
    for _, db in ipairs({ 0, -6, -12, -18, -24, -40 }) do
      parts[#parts + 1] = SVG.text { x = 8, y = top + bar * (-db / 60) + 3, text = tostring(db), size = 9, fill = "#c9b8e8" }
    end
    for _, x in ipairs({ 36, 46 }) do
      parts[#parts + 1] = SVG.rect { x = x, y = top, w = 6, h = bar, rx = 3, fill = "#ffffff", opacity = 0.06 }
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

---------------------------------------------------------------- toggle (local state)
local Toggle = Component.define {
  name = "Toggle",
  kind = "button",
  size = { 140, 32 },
  on_click = function(state) state.on = not state.on end,
  render = function(p: Props<{ label: string, on_text: string?, default_on: boolean?, color: string? }>)
    local on = p.state.on
    if on == nil then on = p.default_on end
    local c = p.color or "#3fd7ff"
    local glow, d = SVG.blur(3)
    return { d,
      SVG.rect { x = 0.5, y = 0.5, w = p.width - 1, h = p.height - 1, rx = 7, fill = "#0a0616", stroke = p.hover and c or "#ffffff", width = 1, opacity = 0.9 },
      on and SVG.circle { cx = 18, cy = 16, r = 5, fill = c, filter = glow } or "",
      SVG.circle { cx = 18, cy = 16, r = 4, fill = on and c or "#3a3448" },
      SVG.text { x = 32, y = 20, text = p.label, size = 10, weight = "bold", spacing = 1, fill = "#f2e9ff" },
      SVG.text { x = p.width - 12, y = 20, text = on and (p.on_text or "ON") or "OFF", size = 10, weight = "bold", anchor = "end", fill = on and c or "#8a80a0" },
    }
  end,
}

---------------------------------------------------------------- tabs (composition + state passed down)
type TabProps = { label: string, index: number, group: { [string]: any } }

local TabButton = Component.define {
  name = "TabButton",
  kind = "button",
  on_click = function(_, props: TabProps) props.group.tab = props.index end,
  render = function(p: Props<TabProps>)
    local active = (p.group.tab or 1) == p.index
    return {
      SVG.rect { x = 0, y = 0, w = p.width, h = p.height, rx = 5, fill = active and "#2a1450" or "#00000000" },
      SVG.text { x = p.width / 2, y = p.height / 2 + 4, text = p.label, size = 10, weight = "bold", anchor = "middle", spacing = 2,
                 fill = active and "#ffffff" or (p.hover and "#e8d8ff" or "#9a8cc0") },
    }
  end,
}

local TabBar = Component.define {
  name = "TabBar",
  render = function(p: Props<{ tabs: { string }, group: { [string]: any } }>)
    local w = p.width / #p.tabs
    local parts = { SVG.rect { x = 0, y = 0, w = p.width, h = p.height, rx = 7, fill = "#0a0616", stroke = "#ffffff", width = 1, opacity = 0.6 } }
    for i, label in ipairs(p.tabs) do
      parts[#parts + 1] = TabButton.new { key = label, label = label, index = i, group = p.group, x = (i - 1) * w + 2, y = 2, w = w - 4, h = p.height - 4 }
    end
    return parts
  end,
}

---------------------------------------------------------------- spectrum display
local Spectrum = Component.define {
  name = "Spectrum",
  render = function(p: Props<{}>)
    local s: { number } = p.series or {}
    local w, h = p.width, p.height
    local pts = {}
    for i, v in ipairs(s) do
      pts[#pts + 1] = fmt("%s%.1f %.1f", i == 1 and "M" or "L", (i - 1) / math.max(1, #s - 1) * w, h - 6 - v * (h - 16))
    end
    local line = table.concat(pts, " ")
    local area, darea = SVG.linear { stops = { { 0, "#ff4fb0", 0.45 }, { 1, "#3fd7ff", 0.05 } } }
    return { darea,
      SVG.rect { x = 0, y = 0, w = w, h = h, rx = 8, fill = "#080414", stroke = "#ffffff", width = 1, opacity = 0.7 },
      #s > 1 and SVG.path { d = line .. fmt(" L%.1f %.1f L0 %.1f Z", w, h, h), fill = fmt("url(#%s)", area) } or "",
      #s > 1 and SVG.path { d = line, stroke = "#ff9ad5", width = 1.5 } or "",
      SVG.text { x = 10, y = 18, text = "OUTPUT SPECTRUM", size = 9, spacing = 2, fill = "#c9b8e8" },
    }
  end,
}

---------------------------------------------------------------- panel (root)
-- patch.luau passes the four macros in.
export type PanelProps = { tape: Macro, tube: Macro, diode: Macro, soft_clip: Macro }

local Panel = Component.define {
  name = "Panel",
  render = function(p: Props<PanelProps>)
    local tab = p.state.tab or 1
    local items = {
      Background.new {},
      Logo.new { x = 220, y = 104, w = 400, h = 170 },
      Pulse.new { x = 652, y = 210 },
      Meter.new { source = Meters.IN, label = "IN", color = "#3fd7ff", x = 26, y = 96, w = 62, h = 440 },
      Meter.new { source = Meters.OUT, label = "OUT", color = "#ffc14d", x = 752, y = 96, w = 62, h = 440 },
      TabBar.new { tabs = { "MAIN", "ADVANCED" }, group = p.state, x = 330, y = 290, w = 180, h = 28 },
      Toggle.new { key = "os", label = "OVERSAMPLE", on_text = "4X", default_on = true, color = "#3fd7ff", x = 270, y = 496 },
      Toggle.new { key = "ag", label = "AUTO-GAIN", default_on = true, color = "#ffc14d", x = 430, y = 496 },
    }
    if tab == 1 then
      for _, k in {
        { label = "TAPE",      macro = p.tape,      color = "#ffb43d", x = 110 },
        { label = "TUBE",      macro = p.tube,      color = "#ff4fb0", x = 230 },
        { label = "DIODE",     macro = p.diode,     color = "#e05bff", x = 500 },
        { label = "SOFT-CLIP", macro = p.soft_clip, color = "#3fd7ff", x = 620 },
      } do
        items[#items + 1] = NeonKnob.new { key = k.label, label = k.label, bind = k.macro, color = k.color, x = k.x, y = 336 }
      end
    else
      items[#items + 1] = Spectrum.new { series = "spectrum", x = 120, y = 336, w = 600, h = 140 }
    end
    return items
  end,
}

return { Panel = Panel }
