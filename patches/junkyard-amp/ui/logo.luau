-- The logo: duct tape and masking tape with marker lettering.
local craft = require("./craft")
local tape, marker = craft.tape, craft.marker

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

return Logo
