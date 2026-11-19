-- The glow at the tape head, follows the output level.
local style = require("./style")
local fmt = string.format
local ACCENT = style.ACCENT

local HeadGlow = Component.define {
  name = "HeadGlow",
  render = function(p: Props<{}>)
    local lvl = Util.clamp((p.level or 0) * 1.3 - 0.2, 0, 1)
    local g, dg = SVG.radial { stops = { { 0, ACCENT, 0.85 * lvl }, { 1, ACCENT, 0 } } }
    return { dg, SVG.circle { cx = p.width / 2, cy = p.height / 2, r = p.width / 2, fill = fmt("url(#%s)", g) } }
  end,
}

return HeadGlow
