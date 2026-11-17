-- Faulty components must only break themselves.
local Ok = component {
  name = "Ok", size = { 180, 60 },
  render = function(p)
    return { svg.rect { x = 0, y = 0, w = p.width, h = p.height, rx = 6, fill = "#24402c" },
             svg.text { x = 10, y = 35, text = "fine", fill = "#b8f0c8" } }
  end,
}
local Throws = component { name = "Throws", size = { 180, 60 }, render = function(p) error("boom: " .. p.nope.field) end }
local Loops = component { name = "Loops", size = { 180, 60 }, render = function() while true do end end }
local BadSvg = component { name = "BadSvg", size = { 180, 60 }, render = function() return "<rect x=" end }
local Hog = component {
  name = "Hog", size = { 180, 60 },
  render = function()
    local t = {}
    for i = 1, 1e9 do t[i] = string.rep("x", 1024) end
  end,
}

local Panel = component {
  name = "Panel",
  render = function(p)
    return {
      svg.rect { x = 0, y = 0, w = p.width, h = p.height, fill = "#1d1f23" },
      Ok { x = 10, y = 10 }, Throws { x = 200, y = 10 }, Loops { x = 390, y = 10 },
      BadSvg { x = 10, y = 80 }, Hog { x = 200, y = 80 }, Ok { x = 390, y = 80 },
    }
  end,
}
return { width = 580, height = 150, root = Panel {} }
