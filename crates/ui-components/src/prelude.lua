-- ScriptFX component API (loaded into the sandbox before ui.lua).
--
--   local Knob = component { name = "Knob", kind = "knob", size = { 80, 100 },
--                            render = function(p) return svg.circle{...} end }
--   Knob { bind = "macro.drive", x = 10, y = 20 }      -- an instance (descriptor)
--
-- render(p) returns SVG strings, nested lists of them, and child instances.
-- SVG parts become this component's own image; children become their own layers
-- (drawn above the parent, cached separately).

local fmt = string.format

local function num(v)
  if v == nil then return "0" end
  if math.type(v) == "integer" then return tostring(v) end
  return fmt("%.2f", v)
end

local function esc(s)
  s = tostring(s)
  s = s:gsub("&", "&amp;"):gsub("<", "&lt;"):gsub(">", "&gt;"):gsub('"', "&quot;")
  return s
end

local uid = 0
local function new_id(prefix)
  uid = uid + 1
  return prefix .. uid
end

-- Shared presentation attributes.
local function common(o)
  local a = {}
  a[#a + 1] = fmt('fill="%s"', o.fill or "none")
  if o.stroke then
    a[#a + 1] = fmt('stroke="%s" stroke-width="%s"', o.stroke, num(o.width or 1))
    if o.cap then a[#a + 1] = fmt('stroke-linecap="%s"', o.cap) end
  end
  if o.opacity then a[#a + 1] = fmt('opacity="%s"', num(o.opacity)) end
  if o.rotate then
    local r = o.rotate
    a[#a + 1] = fmt('transform="rotate(%s %s %s)"', num(r[1]), num(r[2]), num(r[3]))
  end
  if o.filter then a[#a + 1] = fmt('filter="url(#%s)"', o.filter) end
  return table.concat(a, " ")
end

svg = {}

function svg.raw(s) return s end

-- A list of parts; may contain strings, lists and child instances.
function svg.group(list) return list end

function svg.rect(o)
  return fmt('<rect x="%s" y="%s" width="%s" height="%s" rx="%s" %s/>',
    num(o.x), num(o.y), num(o.w), num(o.h), num(o.rx or 0), common(o))
end

function svg.circle(o)
  return fmt('<circle cx="%s" cy="%s" r="%s" %s/>', num(o.cx), num(o.cy), num(o.r), common(o))
end

function svg.line(o)
  o.cap = o.cap or "round"
  return fmt('<line x1="%s" y1="%s" x2="%s" y2="%s" %s/>',
    num(o.x1), num(o.y1), num(o.x2), num(o.y2), common(o))
end

function svg.path(o)
  return fmt('<path d="%s" %s/>', o.d, common(o))
end

function svg.text(o)
  return fmt('<text x="%s" y="%s" font-size="%s" font-family="%s" font-weight="%s" fill="%s" text-anchor="%s" letter-spacing="%s"%s>%s</text>',
    num(o.x), num(o.y), num(o.size or 12), o.family or "Noto Sans", o.weight or "normal",
    o.fill or "#ffffff", o.anchor or "start", num(o.spacing or 0),
    o.opacity and fmt(' opacity="%s"', num(o.opacity)) or "", esc(o.text or ""))
end

-- Image or SVG file from the patch folder (restricted by the shell).
function svg.image(href, o)
  o = o or {}
  return fmt('<image href="%s" x="%s" y="%s" width="%s" height="%s" preserveAspectRatio="%s"%s/>',
    esc(href), num(o.x), num(o.y), num(o.w), num(o.h), o.fit or "xMidYMid slice",
    o.opacity and fmt(' opacity="%s"', num(o.opacity)) or "")
end
svg.file = svg.image

-- Gaussian blur filter; returns id, defs. `pad` = extra filter region around the
-- element as a fraction of its size (default 0.5). Keep it small for big
-- elements: the blurred area is what costs time.
function svg.blur(std, pad)
  pad = pad or 0.5
  local id = new_id("blur")
  return id, fmt('<defs><filter id="%s" x="%s" y="%s" width="%s" height="%s"><feGaussianBlur stdDeviation="%s"/></filter></defs>',
    id, num(-pad), num(-pad), num(1 + 2 * pad), num(1 + 2 * pad), num(std))
end

-- Gradients: stops = { { 0, "#fff" }, { 1, "#000", 0.5 } } (offset, colour, opacity)
local function stops(list)
  local s = {}
  for _, st in ipairs(list) do
    s[#s + 1] = fmt('<stop offset="%s" stop-color="%s" stop-opacity="%s"/>', num(st[1]), st[2], num(st[3] or 1))
  end
  return table.concat(s)
end

function svg.linear(o)
  local id = o.id or new_id("lin")
  local f, t = o.from or { 0, 0 }, o.to or { 0, 1 }
  return id, fmt('<defs><linearGradient id="%s" x1="%s" y1="%s" x2="%s" y2="%s">%s</linearGradient></defs>',
    id, num(f[1]), num(f[2]), num(t[1]), num(t[2]), stops(o.stops))
end

function svg.radial(o)
  local id = o.id or new_id("rad")
  return id, fmt('<defs><radialGradient id="%s" cx="%s" cy="%s" r="%s">%s</radialGradient></defs>',
    id, num(o.cx or 0.5), num(o.cy or 0.5), num(o.r or 0.5), stops(o.stops))
end

-- Arc from angle `from` to `to` (degrees, 0 = up, clockwise), optional glow.
function svg.arc(o)
  local a0, a1 = o.from, o.to
  if math.abs(a1 - a0) < 0.05 then return "" end
  if a1 < a0 then a0, a1 = a1, a0 end
  local function pt(a)
    local r = math.rad(a)
    return o.cx + o.r * math.sin(r), o.cy - o.r * math.cos(r)
  end
  local x0, y0 = pt(a0)
  local x1, y1 = pt(a1)
  local large = (a1 - a0) > 180 and 1 or 0
  local d = fmt("M %s %s A %s %s 0 %d 1 %s %s", num(x0), num(y0), num(o.r), num(o.r), large, num(x1), num(y1))
  local stroke = fmt('stroke="%s" stroke-width="%s" stroke-linecap="%s" fill="none"%s', o.stroke or "#fff", num(o.width or 2), o.cap or "round",
    o.opacity and fmt(' opacity="%s"', num(o.opacity)) or "")
  if o.glow and o.glow > 0 then
    -- Filter region just large enough for the glow (relative to the arc's size).
    local id, defs = svg.blur(o.glow, math.min(0.5, (o.glow * 2.5) / (2 * o.r)))
    return defs .. fmt('<path d="%s" %s filter="url(#%s)" opacity="0.9"/><path d="%s" %s/>', d, stroke, id, d, stroke)
  end
  return fmt('<path d="%s" %s/>', d, stroke)
end

-- Helpers
function lerp(a, b, t) return a + (b - a) * t end
function clamp(v, lo, hi) return math.max(lo, math.min(hi, v)) end
function iff(c, a, b) if c then return a else return b end end

-- Component definitions and instances
local Component = {}
Component.__call = function(def, props)
  return { __component = def, props = props or {} }
end

function component(def)
  assert(type(def.render) == "function", "component needs a render(p) function")
  def.__is_component = true
  return setmetatable(def, Component)
end

function child(def, props) return def(props) end
