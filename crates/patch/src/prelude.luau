-- ScriptFX patch API (loaded into the sandbox before patch.luau).
--
--   local ui = require("./ui")
--   local drive = Param.new("drive", "Drive", { min = 0, max = 24, unit = "dB" })
--   local amount = Macro.new("amount", "Amount"):drives(drive)
--   local chain = Chain.new({ Blocks.Drive.new("sat", { amount = drive }) })
--   return Patch.new("My Patch", { chain = chain, macros = { amount }, panel = ui.Panel.new({ amount = amount }) })
--
-- Constructors take the identity positionally (id, display name) and every
-- setting by name in an options table. Everything is checked here, with the
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

local Block = {}
Block.__index = Block

local function describe(v)
  if is(v, Param) then return "a Param" end
  if is(v, Macro) then return "a Macro" end
  if is(v, Chain) then return "a Chain" end
  if is(v, Block) then return "a block" end
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

local function is_setting(v) return type(v) == "number" or is(v, Param) end

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
  setting  = { is_setting, "a number or a Param" },
  settings = { list_of(is_setting), "a non-empty list of numbers/Params" },
  chain    = { function(v) return is(v, Chain) end, "a Chain" },
  chains   = { list_of(function(v) return is(v, Chain) end), "a non-empty list of Chains" },
  macros   = { list_of(function(v) return is(v, Macro) end), "a non-empty list of Macros" },
  panel    = { function(v) return type(v) == "table" and type(v.__component) == "table" end, "a component instance (Panel.new(...))" },
  theme    = { function(v)
    if type(v) ~= "table" then return false end
    for k, c in pairs(v) do
      if type(k) ~= "string" or type(c) ~= "string" then return false end
    end
    return true
  end, "a table of name = colour strings" },
}

-- `fields` is an ordered list of "name:kind" or "name:kind!" (required).
local function parse_fields(fields)
  local out = {}
  for i, f in ipairs(fields) do
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
Param.__index = Param
Param.__name = "Param"

local PARAM_FIELDS = parse_fields { "min:number!", "max:number!", "default:number", "unit:string", "decimals:integer" }

function Param.new(id, name, o)
  check_id("Param.new", id, 2)
  local what = fmt("Param '%s'", id)
  check_name(what, name, 2)
  o = options(what, o, PARAM_FIELDS, 2)
  if o.min == o.max then fail(what .. ": min and max must differ", 2) end
  local lo, hi = math.min(o.min, o.max), math.max(o.min, o.max)
  if o.default and (o.default < lo or o.default > hi) then
    fail(fmt("%s: default %s is outside %s..%s", what, o.default, o.min, o.max), 2)
  end
  if o.decimals and (o.decimals < 0 or o.decimals > 6) then fail(what .. ": decimals must be 0..6", 2) end
  return setmetatable({ id = id, name = name, min = o.min, max = o.max, default = o.default,
                        unit = o.unit, decimals = o.decimals, __ref = "param." .. id }, Param)
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
    -- Plain data for the DSP side (curves sampled, notes in Hz, sync in beats).
    o = table.clone(o)
    for _, f in ipairs(fields) do
      local v = o[f.name]
      if v ~= nil then
        if f.kind == "transfer" then
          o[f.name] = sample_curve(v, what, 2)
        elseif f.kind == "notes" then
          local hz = {}
          for i, n in ipairs(v) do hz[i] = type(n) == "number" and n or note_hz(n) end
          o[f.name] = hz
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
                          __ref = metered and ("meter." .. id) or nil }, Block)
  end
  Blocks[name] = strict("Blocks." .. name, B)
end

local EQ = { "freq:setting!", "gain:setting!", "q:setting" }
local FILTER = { "freq:setting!", "q:setting" }
local COMP = { "threshold:setting!", "ratio:setting", "attack:setting", "release:setting", "knee:setting",
               "makeup:setting", "auto_makeup:boolean", "mix:setting" }

define("Gain", "gain", { "db:setting!" })
define("PeakEq", "peak_eq", EQ)
define("LowShelf", "low_shelf", EQ)
define("HighShelf", "high_shelf", EQ)
define("LowPass", "low_pass", FILTER)
define("HighPass", "high_pass", FILTER)
define("Drive", "drive", { "amount:setting!", "mix:setting" })
define("Compressor", "compressor", COMP, nil, true)
define("FETCompressor", "fet_compressor", COMP, nil, true)
define("OptoCompressor", "opto_compressor", COMP, nil, true)
define("Rider", "rider", { "target:setting!", "range:setting!", "speed:setting" }, nil, true)
define("Transient", "transient", { "attack:setting!", "sustain:setting" }, nil, true)
-- Utility
define("Pan", "pan", { "pan:setting!" })
define("Width", "width", { "width:setting!" })
define("MonoBass", "mono_bass", { "freq:setting!" })
define("Haas", "haas", { "delay:setting!", "channel:side" }, function(o)
  if type(o.delay) == "number" and (o.delay < 0 or o.delay > 40) then return "delay must be 0..40 ms" end
end)
define("Invert", "invert", { "channels:channels" })
define("DCBlock", "dc_block", {})
define("Meter", "meter", {}, nil, true)

-- Distortion
local function pct_range(lo, hi, ...)
  local names = { ... }
  return function(o)
    for _, n in ipairs(names) do
      local v = o[n]
      if type(v) == "number" and (v < lo or v > hi) then return fmt("%s must be %g..%g", n, lo, hi) end
    end
  end
end
define("Waveshaper", "waveshaper", { "curve:transfer!", "drive:setting", "output:setting", "mix:setting" })
define("Wavefolder", "wavefolder", { "amount:setting!", "bias:setting", "mix:setting" })
define("Clipper", "clipper", { "ceiling:setting", "softness:setting", "drive:setting" })
define("Rectifier", "rectifier", { "mode:half|full", "mix:setting" })
define("Bitcrusher", "bitcrusher", { "bits:setting", "rate:setting", "mix:setting" }, pct_range(1, 24, "bits"))
define("RingMod", "ring_mod", { "freq:setting!", "mix:setting" })
define("Tape", "tape", { "drive:setting", "wow:setting", "flutter:setting", "mix:setting" })

-- Filter
define("MorphFilter", "morph_filter", { "freq:setting!", "morph:setting", "resonance:setting" })
define("LadderFilter", "ladder_filter", { "freq:setting!", "resonance:setting", "drive:setting" })
define("CombFilter", "comb_filter", { "freq:setting!", "feedback:setting", "mix:setting" }, pct_range(-99, 99, "feedback"))
define("Formant", "formant", { "vowel:setting!", "shift:setting", "resonance:setting", "mix:setting" }, pct_range(0, 4, "vowel"))
define("Resonator", "resonator", { "notes:notes!", "decay:setting", "mix:setting" }, function(o)
  if #o.notes > 16 then return "at most 16 notes" end
end)
define("DynamicEq", "dynamic_eq", { "freq:setting!", "q:setting", "threshold:setting!", "ratio:setting", "range:setting",
                                    "attack:setting", "release:setting", "mode:cut|boost" }, nil, true)

-- Modulation (sync overrides rate: the LFO cycle is that note length at the host tempo)
define("Chorus", "chorus", { "rate:setting", "sync:sync", "depth:setting", "mix:setting" })
define("Flanger", "flanger", { "rate:setting", "sync:sync", "depth:setting", "feedback:setting", "mix:setting" },
       pct_range(-99, 99, "feedback"))
define("Phaser", "phaser", { "rate:setting", "sync:sync", "depth:setting", "feedback:setting", "freq:setting",
                             "stages:integer", "mix:setting" }, function(o)
  if o.stages and (o.stages < 2 or o.stages > 12 or o.stages % 2 ~= 0) then return "stages must be 2, 4, ..., 12" end
  return pct_range(-99, 99, "feedback")(o)
end)
-- phase_offset: LFO phase shift in degrees (synced + host playing: relative to the host's beat grid)
define("Tremolo", "tremolo", { "rate:setting", "sync:sync", "phase_offset:setting", "depth:setting", "shape:sine|triangle|square" })
define("AutoPan", "auto_pan", { "rate:setting", "sync:sync", "phase_offset:setting", "depth:setting", "shape:sine|triangle|square" })

-- Space
define("Delay", "delay", { "time:setting", "sync:sync", "feedback:setting", "mix:setting", "pingpong:boolean",
                           "feedback_chain:chain" }, function(o)
  if type(o.time) == "number" and (o.time < 1 or o.time > 4000) then return "time must be 1..4000 ms" end
  return pct_range(0, 100, "feedback")(o)
end)
local REVERB = { "size:setting", "decay:setting", "damping:setting", "predelay:setting", "width:setting", "mix:setting" }
local reverb_fields = table.clone(REVERB)
reverb_fields[#reverb_fields + 1] = "freeze:setting"
define("Reverb", "reverb", reverb_fields, pct_range(0, 500, "predelay"))
local shimmer_fields = table.clone(REVERB)
shimmer_fields[#shimmer_fields + 1] = "pitch:setting"
shimmer_fields[#shimmer_fields + 1] = "amount:setting"
define("Shimmer", "shimmer", shimmer_fields, pct_range(0, 500, "predelay"))

-- Pitch
define("PitchShift", "pitch_shift", { "semitones:setting!", "window:setting", "mix:setting" }, pct_range(-24, 24, "semitones"))
define("Granular", "granular", { "grain:setting", "density:setting", "spread:setting", "pitch:setting", "jitter:setting",
                                 "reverse:setting", "width:setting", "mix:setting" }, pct_range(-24, 24, "pitch"))

-- Rhythm
define("Stutter", "stutter", { "active:setting!", "length:setting", "sync:sync", "mix:setting" })

-- Dynamics
define("Limiter", "limiter", { "ceiling:setting", "release:setting", "lookahead:number" }, function(o)
  if o.lookahead and (o.lookahead < 0 or o.lookahead > 20) then return "lookahead must be 0..20 ms" end
end, true)
define("Gate", "gate", { "threshold:setting!", "ratio:setting", "range:setting", "attack:setting", "hold:setting",
                         "release:setting" }, nil, true)
define("DeEsser", "de_esser", { "freq:setting", "threshold:setting!", "range:setting" }, nil, true)

-- Routing
define("MidSide", "mid_side", { "mid:chain", "side:chain" }, function(o)
  if not o.mid and not o.side then return "needs a 'mid' and/or a 'side' chain" end
end)
define("LeftRight", "left_right", { "left:chain", "right:chain" }, function(o)
  if not o.left and not o.right then return "needs a 'left' and/or a 'right' chain" end
end)
define("BandSplit", "band_split", { "crossovers:settings!", "bands:chains!" }, function(o)
  if #o.bands ~= #o.crossovers + 1 then
    return fmt("%d crossover(s) need %d bands, got %d", #o.crossovers, #o.crossovers + 1, #o.bands)
  end
  for i = 2, #o.crossovers do
    local a, b = o.crossovers[i - 1], o.crossovers[i]
    if type(a) == "number" and type(b) == "number" and b <= a then
      return "crossovers must be in ascending order"
    end
  end
end)
define("Parallel", "parallel", { "branches:chains!", "mix:setting" }, function(o)
  if #o.branches < 2 then return "needs at least 2 branches" end
  if o.mix ~= nil and #o.branches ~= 2 then
    return fmt("'mix' needs exactly 2 branches (got %d); put a Blocks.Gain at the end of each branch instead", #o.branches)
  end
end)

strict("Blocks", Blocks)

---------------------------------------------------------------- Patch
Patch.__index = Patch
Patch.__name = "Patch"

Patch.__is_patch = true

local PATCH_FIELDS = parse_fields { "author:string", "chain:chain", "macros:macros", "theme:theme", "panel:panel" }

function Patch.new(name, o)
  check_name("Patch.new", name, 2)
  local what = fmt("Patch '%s'", name)
  o = options(what, o, PATCH_FIELDS, 2)
  if o.chain then take_chain(o.chain, what, 2) end
  return setmetatable({ name = name, author = o.author, chain = o.chain, macros = o.macros or {}, theme = o.theme,
                        panel = o.panel }, Patch)
end

---------------------------------------------------------------- Meters
-- The plugin's own input/output level, usable as a component `source`.
local Meters = { IN = { __ref = "meter.in" }, OUT = { __ref = "meter.out" } }

_G.Param, _G.Macro, _G.Chain, _G.Blocks, _G.Patch, _G.Meters = Param, Macro, Chain, Blocks, Patch, Meters

---------------------------------------------------------------- export (called by the loader)
-- Params are collected from what the chain and the macros reference (chain
-- order first), so a patch never lists them separately.
return function(p)
  if not is(p, Patch) then
    error("patch.luau must end with `return Patch.new(...)`, got " .. describe(p), 0)
  end
  local params, by_id = {}, {}
  local function use(prm)
    local seen = by_id[prm.id]
    if seen == nil then
      by_id[prm.id] = prm
      params[#params + 1] = { id = prm.id, name = prm.name, min = prm.min, max = prm.max,
                              default = prm.default, unit = prm.unit, decimals = prm.decimals }
    elseif seen ~= prm then
      error(fmt("two different params use the id '%s'", prm.id), 0)
    end
  end
  local function setting(v)
    if is(v, Param) then
      use(v)
      return "param." .. v.id
    end
    return v
  end
  local export_chain
  local function export_block(b)
    local t = { type = b.kind }
    for _, f in ipairs(b.fields) do
      local v = b.o[f.name]
      if v == nil or f.name == "oversampling" then
        -- omitted
      elseif f.kind == "setting" then
        t[f.name] = setting(v)
      elseif f.kind == "settings" then
        local l = {}
        for i, x in ipairs(v) do l[i] = setting(x) end
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
    return { id = b.id, oversampling = b.o.oversampling, kind = t }
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
  return { name = p.name, author = p.author, params = params, macros = macros, chain = chain, theme = p.theme }
end
