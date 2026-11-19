-- The pilot lamp, animated: glows and flickers with the output level.
local fmt = string.format

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

return Lamp
