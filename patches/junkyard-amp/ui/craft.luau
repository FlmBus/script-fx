-- Junkyard Amp's hand-made look: seeded randomness, hand-lettered marker text and ripped tape.
-- Shared by the components (each one seeds the random generator itself, so renders are
-- identical every time).

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

return { rr = rr, g = g, marker = marker, tape = tape }
