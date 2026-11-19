-- ScriptFX component API (loaded into the sandbox, next to the patch API).
--
--   local Knob = Component.define({ name = "Knob", kind = "knob", size = { 80, 100 },
--                                   render = function(p: KnobProps) return SVG.circle({ ... }) end })
--   Knob.new({ bind = drive_macro, x = 10, y = 20 })      -- an instance
--
-- UI files are modules that init.luau requires; the root instance goes into
-- Patch.new(..., { panel = Panel.new({ ... }) }) and always covers the whole
-- ControlPanel, UI.WIDTH x UI.HEIGHT. `bind` takes a Macro or Param, `source`
-- a metered block or Meters.IN / Meters.OUT.
--
-- render(p) returns SVG strings, nested lists of them, and child instances.
-- SVG parts become this component's own image; children become their own layers
-- (drawn above the parent, cached separately).

local fmt = string.format

local function num(v)
  if v == nil then return "0" end
  if v == math.floor(v) and math.abs(v) < 1e15 then return fmt("%d", v) end
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

-- `transform` (raw SVG transform string), `rotate` ({ deg, cx, cy }) and `filter` (id).
local function transform_filter(o)
  local t = {}
  if o.transform then t[#t + 1] = o.transform end
  if o.rotate then
    local r = o.rotate
    t[#t + 1] = fmt("rotate(%s %s %s)", num(r[1]), num(r[2]), num(r[3]))
  end
  local out = ""
  if #t > 0 then out = fmt('transform="%s"', table.concat(t, " ")) end
  if o.filter then out = out .. fmt(' filter="url(#%s)"', o.filter) end
  return out
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
  a[#a + 1] = transform_filter(o)
  return table.concat(a, " ")
end

local SVG = {}

function SVG.raw(s) return s end

-- A list of parts; may contain strings, lists and child instances.
function SVG.group(list) return list end

function SVG.rect(o)
  return fmt('<rect x="%s" y="%s" width="%s" height="%s" rx="%s" %s/>',
    num(o.x), num(o.y), num(o.w), num(o.h), num(o.rx or 0), common(o))
end

function SVG.circle(o)
  return fmt('<circle cx="%s" cy="%s" r="%s" %s/>', num(o.cx), num(o.cy), num(o.r), common(o))
end

function SVG.line(o)
  o.cap = o.cap or "round"
  return fmt('<line x1="%s" y1="%s" x2="%s" y2="%s" %s/>',
    num(o.x1), num(o.y1), num(o.x2), num(o.y2), common(o))
end

function SVG.path(o)
  return fmt('<path d="%s" %s/>', o.d, common(o))
end

function SVG.text(o)
  return fmt('<text x="%s" y="%s" font-size="%s" font-family="%s" font-weight="%s" fill="%s" text-anchor="%s" letter-spacing="%s"%s %s>%s</text>',
    num(o.x), num(o.y), num(o.size or 12), o.family or "Noto Sans", o.weight or "normal",
    o.fill or "#ffffff", o.anchor or "start", num(o.spacing or 0),
    o.opacity and fmt(' opacity="%s"', num(o.opacity)) or "", transform_filter(o), esc(o.text or ""))
end

-- Image or SVG file from the patch folder (restricted by the shell).
function SVG.image(href, o)
  o = o or {}
  return fmt('<image href="%s" x="%s" y="%s" width="%s" height="%s" preserveAspectRatio="%s"%s/>',
    esc(href), num(o.x), num(o.y), num(o.w), num(o.h), o.fit or "xMidYMid slice",
    o.opacity and fmt(' opacity="%s"', num(o.opacity)) or "")
end
SVG.file = SVG.image

-- Gaussian blur filter; returns id, defs. `pad` = extra filter region around the
-- element as a fraction of its size (default 0.5). Keep it small for big
-- elements: the blurred area is what costs time.
function SVG.blur(std, pad)
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

function SVG.linear(o)
  local id = o.id or new_id("lin")
  local f, t = o.from or { 0, 0 }, o.to or { 0, 1 }
  return id, fmt('<defs><linearGradient id="%s" x1="%s" y1="%s" x2="%s" y2="%s">%s</linearGradient></defs>',
    id, num(f[1]), num(f[2]), num(t[1]), num(t[2]), stops(o.stops))
end

function SVG.radial(o)
  local id = o.id or new_id("rad")
  return id, fmt('<defs><radialGradient id="%s" cx="%s" cy="%s" r="%s">%s</radialGradient></defs>',
    id, num(o.cx or 0.5), num(o.cy or 0.5), num(o.r or 0.5), stops(o.stops))
end

-- Arc from angle `from` to `to` (degrees, 0 = up, clockwise), optional glow.
function SVG.arc(o)
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
    -- Filter region in user space around the whole circle plus the blur's reach. A region
    -- relative to the path (objectBoundingBox) shrank with the arc: a short arc (knob turned
    -- down) cut its own glow off, worst with the wider hover glow.
    local id = new_id("blur")
    local m = o.r + (o.width or 2) / 2 + o.glow * 3
    local defs = fmt('<defs><filter id="%s" filterUnits="userSpaceOnUse" x="%s" y="%s" width="%s" height="%s"><feGaussianBlur stdDeviation="%s"/></filter></defs>',
      id, num(o.cx - m), num(o.cy - m), num(2 * m), num(2 * m), num(o.glow))
    return defs .. fmt('<path d="%s" %s filter="url(#%s)" opacity="0.9"/><path d="%s" %s/>', d, stroke, id, d, stroke)
  end
  return fmt('<path d="%s" %s/>', d, stroke)
end

-- Helpers
local Util = {}
function Util.lerp(a, b, t) return a + (b - a) * t end
function Util.clamp(v, lo, hi) return math.max(lo, math.min(hi, v)) end
function Util.iff(c, a, b) if c then return a else return b end end

-- Component definitions and instances
local Component = {}

function Component.define(def)
  if type(def) ~= "table" then error("Component.define expects a table { name = ..., render = function(p) ... end }", 2) end
  if type(def.render) ~= "function" then error("Component.define: needs a render(p) function", 2) end
  if def.new ~= nil then error("Component.define: 'new' is reserved", 2) end
  def.__is_component = true
  def.new = function(props)
    local name = def.name or "Component"
    if props ~= nil and type(props) ~= "table" then
      error(string.format("%s.new expects a props table, got %s", name, type(props)), 2)
    end
    props = props or {}
    -- bind/source must be objects from init.luau (a string would silently show nothing)
    local bind, source = props.bind, props.source
    if bind ~= nil and not (type(bind) == "table" and type(bind.__ref) == "string"
                            and (bind.__ref:sub(1, 6) == "macro." or bind.__ref:sub(1, 6) == "param.")) then
      error(string.format("%s.new: 'bind' must be a Macro or Param, got %s", name,
        type(bind) == "string" and string.format("the string %q", bind) or type(bind)), 2)
    end
    if source ~= nil and not (type(source) == "table" and type(source.__ref) == "string"
                              and source.__ref:sub(1, 6) == "meter.") then
      error(string.format("%s.new: 'source' must be a metered block (compressor, rider, transient) or Meters.IN / Meters.OUT", name), 2)
    end
    return { __component = def, props = props or {} }
  end
  return def
end

UI = { WIDTH = 840, HEIGHT = 560 }
_G.SVG, _G.Util, _G.Component = SVG, Util, Component
