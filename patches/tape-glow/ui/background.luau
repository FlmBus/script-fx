-- The static background: panel, deck window, tape packs, guides and the head.
local style = require("./style")
local fmt = string.format
local BG, PANEL, EDGE, TEXT, DIM, REELS, REEL_R = style.BG, style.PANEL, style.EDGE, style.TEXT, style.DIM, style.REELS, style.REEL_R

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

return Background
