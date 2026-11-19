-- A chicken-head knob: static dial (scale, nut, taped label) + the moving head.
local craft = require("./craft")
local fmt = string.format
local rr, g, marker, tape = craft.rr, craft.g, craft.marker, craft.tape

local CX, CY = 90, 92

local Dial = Component.define {
  name = "Dial",
  render = function(p: Props<{ label: string, seed: number, tilt: number }>)
    math.randomseed(p.seed)
    local parts = {}
    for i = 0, 10 do
      local a = math.rad(-135 + 27 * i)
      local r0, r1 = 52, i % 5 == 0 and 64 or 59
      local j = rr(-0.03, 0.03)
      parts[#parts + 1] = SVG.line { x1 = CX + r0 * math.sin(a + j), y1 = CY - r0 * math.cos(a + j), x2 = CX + r1 * math.sin(a), y2 = CY - r1 * math.cos(a),
        stroke = "#f1e9d6", width = rr(1.6, 2.6), opacity = 0.8 }
      parts[#parts + 1] = marker(tostring(i), CX + 74 * math.sin(a), CY - 74 * math.cos(a) + 5, 13, "#f1e9d6", { anchor = "middle", opacity = 0.85 })
    end
    -- the pot's mounting nut (metal) under the knob
    local nut, dnut = SVG.radial { cx = 0.4, cy = 0.35, r = 0.7, stops = { { 0, "#bdb8ad" }, { 1, "#3b3833" } } }
    parts[#parts + 1] = dnut
    parts[#parts + 1] = SVG.circle { cx = CX, cy = CY, r = 31, fill = fmt("url(#%s)", nut), stroke = "#1a1815", width = 1 }
    -- label on ripped masking tape
    parts[#parts + 1] = tape(CX - 62, 178, 124, 34, "#e4d6ad", { rot = p.tilt })
    parts[#parts + 1] = marker(p.label, CX, 201, 17, "#1a1612", { anchor = "middle", rot = p.tilt })
    return parts
  end,
}

-- Chicken-head pointer, drawn pointing up, rotated by the value.
local HEAD = "M 0 -56 C 6 -56 8 -46 10 -33 C 12 -20 27 -15 27 0 C 27 17 14 28 0 28 C -14 28 -27 17 -27 0 C -27 -15 -12 -20 -10 -33 C -8 -46 -6 -56 0 -56 Z"

local Head = Component.define {
  name = "Head",
  render = function(p: Props<{ v: number?, text: string?, show: boolean, active: boolean }>)
    local deg = -135 + 270 * (p.v or 0)
    local body, dbody = SVG.linear { from = { 0, 0 }, to = { 1, 1 }, stops = { { 0, "#4a403a" }, { 0.45, "#1d1714" }, { 1, "#070505" } } }
    local parts = {
      dbody,
      -- shadow falls down-right, independent of the rotation (two offset copies
      -- instead of a blur filter: cheap enough to redraw on every value change)
      g(fmt("translate(%d %d)", CX + 7, CY + 10), g(fmt("rotate(%.1f)", deg), SVG.path { d = HEAD, fill = "#000000", opacity = 0.18 })),
      g(fmt("translate(%d %d)", CX + 4, CY + 6), g(fmt("rotate(%.1f)", deg), SVG.path { d = HEAD, fill = "#000000", opacity = 0.32 })),
      g(fmt("translate(%d %d) rotate(%.1f)", CX, CY, deg), {
        SVG.path { d = HEAD, fill = fmt("url(#%s)", body), stroke = p.active and "#d9472b" or "#000000", width = p.active and 1.6 or 1 },
        SVG.line { x1 = 0, y1 = -50, x2 = 0, y2 = -22, stroke = "#f3eee2", width = 3, opacity = 0.9 },
        SVG.path { d = "M -20 -6 C -18 -16 -10 -18 -8 -26", stroke = "#ffffff", width = 2, opacity = 0.18 },
        SVG.circle { cx = 0, cy = 0, r = 10, fill = "#2c2521", stroke = "#000000", width = 1 },
        SVG.circle { cx = -3, cy = -3, r = 3, fill = "#ffffff", opacity = 0.12 },
      }),
    }
    if p.show then
      parts[#parts + 1] = tape(CX + 22, CY - 70, 64, 24, "#f2ecd8", { rot = 6 })
      parts[#parts + 1] = marker(p.text or "", CX + 54, CY - 53, 12, "#8a1c10", { anchor = "middle", rot = 6, advance = 0.58 })
    end
    return parts
  end,
}

local ChickenKnob = Component.define {
  name = "ChickenKnob",
  kind = "knob",
  size = { 180, 220 },
  render = function(p: Props<{ label: string, seed: number, tilt: number }>)
    return {
      Dial.new { label = p.label, seed = p.seed, tilt = p.tilt },
      Head.new { v = p.value, text = p.display, show = p.dragging or p.hover, active = p.dragging },
    }
  end,
}

return ChickenKnob
