-- ScriptFX patch API (loaded into the sandbox before init.luau).
--
--   local ui = require("./ui")
--   local sat = Blocks.Drive.new("sat", { amount = 6 })            -- its knobs: sat.params.amount, sat.params.mix
--   local amount = Macro.new("amount", "Amount"):drives(sat.params.amount, { from = 0, to = 24 })
--   local chain = Chain.new({ sat })
--   return Patch.new("My Patch", { chain = chain, macros = { amount }, surface = ui.Surface.new({ amount = amount }) })
--
-- Constructors take the identity positionally (id, display name) and every
-- setting by name in an options table. A block's numeric settings are its knobs (Params,
-- `block.params.<name>`); the options give their start values. Everything is checked here, with the
-- error pointing at the offending line. Types for the editor: patches/.types/scriptfx.d.luau.
-- The chunk returns the function that turns a Patch into plain data for the Rust side.
--
-- Params, macros and metered blocks carry `__ref` ("param.<id>", "macro.<id>",
-- "meter.<id>"): that's how the UI engine and the shell identify what a
-- component's `bind` / `source` points at.

local fmt = string.format

-- error() at `level` = the stack level of the caller to blame (1 = the function calling fail).
local function fail(msg, level) error(msg, level + 1) end

local function is(v, class) return type(v) == "table" and getmetatable(v) == class end

-- Classes are locals (exported as globals at the end), so patch code that
-- shadows a global can't break the checks below.
local Param, Macro, Chain, Patch, Blocks = {}, {}, {}, {}, {}
local Font, Image = {}, {}

local Block = {}
Block.__index = Block

local function describe(v)
  if is(v, Param) then return "a Param" end
  if is(v, Macro) then return "a Macro" end
  if is(v, Chain) then return "a Chain" end
  if is(v, Block) then return "a block" end
  if is(v, Font) then return "a Font" end
  if is(v, Image) then return "an Image" end
  if type(v) == "string" then return fmt("the string %q", v) end
  return type(v)
end

-- A plain list: a table without metatable whose keys are exactly 1..n.
local function is_list(v)
  if type(v) ~= "table" or getmetatable(v) ~= nil then return false end
  local n = 0
  for _ in pairs(v) do n = n + 1 end
  return n == #v
end

local function list_of(check)
  return function(v)
    if not is_list(v) or #v == 0 then return false end
    for _, x in ipairs(v) do
      if not check(x) then return false end
    end
    return true
  end
end


local NOTE = { c = 0, d = 2, e = 4, f = 5, g = 7, a = 9, b = 11 }

-- "A4" -> 440, "C#3", "Bb1"; nil if it isn't a note name.
local function note_hz(name)
  local letter, acc, oct = name:match("^([A-Ga-g])([#b]?)(-?%d)$")
  if not letter then return nil end
  local midi = 12 * (tonumber(oct) + 1) + NOTE[letter:lower()] + (acc == "#" and 1 or acc == "b" and -1 or 0)
  return 440 * 2 ^ ((midi - 69) / 12)
end

-- "1/8" -> 0.5 beats (quarter notes); "." = dotted, "t" = triplet; nil if invalid.
local function sync_beats(s)
  local num, den, mod = s:match("^(%d+)/(%d+)([%.t]?)$")
  if not num or tonumber(den) == 0 or tonumber(num) == 0 then return nil end
  local beats = 4 * tonumber(num) / tonumber(den)
  if mod == "." then beats = beats * 1.5 elseif mod == "t" then beats = beats * 2 / 3 end
  return beats
end

-- Samples a transfer curve (function or list) into a plain list for the DSP side.
local CURVE_POINTS = 1025
local function sample_curve(v, what, level)
  if type(v) ~= "function" then return table.clone(v) end
  local out = table.create(CURVE_POINTS)
  for i = 1, CURVE_POINTS do
    local x = -1 + 2 * (i - 1) / (CURVE_POINTS - 1)
    local ok, y = pcall(v, x)
    if not ok then fail(fmt("%s: curve(%g) failed: %s", what, x, tostring(y)), level + 1) end
    if type(y) ~= "number" or y ~= y or math.abs(y) == math.huge then
      fail(fmt("%s: curve(%g) must return a finite number, got %s", what, x, tostring(y)), level + 1)
    end
    out[i] = y
  end
  return out
end

local KINDS = {
  number   = { function(v) return type(v) == "number" end, "a number" },
  integer  = { function(v) return type(v) == "number" and v == math.floor(v) end, "a whole number" },
  string   = { function(v) return type(v) == "string" end, "a string" },
  boolean  = { function(v) return type(v) == "boolean" end, "true or false" },
  curve    = { function(v) return v == "lin" or v == "exp" end, '"lin" or "exp"' },
  side     = { function(v) return v == "left" or v == "right" end, '"left" or "right"' },
  channels = { function(v) return v == "both" or v == "left" or v == "right" end, '"both", "left" or "right"' },
  oversampling = { function(v) return v == 1 or v == 2 or v == 4 or v == 8 end, "1, 2, 4 or 8" },
  -- A transfer curve: a function of x in -1..1, or a list of >= 2 numbers (evenly spaced over -1..1).
  transfer = { function(v)
    if type(v) == "function" then return true end
    if not is_list(v) or #v < 2 then return false end
    for _, x in ipairs(v) do if type(x) ~= "number" then return false end end
    return true
  end, "a function(x) or a list of at least 2 numbers" },
  -- Notes: Hz numbers or names like "C2", "F#3", "Bb1" (A4 = 440 Hz).
  notes = { list_of(function(v) return type(v) == "number" or (type(v) == "string" and note_hz(v) ~= nil) end),
            'a non-empty list of Hz values or note names like "C2", "F#3"' },
  -- A note length synced to the host tempo: "1/4", "1/8." (dotted), "1/16t" (triplet), "2/1" (two bars).
  sync = { function(v) return type(v) == "string" and sync_beats(v) ~= nil end, 'a note length like "1/8", "1/8." or "1/16t"' },
  -- A knob's start value (the block makes it a Param, see Blocks below).
  setting  = { function(v) return type(v) == "number" end, "a number (the start value; to control it, use block.params.<name>)" },
  settings = { list_of(function(v) return type(v) == "number" end), "a non-empty list of numbers" },
  chain    = { function(v) return is(v, Chain) end, "a Chain" },
  chains   = { list_of(function(v) return is(v, Chain) end), "a non-empty list of Chains" },
  macros   = { list_of(function(v) return is(v, Macro) end), "a non-empty list of Macros" },
  surface  = { function(v) return type(v) == "table" and type(v.__component) == "table" end, "a component instance (Surface.new(...))" },
  theme    = { function(v) return type(v) == "table" and getmetatable(v) == nil end, "a table (colors, fonts, images, library)" },
  modulators = { list_of(function(v) return type(v) == "table" and getmetatable(v) == nil end), "a non-empty list of modulator tables" },
  assignments = { list_of(function(v) return type(v) == "table" and getmetatable(v) == nil end), "a non-empty list of assignment tables" },
  destination = { function(v) return is(v, Param) or is(v, Macro) end, "a Param or Macro" },
}

-- `fields` is an ordered list of "name:kind" or "name:kind!" (required), or knob settings
-- (tables from the knob constructors in Blocks, already parsed).
local function parse_fields(fields)
  local out = {}
  for i, f in ipairs(fields) do
    if type(f) == "table" then
      out[i] = f
      continue
    end
    local name, kind, req = f:match("^([%w_]+):([%w_|]+)(!?)$")
    assert(name, "bad field spec " .. f)
    if kind:find("|", 1, true) and not KINDS[kind] then
      -- An enum of strings: "mode:half|full".
      local allowed, quoted = {}, {}
      for word in kind:gmatch("[^|]+") do
        allowed[word] = true
        quoted[#quoted + 1] = fmt("%q", word)
      end
      KINDS[kind] = { function(v) return allowed[v] == true end, table.concat(quoted, ", ") }
    end
    assert(KINDS[kind], "bad field spec " .. f)
    out[i] = { name = name, kind = kind, required = req == "!" }
  end
  return out
end

-- Checks an options table against `fields`; `level` as in fail().
local function options(what, o, fields, level)
  level = level + 1
  if o == nil then o = {} end
  if type(o) ~= "table" or getmetatable(o) ~= nil then
    fail(fmt("%s: options must be a table { ... }, got %s", what, describe(o)), level)
  end
  local known = {}
  for _, f in ipairs(fields) do known[f.name] = true end
  for k in pairs(o) do
    if not known[k] then
      local names = {}
      for _, f in ipairs(fields) do names[#names + 1] = f.name end
      fail(fmt("%s: unknown option '%s' (allowed: %s)", what, tostring(k), table.concat(names, ", ")), level)
    end
  end
  for _, f in ipairs(fields) do
    local v = o[f.name]
    if v == nil then
      if f.required then fail(fmt("%s: '%s' is required", what, f.name), level) end
    elseif not KINDS[f.kind][1](v) then
      fail(fmt("%s: '%s' must be %s, got %s", what, f.name, KINDS[f.kind][2], describe(v)), level)
    end
  end
  return o
end

local function check_id(what, id, level)
  if type(id) ~= "string" or not id:match("^[%a_][%w_]*$") then
    fail(fmt("%s: the id must be a string of letters, digits and _ (got %s)", what, describe(id)), level + 1)
  end
end

local function check_name(what, name, level)
  if type(name) ~= "string" or name == "" then
    fail(fmt("%s: the display name must be a non-empty string (got %s)", what, describe(name)), level + 1)
  end
end

-- Namespaces that raise on unknown members (catches `Blocks.Compresor`).
local function strict(name, t)
  return setmetatable(t, {
    __index = function(_, k) error(fmt("'%s.%s' does not exist", name, tostring(k)), 2) end,
  })
end

---------------------------------------------------------------- Param
-- A knob of a block ("<block id>.<setting>"). Blocks create them (see Blocks); patches reach
-- them as `block.params.<name>`. There is no Param.new: every param belongs to a block.
Param.__index = Param
Param.__name = "Param"

-- Narrows the knob to lo..hi within the block type's range (the knob, the surface and typed values
-- stay in there; a macro's from/to default to it). The start value must be inside.
function Param:range(lo, hi)
  if not is(self, Param) then fail("use param:range(lo, hi) with a colon", 2) end
  local what = fmt("'%s':range(%s, %s)", self.id, tostring(lo), tostring(hi))
  if type(lo) ~= "number" or type(hi) ~= "number" or lo == hi then fail(what .. ": needs two different numbers", 2) end
  local min, max = math.min(self.min, self.max), math.max(self.min, self.max)
  if math.min(lo, hi) < min or math.max(lo, hi) > max then
    fail(fmt("%s: must stay within %g..%g %s", what, self.min, self.max, self.unit or ""), 2)
  end
  if self.scale == "log" and (lo <= 0 or hi <= 0) then fail(what .. ": a log knob needs a range above 0", 2) end
  if self.default < math.min(lo, hi) or self.default > math.max(lo, hi) then
    fail(fmt("%s: the start value %g is outside", what, self.default), 2)
  end
  self.min, self.max = lo, hi
  return self
end

---------------------------------------------------------------- Macro
Macro.__index = Macro
Macro.__name = "Macro"

local MACRO_FIELDS = parse_fields { "default:number" }
local TARGET_FIELDS = parse_fields { "from:number", "to:number", "curve:curve" }

function Macro.new(id, name, o)
  check_id("Macro.new", id, 2)
  local what = fmt("Macro '%s'", id)
  check_name(what, name, 2)
  o = options(what, o, MACRO_FIELDS, 2)
  if o.default and (o.default < 0 or o.default > 1) then fail(what .. ": default must be 0..1", 2) end
  return setmetatable({ id = id, name = name, default = o.default, targets = {}, __ref = "macro." .. id }, Macro)
end

function Macro:drives(param, o)
  if not is(self, Macro) then fail("use macro:drives(param, ...) with a colon", 2) end
  if not is(param, Param) then
    fail(fmt("Macro '%s': drives() expects a Param, got %s", self.id, describe(param)), 2)
  end
  local what = fmt("Macro '%s' -> '%s'", self.id, param.id)
  o = options(what, o, TARGET_FIELDS, 2)
  for _, t in ipairs(self.targets) do
    if t.param == param then fail(what .. ": this macro already drives that param", 2) end
  end
  local from, to = o.from or param.min, o.to or param.max
  if o.curve == "exp" and (from == 0 or to == 0 or (from < 0) ~= (to < 0)) then
    fail(what .. ': curve "exp" needs non-zero from/to with the same sign', 2)
  end
  -- Macros modulate around the knob's base value (Serum style: normalized deltas), so the first
  -- macro on a param makes the macro's `from` that base. Later macros only add deltas. Without an explicit
  -- default, the macro starts where the knob's original start value sat in the range.
  if not param.driven then
    if self.default == nil and #self.targets == 0 then
      -- Where the start sits between from and to, measured on the knob's own travel (lin or
      -- log): the same normalized space the modulation runs in.
      local function travel(x)
        if param.scale == "log" then return math.log(x / param.min) / math.log(param.max / param.min) end
        return (x - param.min) / (param.max - param.min)
      end
      local span = travel(to) - travel(from)
      local v = span ~= 0 and (travel(param.default) - travel(from)) / span or 0
      if v == v then self.default = math.min(math.max(v, 0), 1) end
    end
    param.default = math.min(math.max(from, math.min(param.min, param.max)), math.max(param.min, param.max))
    param.driven = true
  end
  self.targets[#self.targets + 1] = { param = param, from = o.from, to = o.to, curve = o.curve }
  return self
end

---------------------------------------------------------------- Chain
Chain.__index = Chain
Chain.__name = "Chain"

local function add_block(chain, block, level)
  if not is(block, Block) then
    fail(fmt("Chain: expected a block (Blocks.<Type>.new(...)), got %s", describe(block)), level + 1)
  end
  if block.owner then
    fail(fmt("block '%s' is already part of a chain; create a new block instead", block.id), level + 1)
  end
  block.owner = chain
  chain.blocks[#chain.blocks + 1] = block
end

function Chain.new(blocks)
  local c = setmetatable({ blocks = {} }, Chain)
  if blocks ~= nil then
    if type(blocks) ~= "table" or getmetatable(blocks) ~= nil then
      fail(fmt("Chain.new expects a list of blocks, got %s", describe(blocks)), 2)
    end
    if not is_list(blocks) then
      fail("Chain.new: the list has holes (is one of the blocks nil?)", 2)
    end
    for _, b in ipairs(blocks) do add_block(c, b, 2) end
  end
  return c
end

function Chain:add(block)
  if not is(self, Chain) then fail("use chain:add(block) with a colon", 2) end
  add_block(self, block, 2)
  return self
end

local function take_chain(chain, what, level)
  if chain.used then fail(what .. ": this Chain is already used elsewhere; create a new one", level + 1) end
  chain.used = true
end

---------------------------------------------------------------- Blocks

-- Every numeric block setting is a knob: the block type declares it once here (unit, range,
-- default), and each block creates one Param per setting, "<block id>.<setting>", starting at
-- the value the patch gives (or the default). The patch reaches them as `block.params.<name>`
-- (e.g. `macro:drives(echo.params.feedback)`); the shell shows them as the block's knobs.
--
-- Knob kinds: dB, pct (%), hz, ms, sec (s), num (any unit). Hz/ms/s knobs move in even ratios
-- (log scale) when their range starts above 0. `optional`: only created when the patch sets it
-- (its absence means something: no sample-and-hold, no crossfade).
local function knob(unit, log)
  return function(name, min, max, default, o)
    o = o or {}
    local span = math.max(math.abs(min), math.abs(max))
    local decimals = o.decimals
    if decimals == nil then
      if unit == "%" or unit == "Hz" and span > 50 then decimals = 0
      elseif span <= 20 then decimals = 2
      elseif span <= 1000 then decimals = 1
      else decimals = 0 end
    end
    return { name = name, kind = o.list and "settings" or "setting", required = o.required, optional = o.optional,
             meta = { unit = o.unit or unit, min = min, max = max, default = default, decimals = decimals,
                      scale = (log and min > 0) and "log" or nil, step = o.step } }
  end
end
local dB, pct, hz, ms, sec = knob("dB"), knob("%"), knob("Hz", true), knob("ms", true), knob("s", true)
local function num(unit, log) return knob(unit, log) end
local ratio = num(":1")

local function title(name) return (name:gsub("_", " "):gsub("^%l", string.upper)) end

-- The Param of one setting (ids may contain the block's dot; Param.new doesn't exist any more).
local function block_param(block_id, name, display, meta, value)
  return setmetatable({ id = block_id .. "." .. name, name = display, min = meta.min, max = meta.max,
                        default = value or meta.default, unit = meta.unit, decimals = meta.decimals,
                        scale = meta.scale, step = meta.step, __ref = "param." .. block_id .. "." .. name }, Param)
end

-- `check(o)` returns an error message for constraints between options.
-- Metered blocks can be a UI component's `source` (their __ref is "meter.<id>").
-- Every block also takes `oversampling` (1, 2, 4, 8): it and everything inside
-- it runs at that multiple of the rate (default: automatic for nonlinear blocks).
local function define(name, kind, fields, check, metered)
  fields = table.clone(fields)
  fields[#fields + 1] = "oversampling:oversampling"
  fields = parse_fields(fields)
  local B = {}
  function B.new(id, o)
    check_id(fmt("Blocks.%s.new", name), id, 2)
    local what = fmt("Blocks.%s '%s'", name, id)
    o = options(what, o, fields, 2)
    local msg = check and check(o)
    if msg then fail(what .. ": " .. msg, 2) end
    -- Plain data for the DSP side (curves sampled, notes in Hz, sync in beats, settings as Params).
    o = table.clone(o)
    local params = {}
    for _, f in ipairs(fields) do
      local v = o[f.name]
      if f.kind == "setting" then
        if v ~= nil or not f.optional then
          local m = f.meta
          if v ~= nil and (v < math.min(m.min, m.max) or v > math.max(m.min, m.max)) then
            fail(fmt("%s: %s must be %g..%g %s, got %g", what, f.name, m.min, m.max, m.unit, v), 2)
          end
          local p = block_param(id, f.name, title(f.name), m, v)
          params[f.name] = p
          o[f.name] = p
        end
      elseif f.kind == "settings" then
        -- A list (crossovers): "<id>.crossover_1", ... as `params.crossovers[i]`.
        local list, single = {}, f.name:gsub("s$", "")
        for i, x in ipairs(v) do
          local m = f.meta
          if x < m.min or x > m.max then
            fail(fmt("%s: %s must be %g..%g %s, got %g", what, f.name, m.min, m.max, m.unit, x), 2)
          end
          list[i] = block_param(id, fmt("%s_%d", single, i), fmt("%s %d", title(single), i), m, x)
        end
        params[f.name] = list
        o[f.name] = list
      elseif v ~= nil then
        if f.kind == "transfer" then
          o[f.name] = sample_curve(v, what, 2)
        elseif f.kind == "notes" then
          local hzs = {}
          for i, n in ipairs(v) do hzs[i] = type(n) == "number" and n or note_hz(n) end
          o[f.name] = hzs
        elseif f.kind == "sync" then
          o[f.name] = sync_beats(v)
        end
      end
    end
    for _, f in ipairs(fields) do
      if f.kind == "chains" then
        for _, c in ipairs(o[f.name]) do take_chain(c, what, 2) end
      elseif f.kind == "chain" and o[f.name] then
        take_chain(o[f.name], what, 2)
      end
    end
    return setmetatable({ kind = kind, type = name, id = id, o = o, fields = fields,
                          params = strict(id .. ".params", params),
                          __ref = metered and ("meter." .. id) or nil }, Block)
  end
  Blocks[name] = strict("Blocks." .. name, B)
end

-- phase: "minimum" (default: IIR, no latency) or "linear" (FIR, no phase shift, ~90 ms latency).
local function eq(freq) return { hz("freq", 20, 20000, freq), dB("gain", -24, 24, 0), num("")("q", 0.1, 10, 0.707), "phase:minimum|linear" } end
local function filter(freq)
  return { hz("freq", 20, 20000, freq), num("")("q", 0.1, 10, 0.707), num("dB/oct")("slope", 12, 48, 12, { step = 12, decimals = 0 }),
           "phase:minimum|linear" }
end
-- One versatile compressor; the compressor blocks are its presets (`d` = the defaults).
-- detector: 0 = peak .. 100 = RMS over `window`; feedback: 0 = feed-forward .. 100 = feedback;
-- ratio_curve: the ratio rises with level (vari-mu); program: program-dependent release.
local function comp(d)
  return { dB("threshold", -60, 0, -18), num(":1", true)("ratio", 1, 100, d.ratio, { decimals = 1 }),
           ms("attack", 0.05, 200, d.attack), ms("release", 5, 3000, d.release), dB("knee", 0, 30, d.knee),
           dB("makeup", 0, 24, 0), "auto_makeup:boolean", pct("mix", 0, 100, 100),
           pct("detector", 0, 100, d.detector), ms("window", 1, 300, d.window or 10), pct("feedback", 0, 100, d.feedback),
           pct("ratio_curve", 0, 100, d.curve or 0), pct("program", 0, 100, d.program or 0), pct("link", 0, 100, 100),
           pct("saturation", 0, 100, d.saturation or 0), "lookahead:number" }
end
-- Lookahead (ms) is latency: a fixed number, not a knob.
local function check_lookahead(o)
  if o.lookahead and (o.lookahead < 0 or o.lookahead > 20) then return "lookahead must be 0..20 ms" end
end
local function reverb(extra)
  local f = { pct("size", 0, 100, 50), sec("decay", 0.1, 30, 2), pct("damping", 0, 100, 50), ms("predelay", 0, 500, 0),
              pct("width", 0, 200, 100), pct("mix", 0, 100, 30) }
  for _, x in ipairs(extra) do f[#f + 1] = x end
  return f
end

-- Utility
define("Gain", "gain", { dB("db", -120, 24, 0) })  -- down to -120: mute
define("PeakEq", "peak_eq", eq(1000))
define("LowShelf", "low_shelf", eq(150))
define("HighShelf", "high_shelf", eq(6000))
define("LowPass", "low_pass", filter(20000))
define("HighPass", "high_pass", filter(20))
define("Drive", "drive", { dB("amount", 0, 36, 0), pct("mix", 0, 100, 100) })
define("Compressor", "compressor", comp { ratio = 2, attack = 5, release = 100, knee = 4, detector = 0, feedback = 0 }, check_lookahead, true)
define("FETCompressor", "fet_compressor",
       comp { ratio = 4, attack = 0.3, release = 80, knee = 2, detector = 0, feedback = 100, saturation = 15 }, check_lookahead, true)
define("OptoCompressor", "opto_compressor",
       comp { ratio = 3, attack = 10, release = 250, knee = 8, detector = 100, window = 30, feedback = 100, program = 100 },
       check_lookahead, true)
define("VariMuCompressor", "vari_mu_compressor",
       comp { ratio = 6, attack = 25, release = 400, knee = 12, detector = 60, window = 20, feedback = 100, curve = 100,
              program = 60, saturation = 30 }, check_lookahead, true)
define("Rider", "rider", { dB("target", -48, 0, -18), dB("range", 0, 24, 6), ms("speed", 10, 5000, 300) }, nil, true)
define("Transient", "transient", { dB("attack", -24, 24, 0), dB("sustain", -24, 24, 0), "lookahead:number" }, check_lookahead, true)
define("Pan", "pan", { num("")("pan", -100, 100, 0, { decimals = 0 }) })
define("Width", "width", { pct("width", 0, 200, 100) })
define("MonoBass", "mono_bass", { hz("freq", 20, 500, 120), num("dB/oct")("slope", 24, 48, 24, { step = 24, decimals = 0 }) })
-- delay > 0 delays `channel` (default right), < 0 the other one
define("Haas", "haas", { ms("delay", -40, 40, 10), "channel:side" })
define("Invert", "invert", { "channels:channels" })
define("DCBlock", "dc_block", {})
define("Meter", "meter", {}, nil, true)

-- Distortion
define("Waveshaper", "waveshaper", { "curve:transfer!", dB("drive", 0, 36, 0), dB("output", -36, 12, 0), pct("mix", 0, 100, 100) })
define("Wavefolder", "wavefolder", { dB("amount", 0, 36, 0), pct("bias", -100, 100, 0), pct("mix", 0, 100, 100) })
define("Clipper", "clipper", { dB("ceiling", -24, 0, 0), pct("softness", 0, 100, 0), dB("drive", 0, 36, 0) })
define("Rectifier", "rectifier", { "mode:half|full", pct("mix", 0, 100, 100) })
define("Bitcrusher", "bitcrusher", { num("bits")("bits", 1, 24, 8, { decimals = 0 }),
                                     hz("rate", 100, 48000, 8000, { optional = true }), pct("mix", 0, 100, 100) })
define("RingMod", "ring_mod", { hz("freq", 1, 5000, 200), pct("mix", 0, 100, 100) })
define("Tape", "tape", { dB("drive", 0, 24, 6), pct("wow", 0, 100, 0), pct("flutter", 0, 100, 0), pct("mix", 0, 100, 100) })

-- Filter
define("MorphFilter", "morph_filter", { hz("freq", 20, 20000, 1000), pct("morph", 0, 100, 0), pct("resonance", 0, 100, 0) })
define("LadderFilter", "ladder_filter", { hz("freq", 20, 20000, 1000), pct("resonance", 0, 100, 0), dB("drive", 0, 24, 0) })
define("CombFilter", "comb_filter", { hz("freq", 20, 5000, 200), pct("feedback", -99, 99, 70), pct("mix", 0, 100, 50) })
define("Formant", "formant", { num("")("vowel", 0, 4, 0), num("st")("shift", -12, 12, 0, { decimals = 1 }),
                               pct("resonance", 0, 100, 50), pct("mix", 0, 100, 100) })
define("Resonator", "resonator", { "notes:notes!", ms("decay", 10, 5000, 500), pct("mix", 0, 100, 50) }, function(o)
  if #o.notes > 16 then return "at most 16 notes" end
end)
define("DynamicEq", "dynamic_eq", { hz("freq", 20, 20000, 1000), num("")("q", 0.1, 10, 1), dB("threshold", -60, 0, -24),
                                    ratio("ratio", 1, 20, 3), dB("range", 0, 24, 12), ms("attack", 0.1, 200, 5),
                                    ms("release", 5, 2000, 100), "mode:cut|boost" }, nil, true)

-- Modulation (sync overrides rate: the LFO cycle is that note length at the host tempo)
define("Chorus", "chorus", { hz("rate", 0.01, 20, 0.8), "sync:sync", pct("depth", 0, 100, 50), pct("mix", 0, 100, 50) })
define("Flanger", "flanger", { hz("rate", 0.01, 20, 0.3), "sync:sync", pct("depth", 0, 100, 70), pct("feedback", -99, 99, 50),
                               pct("mix", 0, 100, 50) })
define("Phaser", "phaser", { hz("rate", 0.01, 20, 0.5), "sync:sync", pct("depth", 0, 100, 70), pct("feedback", -99, 99, 30),
                             hz("freq", 50, 10000, 800), "stages:integer", pct("mix", 0, 100, 50) }, function(o)
  if o.stages and (o.stages < 2 or o.stages > 12 or o.stages % 2 ~= 0) then return "stages must be 2, 4, ..., 12" end
end)
-- phase_offset: LFO phase shift in degrees (synced + host playing: relative to the host's beat grid)
define("Tremolo", "tremolo", { hz("rate", 0.01, 20, 5), "sync:sync", num("°")("phase_offset", 0, 360, 0, { decimals = 0 }),
                               pct("depth", 0, 100, 50), "shape:sine|triangle|square" })
define("AutoPan", "auto_pan", { hz("rate", 0.01, 20, 1), "sync:sync", num("°")("phase_offset", 0, 360, 0, { decimals = 0 }),
                                pct("depth", 0, 100, 100), "shape:sine|triangle|square" })

-- Space
define("Delay", "delay", { ms("time", 1, 4000, 375), "sync:sync", pct("feedback", 0, 100, 35), pct("mix", 0, 100, 30),
                           "pingpong:boolean", "feedback_chain:chain" })
-- freeze: > 0.5 = infinite sustain (a switch; drive it with a macro)
define("Reverb", "reverb", reverb({ num("")("freeze", 0, 1, 0, { decimals = 0 }) }))
define("Shimmer", "shimmer", reverb({ num("st")("pitch", -24, 24, 12, { decimals = 0 }), pct("amount", 0, 100, 50) }))

-- Pitch
define("PitchShift", "pitch_shift", { num("st")("semitones", -24, 24, 0, { decimals = 1 }), ms("window", 10, 200, 60),
                                      pct("mix", 0, 100, 100) })
define("Granular", "granular", { ms("grain", 5, 500, 80), num("/s", true)("density", 1, 100, 20, { decimals = 0 }),
                                 ms("spread", 0, 2000, 300), num("st")("pitch", -24, 24, 0, { decimals = 1 }),
                                 num("ct")("jitter", 0, 100, 0, { decimals = 0 }), pct("reverse", 0, 100, 0),
                                 pct("width", 0, 200, 100), pct("mix", 0, 100, 50) })

-- Rhythm (active: > 0.5 = repeating; drive it with a macro)
define("Stutter", "stutter", { num("")("active", 0, 1, 0, { decimals = 0 }), ms("length", 10, 2000, 125), "sync:sync",
                               pct("mix", 0, 100, 100) })

-- Dynamics
define("Limiter", "limiter", { dB("ceiling", -24, 0, -0.3), ms("release", 1, 1000, 80), "lookahead:number" }, check_lookahead, true)
define("Gate", "gate", { dB("threshold", -80, 0, -40), ratio("ratio", 1, 100, 20), dB("range", 0, 90, 60),
                         ms("attack", 0.1, 100, 1), ms("hold", 0, 500, 20), ms("release", 5, 2000, 150), "lookahead:number" },
       check_lookahead, true)
define("DeEsser", "de_esser", { hz("freq", 2000, 16000, 6000), dB("threshold", -60, 0, -20), dB("range", 0, 24, 12), "lookahead:number" },
       check_lookahead, true)

-- Routing
define("MidSide", "mid_side", { "mid:chain", "side:chain" }, function(o)
  if not o.mid and not o.side then return "needs a 'mid' and/or a 'side' chain" end
end)
define("LeftRight", "left_right", { "left:chain", "right:chain" }, function(o)
  if not o.left and not o.right then return "needs a 'left' and/or a 'right' chain" end
end)
define("BandSplit", "band_split", { hz("crossovers", 20, 20000, 1000, { list = true, required = true }),
                                    num("dB/oct")("slope", 24, 48, 24, { step = 24, decimals = 0 }), "bands:chains!", "phase:minimum|linear" }, function(o)
  if #o.bands ~= #o.crossovers + 1 then
    return fmt("%d crossover(s) need %d bands, got %d", #o.crossovers, #o.crossovers + 1, #o.bands)
  end
  for i = 2, #o.crossovers do
    if o.crossovers[i] <= o.crossovers[i - 1] then return "crossovers must be in ascending order" end
  end
end)
-- mix (exactly 2 branches): a crossfade, 0 % = first, 100 % = second; without it, the branches are summed
define("Parallel", "parallel", { "branches:chains!", pct("mix", 0, 100, 50, { optional = true }) }, function(o)
  if #o.branches < 2 then return "needs at least 2 branches" end
  if o.mix ~= nil and #o.branches ~= 2 then
    return fmt("'mix' needs exactly 2 branches (got %d); put a Blocks.Gain at the end of each branch instead", #o.branches)
  end
end)

strict("Blocks", Blocks)

---------------------------------------------------------------- Assets
-- Font.new(path) / Image.new(path): files from the patch folder, used as tokens in the theme
-- (and later by surfaces). Paths are relative to the patch folder; the loader checks they exist.
Font.__index, Font.__name = Font, "Font"
Image.__index, Image.__name = Image, "Image"

local function asset(class, what, exts, path, level)
  if type(path) ~= "string" or path == "" then
    fail(fmt("%s: needs a path relative to the patch folder, got %s", what, describe(path)), level + 1)
  end
  if path:sub(1, 1) == "/" or path:find("..", 1, true) or path:find("\\", 1, true) then
    fail(fmt("%s: %q must be a path inside the patch folder", what, path), level + 1)
  end
  local ext = (path:match("%.(%w+)$") or ""):lower()
  if not exts[ext] then
    local list = {}
    for e in pairs(exts) do list[#list + 1] = "." .. e end
    table.sort(list)
    fail(fmt("%s: %q must be %s", what, path, table.concat(list, ", ")), level + 1)
  end
  return setmetatable({ path = path }, class)
end

-- .ttf/.otf, .svg (SVG 1.1 font) or .fnt (BMFont + PNG atlas)
function Font.new(path) return asset(Font, "Font.new", { ttf = true, otf = true, svg = true, fnt = true }, path, 2) end
-- .svg or .png
function Image.new(path, o)
  if o ~= nil then fail("Image.new: no options yet (sprite sheets come with the raster UI)", 2) end
  return asset(Image, "Image.new", { svg = true, png = true }, path, 2)
end
strict("Font", Font)
strict("Image", Image)

-- theme = { colors = { name = "#rrggbb" }, fonts = { name = Font }, images = { name = Image },
--           library = { accent, bg, ink = "#rrggbb", font = Font, logo = Image } }
-- colors/fonts/images are the patch's own tokens (any names); `library` overrides them for the
-- library card. The shell reads colors.accent/bg/ink, fonts.title and images.logo by default.
local LIBRARY_KEYS = { accent = "colour", bg = "colour", ink = "colour", font = "Font", logo = "Image" }
local function check_theme(t, what, level)
  local function colour(v) return type(v) == "string" and v:match("^#%x%x%x%x%x%x$") ~= nil end
  local checks = {
    colors = { colour, 'a colour "#rrggbb"' },
    fonts = { function(v) return is(v, Font) end, "a Font (Font.new(path))" },
    images = { function(v) return is(v, Image) end, "an Image (Image.new(path))" },
  }
  for k, v in pairs(t) do
    if checks[k] then
      if type(v) ~= "table" or getmetatable(v) ~= nil then fail(fmt("%s: theme.%s must be a table", what, k), level + 1) end
      for name, x in pairs(v) do
        if type(name) ~= "string" then fail(fmt("%s: theme.%s needs names as keys", what, k), level + 1) end
        if not checks[k][1](x) then
          fail(fmt("%s: theme.%s.%s must be %s, got %s", what, k, name, checks[k][2], describe(x)), level + 1)
        end
      end
    elseif k == "library" then
      if type(v) ~= "table" or getmetatable(v) ~= nil then fail(fmt("%s: theme.library must be a table", what), level + 1) end
      for name, x in pairs(v) do
        local kind = LIBRARY_KEYS[name]
        if not kind then
          fail(fmt("%s: theme.library: unknown key '%s' (accent, bg, ink, font, logo)", what, tostring(name)), level + 1)
        end
        local ok = (kind == "colour" and colour(x)) or (kind == "Font" and is(x, Font)) or (kind == "Image" and is(x, Image))
        if not ok then
          local want = kind == "colour" and 'a colour "#rrggbb"' or (kind == "Font" and "a Font" or "an Image")
          fail(fmt("%s: theme.library.%s must be %s, got %s", what, name, want, describe(x)), level + 1)
        end
      end
    else
      fail(fmt("%s: theme: unknown key '%s' (colors, fonts, images, library)", what, tostring(k)), level + 1)
    end
  end
end

-- Plain data for the loader: assets become their paths.
local function export_theme(t)
  if not t then return nil end
  local function map(m, f)
    if not m or next(m) == nil then return nil end
    local out = {}
    for k, v in pairs(m) do out[k] = f(v) end
    return out
  end
  local path = function(a) return a.path end
  local lib = t.library or {}
  local library = {
    accent = lib.accent, bg = lib.bg, ink = lib.ink,
    font = lib.font and lib.font.path, logo = lib.logo and lib.logo.path,
  }
  return {
    colors = map(t.colors, function(c) return c end),
    fonts = map(t.fonts, path),
    images = map(t.images, path),
    library = next(library) and library or nil,
  }
end

---------------------------------------------------------------- Patch
Patch.__index = Patch
Patch.__name = "Patch"

Patch.__is_patch = true

local PATCH_FIELDS = parse_fields { "author:string", "chain:chain", "macros:macros", "modulators:modulators", "assignments:assignments", "theme:theme", "surface:surface" }

local MOD_FOLLOW_FIELDS = parse_fields { "id:string!", "attack_ms:number", "release_ms:number" }
-- sync (a note length, "1/8") overrides rate_hz: the LFO cycle / one step lasts that long at the host tempo.
local MOD_LFO_FIELDS = parse_fields { "id:lfo1|lfo2|lfo3!", "rate_hz:number", "sync:sync", "phase:number", "shape:sine|triangle|square" }
local MOD_STEP_FIELDS = parse_fields { "id:string!", "rate_hz:number", "sync:sync", "values:settings" }
local ASSIGNMENT_FIELDS = parse_fields { "source:follow|lfo1|lfo2|lfo3|step!", "destination:destination!", "amount:number", "bipolar:boolean", "curve:number", "bypass:boolean" }

local function check_modulators(list, what, level)
  local out, seen = {}, {}
  for i, m in ipairs(list) do
    local id = m.id
    if id ~= "follow" and id ~= "lfo1" and id ~= "lfo2" and id ~= "lfo3" and id ~= "step" then
      fail(fmt("%s: modulators[%d].id must be 'follow', 'lfo1', 'lfo2', 'lfo3' or 'step', got %s", what, i, describe(id)), level + 1)
    end
    if seen[id] then fail(fmt("%s: duplicate modulator '%s'", what, id), level + 1) end
    seen[id] = true
    local mwhat = fmt("%s.modulators['%s']", what, id)
    if id == "follow" then
      options(mwhat, m, MOD_FOLLOW_FIELDS, level + 1)
      out[#out + 1] = {
        id = id,
        settings = { kind = "follow", attack_ms = m.attack_ms or 10.0, release_ms = m.release_ms or 150.0 },
      }
    elseif id == "step" then
      options(mwhat, m, MOD_STEP_FIELDS, level + 1)
      out[#out + 1] = {
        id = id,
        settings = { kind = "step", rate_hz = m.rate_hz or 2.0, sync = m.sync and sync_beats(m.sync),
                     values = m.values or { 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5 } },
      }
    else
      options(mwhat, m, MOD_LFO_FIELDS, level + 1)
      out[#out + 1] = {
        id = id,
        settings = { kind = "lfo", rate_hz = m.rate_hz or 1.0, sync = m.sync and sync_beats(m.sync), phase = m.phase or 0.0,
                     shape = m.shape or "sine" },
      }
    end
  end
  return out
end

local function check_assignments(list, what, level)
  local out = {}
  for i, a in ipairs(list) do
    local awhat = fmt("%s.assignments[%d]", what, i)
    options(awhat, a, ASSIGNMENT_FIELDS, level + 1)
    local dst = a.destination
    if not is(dst, Param) and not is(dst, Macro) then
      fail(fmt("%s: 'destination' must be a Param or Macro, got %s", awhat, describe(dst)), level + 1)
    end
    out[#out + 1] = {
      source = a.source,
      destination = dst,
      settings = {
        amount = a.amount or 0.0,
        bipolar = a.bipolar or false,
        curve = a.curve or 0.0,
        bypass = a.bypass or false,
      },
    }
  end
  return out
end

function Patch.new(name, o)
  check_name("Patch.new", name, 2)
  local what = fmt("Patch '%s'", name)
  o = options(what, o, PATCH_FIELDS, 2)
  if o.chain then take_chain(o.chain, what, 2) end
  if o.theme then check_theme(o.theme, what, 2) end
  local modulators = o.modulators and check_modulators(o.modulators, what, 2) or {}
  local assignments = o.assignments and check_assignments(o.assignments, what, 2) or {}
  return setmetatable({ name = name, author = o.author, chain = o.chain, macros = o.macros or {},
                        modulators = modulators, assignments = assignments, theme = o.theme,
                        surface = o.surface }, Patch)
end

---------------------------------------------------------------- Meters
-- The plugin's own input/output level, usable as a component `source`.
local Meters = { IN = { __ref = "meter.in" }, OUT = { __ref = "meter.out" } }

_G.Macro, _G.Chain, _G.Blocks, _G.Patch, _G.Meters = Macro, Chain, Blocks, Patch, Meters
-- The old API, with directions (patches from before block params).
_G.Param = setmetatable({}, { __index = function()
  error("Param.new is gone: every param belongs to a block. Set the start value on the block "
     .. "(Blocks.Drive.new(\"sat\", { amount = 6 })) and use sat.params.amount, e.g. macro:drives(sat.params.amount)", 2)
end })
_G.Font, _G.Image = Font, Image

---------------------------------------------------------------- export (called by the loader)
-- Params are collected from the blocks (chain order) and the macros' targets; blocks refer to
-- theirs by index (0-based, the Rust side's `ParamRef`), so a patch never lists them.
return function(p)
  if not is(p, Patch) then
    error("init.luau must end with `return Patch.new(...)`, got " .. describe(p), 0)
  end
  local params, index = {}, {}
  -- The param's index in `params` (0-based), adding it on first use.
  local function use(prm)
    if index[prm] == nil then
      index[prm] = #params
      params[#params + 1] = { id = prm.id, name = prm.name, min = prm.min, max = prm.max,
                              default = prm.default, unit = prm.unit, decimals = prm.decimals, scale = prm.scale,
                              step = prm.step }
    end
    return index[prm]
  end
  local export_chain
  local function export_block(b)
    local t, own = { type = b.kind }, {}
    for _, f in ipairs(b.fields) do
      local v = b.o[f.name]
      if v == nil or f.name == "oversampling" then
        -- omitted
      elseif f.kind == "setting" then
        t[f.name] = use(v)
        own[#own + 1] = t[f.name]
      elseif f.kind == "settings" then
        local l = {}
        for i, x in ipairs(v) do
          l[i] = use(x)
          own[#own + 1] = l[i]
        end
        t[f.name] = l
      elseif f.kind == "chain" then
        t[f.name] = export_chain(v)
      elseif f.kind == "chains" then
        local l = {}
        for i, c in ipairs(v) do l[i] = export_chain(c) end
        t[f.name] = l
      else
        t[f.name] = v
      end
    end
    return { id = b.id, oversampling = b.o.oversampling, params = own, kind = t }
  end
  export_chain = function(c)
    local out = {}
    for i, b in ipairs(c.blocks) do out[i] = export_block(b) end
    return out
  end

  local chain = p.chain and export_chain(p.chain) or {}
  local macros, macro_ids = {}, {}
  for i, m in ipairs(p.macros) do
    if macro_ids[m.id] then error(fmt("macro '%s' is listed twice (or two macros share that id)", m.id), 0) end
    macro_ids[m.id] = true
    if #m.targets == 0 then error(fmt("macro '%s' drives nothing; add :drives(param)", m.id), 0) end
    local targets = {}
    for j, t in ipairs(m.targets) do
      use(t.param)
      targets[j] = { param = t.param.id, from = t.from, to = t.to, curve = t.curve }
    end
    macros[i] = { id = m.id, name = m.name, default = m.default, targets = targets }
  end
  local assignments = {}
  for i, a in ipairs(p.assignments) do
    local dst
    if is(a.destination, Param) then
      use(a.destination)
      dst = { kind = "param", id = a.destination.id }
    else
      dst = { kind = "macro", id = a.destination.id }
    end
    assignments[i] = { source = a.source, destination = dst, settings = a.settings }
  end
  return { name = p.name, author = p.author, params = params, macros = macros,
           modulators = p.modulators, assignments = assignments, chain = chain, theme = export_theme(p.theme) }
end
