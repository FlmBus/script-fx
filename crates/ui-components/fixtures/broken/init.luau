-- Faulty components must only break themselves.
local Ok = Component.define {
  name = "Ok", size = { 180, 60 },
  render = function(p)
    return { SVG.rect { x = 0, y = 0, w = p.width, h = p.height, rx = 6, fill = "#24402c" },
             SVG.text { x = 10, y = 35, text = "fine", fill = "#b8f0c8" } }
  end,
}
local Throws = Component.define { name = "Throws", size = { 180, 60 }, render = function(p) error("boom: " .. p.nope.field) end }
local Loops = Component.define { name = "Loops", size = { 180, 60 }, render = function() while true do end end }
local BadSvg = Component.define { name = "BadSvg", size = { 180, 60 }, render = function() return "<rect x=" end }
local Hog = Component.define {
  name = "Hog", size = { 180, 60 },
  render = function()
    local t = {}
    for i = 1, 1e9 do t[i] = string.rep("x", 1024) .. i end  -- unique: Luau interns all strings
  end,
}

local Surface = Component.define {
  name = "Surface",
  render = function(p)
    return {
      SVG.rect { x = 0, y = 0, w = p.width, h = p.height, fill = "#1d1f23" },
      Ok.new { x = 10, y = 10 }, Throws.new { x = 200, y = 10 }, Loops.new { x = 390, y = 10 },
      BadSvg.new { x = 10, y = 80 }, Hog.new { x = 200, y = 80 }, Ok.new { x = 390, y = 80 },
    }
  end,
}
return Patch.new("fixture", { surface = Surface.new {} })
