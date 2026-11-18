-- Junkyard Amp: skeuomorphic, grungy, improvised. Everything is generated
-- procedurally (seeded randomness, so every render is identical).

local fmt = string.format
local rnd = math.random
local function rr(a: number, b: number): number return a + (b - a) * rnd() end

-- Wrap SVG parts in a transform group (SVG only, no child components inside).
local function g(transform: string, parts: Part): Part return { SVG.raw('<g transform="' .. transform .. '">'), parts, SVG.raw('</g>') } end

-- Hand-lettered text: every letter slightly off in position, angle and size.
type MarkerOpts = { advance: number?, anchor: string?, rot: number?, opacity: number? }
local function marker(text: string, x: number, y: number, size: number, color: string, o: MarkerOpts?): Part
  local opts: MarkerOpts = o or {}
  local adv = size * (opts.advance or 0.66)
  local n = utf8.len(text) or 0
  local x0 = x - (opts.anchor == "middle" and adv * n / 2 or 0)
  local parts = {}
  local i = 0
  for _, cp in utf8.codes(text) do
    local ch = utf8.char(cp)
    if ch ~= " " then
      local cx = x0 + i * adv + adv / 2 + rr(-1, 1)
      local cy = y + rr(-1.5, 1.5)
      parts[#parts + 1] = g(fmt("rotate(%.1f %.1f %.1f)", rr(-7, 7) + (opts.rot or 0), cx, cy),
        SVG.text { x = cx, y = cy, text = ch, size = size * rr(0.92, 1.08), weight = "bold", anchor = "middle",
                   fill = color, opacity = opts.opacity or 0.92 })
    end
    i = i + 1
  end
  return parts
end

-- A strip of tape with ripped ends, a little shadow and fibre texture.
type TapeOpts = { rot: number?, fibres: number?, fibre: string?, opacity: number? }
local function tape(x: number, y: number, w: number, h: number, color: string, o: TapeOpts?): Part
  local opts: TapeOpts = o or {}
  local pts = {}
  local steps = 7
  for i = 0, steps do pts[#pts + 1] = fmt("%.1f %.1f", rr(0, 7), i * h / steps) end            -- left edge, top→bottom
  local left = table.concat(pts, " L ")
  pts = {}
  for i = steps, 0, -1 do pts[#pts + 1] = fmt("%.1f %.1f", w - rr(0, 7), i * h / steps) end    -- right edge, bottom→top
  local right = table.concat(pts, " L ")
  local d = "M " .. left .. " L " .. right .. " Z"
  local tex = {}
  for _ = 1, opts.fibres or 10 do
    local yy = rr(2, h - 2)
    tex[#tex + 1] = SVG.line { x1 = rr(6, w * 0.4), y1 = yy, x2 = rr(w * 0.6, w - 6), y2 = yy + rr(-1, 1), stroke = opts.fibre or "#000000", width = 0.6, opacity = 0.06 }
  end
  return g(fmt("translate(%.1f %.1f) rotate(%.1f %.1f %.1f)", x, y, opts.rot or 0, w / 2, h / 2), {
    SVG.path { d = d, fill = "#000000", opacity = 0.28, transform = "translate(2 3)" },
    SVG.path { d = d, fill = color, opacity = opts.opacity or 0.93 },
    tex,
    SVG.line { x1 = 4, y1 = 1.5, x2 = w - 4, y2 = 1.5, stroke = "#ffffff", width = 1, opacity = 0.18 },
  })
end

---------------------------------------------------------------- wood background (static)
local Wood = Component.define {
  name = "Wood",
  render = function(p: Props<{}>)
    math.randomseed(1977)
    local w, h = p.width, p.height
    local planks = { { "#6e4527", "#5a371e" }, { "#634023", "#4f3019" }, { "#76492a", "#5f3a20" } }
    local ph = h / #planks
    local parts = {}
    for i, c in ipairs(planks) do
      local y0 = (i - 1) * ph
      local grad, dgrad = SVG.linear { stops = { { 0, c[1] }, { 1, c[2] } } }
      parts[#parts + 1] = dgrad
      parts[#parts + 1] = SVG.rect { x = 0, y = y0, w = w, h = ph, fill = fmt("url(#%s)", grad) }
      -- grain: long wavy lines
      for _ = 1, 46 do
        local gy = y0 + rr(2, ph - 2)
        local amp = rr(1, 6)
        local d = { fmt("M -10 %.1f", gy) }
        local x = -10
        while x < w + 10 do
          local nx = x + rr(60, 160)
          d[#d + 1] = fmt("Q %.1f %.1f %.1f %.1f", (x + nx) / 2, gy + rr(-amp, amp), nx, gy + rr(-amp * 0.4, amp * 0.4))
          x = nx
        end
        parts[#parts + 1] = SVG.path { d = table.concat(d, " "), stroke = rnd() < 0.8 and "#2e1a0c" or "#a8774d", width = rr(0.6, 2.6), opacity = rr(0.08, 0.32) }
      end
      -- a knot per plank
      local kx, ky = rr(80, w - 80), y0 + rr(30, ph - 30)
      for r = 1, 6 do
        parts[#parts + 1] = SVG.path { d = fmt("M %.1f %.1f a %.1f %.1f 0 1 0 0.1 0", kx - r * 7, ky, r * 7, r * 3.2), stroke = "#2a170a", width = rr(0.8, 1.8), opacity = 0.35 - r * 0.04 }
      end
      parts[#parts + 1] = SVG.circle { cx = kx, cy = ky, r = 4, fill = "#24130a", opacity = 0.7 }
      -- seam + nails
      parts[#parts + 1] = SVG.rect { x = 0, y = y0 - 1.5, w = w, h = 3, fill = "#1a0e06", opacity = 0.75 }
      parts[#parts + 1] = SVG.rect { x = 0, y = y0 + 1.5, w = w, h = 1, fill = "#c39565", opacity = 0.18 }
      for _, nx in ipairs({ 22, w - 22 }) do
        local ny = y0 + ph / 2 + rr(-20, 20)
        local ng, dng = SVG.radial { cx = 0.35, cy = 0.35, r = 0.7, stops = { { 0, "#d9d4c8" }, { 1, "#4a4740" } } }
        parts[#parts + 1] = dng
        parts[#parts + 1] = SVG.circle { cx = nx + 1.5, cy = ny + 2, r = 5.5, fill = "#000000", opacity = 0.35 }
        parts[#parts + 1] = SVG.circle { cx = nx, cy = ny, r = 5, fill = fmt("url(#%s)", ng) }
        parts[#parts + 1] = SVG.circle { cx = nx + 3, cy = ny + 3, r = 7, fill = "#4a1d08", opacity = 0.25 }   -- rust bleed
      end
    end
    -- uneven colour bands across each plank
    for i = 0, #planks - 1 do
      for _ = 1, 6 do
        local by = i * ph + rr(0, ph)
        parts[#parts + 1] = SVG.rect { x = -5, y = by, w = w + 10, h = rr(4, 22), fill = rnd() < 0.5 and "#1f1007" or "#b98a5a", opacity = rr(0.04, 0.1) }
      end
    end
    -- grime: big faint dirt patches
    local dirt, ddirt = SVG.radial { stops = { { 0, "#120903", 0.35 }, { 1, "#120903", 0 } } }
    parts[#parts + 1] = ddirt
    for _ = 1, 14 do
      parts[#parts + 1] = SVG.circle { cx = rr(0, w), cy = rr(0, h), r = rr(30, 110), fill = fmt("url(#%s)", dirt), opacity = rr(0.4, 0.9) }
    end
    -- faded crate stencil, partly worn away
    math.randomseed(5)
    local stencil = {}
    for i, ch in ipairs({ "F", "R", "A", "G", "I", "L", "E" }) do
      stencil[#stencil + 1] = SVG.text { x = 260 + i * 52, y = 545, text = ch, size = 58, weight = "bold", fill = "#9c1f12", opacity = rr(0.12, 0.26) }
    end
    parts[#parts + 1] = g("rotate(-2 420 520)", stencil)
    for _ = 1, 40 do   -- worn spots in the stencil
      parts[#parts + 1] = SVG.circle { cx = rr(300, 640), cy = rr(495, 545), r = rr(1, 5), fill = "#6a4226", opacity = rr(0.3, 0.6) }
    end
    -- dents and drill holes
    for _ = 1, 18 do
      local x, y, r = rr(0, w), rr(0, h), rr(2, 6)
      parts[#parts + 1] = SVG.circle { cx = x, cy = y, r = r, fill = "#1a0d05", opacity = 0.35 }
      parts[#parts + 1] = SVG.arc { cx = x, cy = y, r = r + 0.8, from = 100, to = 260, stroke = "#d7b083", width = 0.8, opacity = 0.35 }
    end
    for _, hpos in ipairs({ { 300, 130 }, { 560, 172 }, { 780, 520 } }) do
      parts[#parts + 1] = SVG.circle { cx = hpos[1], cy = hpos[2], r = 3.5, fill = "#080402", opacity = 0.9 }
      parts[#parts + 1] = SVG.circle { cx = hpos[1], cy = hpos[2], r = 5, stroke = "#2a160a", width = 1.5, opacity = 0.5 }
    end
    math.randomseed(1977 + 99)
    -- scratches and gouges
    for _ = 1, 90 do
      local x, y, a, l = rr(0, w), rr(0, h), rr(0, math.pi), rr(8, 70)
      local light = rnd() < 0.7
      parts[#parts + 1] = SVG.line { x1 = x, y1 = y, x2 = x + l * math.cos(a), y2 = y + l * math.sin(a) * 0.4,
        stroke = light and "#e6c49a" or "#1c0f06", width = rr(0.4, 1.4), opacity = light and rr(0.12, 0.35) or rr(0.2, 0.45) }
    end
    -- coffee ring
    local cx, cy = 690, 430
    parts[#parts + 1] = SVG.circle { cx = cx, cy = cy, r = 46, stroke = "#2b170a", width = 3.5, opacity = 0.32 }
    parts[#parts + 1] = SVG.arc { cx = cx + 4, cy = cy - 2, r = 44, from = 40, to = 230, stroke = "#2b170a", width = 6, opacity = 0.18 }
    local stain, dstain = SVG.radial { stops = { { 0, "#2b170a", 0 }, { 0.8, "#2b170a", 0.08 }, { 1, "#2b170a", 0 } } }
    parts[#parts + 1] = dstain
    parts[#parts + 1] = SVG.circle { cx = cx, cy = cy, r = 48, fill = fmt("url(#%s)", stain) }
    -- cigarette burn
    local burn, dburn = SVG.radial { stops = { { 0, "#050302", 0.95 }, { 0.35, "#1a0d05", 0.7 }, { 1, "#3a1f0c", 0 } } }
    parts[#parts + 1] = dburn
    parts[#parts + 1] = SVG.circle { cx = 128, cy = 470, r = 22, fill = fmt("url(#%s)", burn) }
    parts[#parts + 1] = SVG.circle { cx = 128, cy = 470, r = 6, fill = "#000000", opacity = 0.8 }
    -- vignette
    local vig, dvig = SVG.radial { r = 0.75, stops = { { 0.55, "#000000", 0 }, { 1, "#000000", 0.6 } } }
    parts[#parts + 1] = dvig
    parts[#parts + 1] = SVG.rect { x = 0, y = 0, w = w, h = h, fill = fmt("url(#%s)", vig) }
    -- marker scrawls
    parts[#parts + 1] = marker("MORE!!", 92, 236, 15, "#f1e9d6", { rot = -12, opacity = 0.75 })
    parts[#parts + 1] = SVG.path { d = "M 112 246 Q 135 268 158 268", stroke = "#f1e9d6", width = 2, opacity = 0.7 }
    parts[#parts + 1] = SVG.path { d = "M 150 262 L 160 268 L 151 275", stroke = "#f1e9d6", width = 2, opacity = 0.7 }
    parts[#parts + 1] = marker("careful", 726, 238, 13, "#f1e9d6", { rot = 9, opacity = 0.6, advance = 0.55 })
    return parts
  end,
}

---------------------------------------------------------------- logo: duct tape (static)
local Logo = Component.define {
  name = "Logo",
  size = { 290, 110 },
  render = function(p: Props<{}>)
    math.randomseed(42)
    return {
      tape(4, 8, 270, 58, "#8e8d88", { rot = -4, fibres = 28, fibre = "#ffffff", opacity = 0.96 }),
      marker("JUNKYARD", 140, 50, 30, "#151311", { anchor = "middle", rot = -4 }),
      tape(150, 66, 120, 30, "#e2d3a8", { rot = 5 }),
      marker("AMP v2", 208, 87, 14, "#8a1c10", { anchor = "middle", rot = 5, advance = 0.62 }),
    }
  end,
}

---------------------------------------------------------------- pilot lamp (animated, follows the output)
local Lamp = Component.define {
  name = "Lamp",
  animated = true,
  size = { 90, 90 },
  render = function(p: Props<{}>)
    local lvl = p.level or 0
    local flicker = 0.92 + 0.08 * math.sin((p.time or 0) * 37) * math.sin((p.time or 0) * 13)
    local on = (0.35 + 0.65 * lvl) * flicker
    local glow, dglow = SVG.radial { stops = { { 0, "#ff5a2a", 0.85 * on }, { 1, "#ff3a10", 0 } } }
    local jewel, djewel = SVG.radial { cx = 0.4, cy = 0.35, r = 0.7, stops = { { 0, "#ffd0a0" }, { 0.35, "#ff4a1c" }, { 1, "#5a0c04" } } }
    local bezel, dbezel = SVG.linear { from = { 0, 0 }, to = { 1, 1 }, stops = { { 0, "#f0eee8" }, { 0.5, "#8c8a84" }, { 1, "#3c3a36" } } }
    return {
      dglow, djewel, dbezel,
      SVG.circle { cx = 45, cy = 45, r = 44, fill = fmt("url(#%s)", glow) },
      SVG.circle { cx = 47, cy = 48, r = 19, fill = "#000000", opacity = 0.4 },
      SVG.circle { cx = 45, cy = 45, r = 18, fill = fmt("url(#%s)", bezel) },
      SVG.circle { cx = 45, cy = 45, r = 13, fill = fmt("url(#%s)", jewel), opacity = 0.45 + 0.55 * on },
      SVG.path { d = "M 45 32 L 52 41 L 45 50 L 38 41 Z", fill = "#ffffff", opacity = 0.12 + 0.2 * on },
      SVG.circle { cx = 40, cy = 39, r = 3, fill = "#ffffff", opacity = 0.7 },
    }
  end,
}

---------------------------------------------------------------- knob: static dial + dynamic chicken head
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

---------------------------------------------------------------- panel
-- patch.luau passes the three macros in.
export type PanelProps = { gain: Macro, presence: Macro, master: Macro }

local Panel = Component.define {
  name = "Panel",
  render = function(p: Props<PanelProps>)
    return {
      Wood.new {},
      Logo.new { x = 30, y = 26 },
      Lamp.new { source = Meters.OUT, x = 718, y = 40 },
      ChickenKnob.new { key = "gain",     label = "GAIN",     bind = p.gain,     seed = 11, tilt = -3, x = 130, y = 230 },
      ChickenKnob.new { key = "presence", label = "PRESENCE", bind = p.presence, seed = 23, tilt = 2,  x = 330, y = 230 },
      ChickenKnob.new { key = "master",   label = "MASTER",   bind = p.master,   seed = 37, tilt = -1, x = 530, y = 230 },
    }
  end,
}

return { Panel = Panel }
