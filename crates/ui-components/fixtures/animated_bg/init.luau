-- Worst case: a full-size background that animates every frame.
local Bg = Component.define {
  name = "Bg", animated = true,
  render = function(p)
    local t = p.time or 0
    local g, d = SVG.radial { cx = 0.5 + 0.3 * math.sin(t), cy = 0.4, r = 0.6, stops = { { 0, "#d63cff", 0.6 }, { 1, "#d63cff", 0 } } }
    return { d,
      SVG.rect { x = 0, y = 0, w = p.width, h = p.height, fill = "#0d0820" },
      SVG.rect { x = 0, y = 0, w = p.width, h = p.height, fill = "url(#" .. g .. ")" } }
  end,
}
return Patch.new("fixture", { surface = Bg.new {} })
