-- The wood background: planks, grain, knots, nails, grime, stencil, scratches, stains, scrawls.
local craft = require("./craft")
local fmt = string.format
local rr, g, marker = craft.rr, craft.g, craft.marker

local rnd = math.random

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

return Wood
