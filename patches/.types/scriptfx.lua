---@meta
-- Type stubs for ScriptFX patches (LuaLS / lua-language-server).
-- Also a compact API description to hand to an LLM.

---@alias Source string  # "macro.<id>" | "param.<id>" (both 0..1) | "meter.in" | "meter.out" (0..1 = -60..0 dBFS) | "meter.<block id>" (comp: gain reduction 0..24 dB → 0..1; rider/transient: gain, 0.5 = unity) | "time" (s) | "var.<name>" (from animate)
---@alias Setting number|string  # constant, or "param.<id>" (the param's plain value; units must match)
---@alias Color string   # "#rrggbb" | "#rgb" | "$<theme key>"
---@alias Pivot string|number[]  # element id (its bbox center) or { x, y } in SVG units

---@class Param  Internal control (unlimited). Saved with the host project, not automatable by itself.
---@field id string
---@field name string
---@field min? number      default 0
---@field max? number      default 1
---@field default? number  plain units, default = min (ignored when a macro drives it)
---@field unit? string
---@field decimals? integer default 2

---@class MacroTarget
---@field param string      param id; each param can be driven by at most one macro
---@field from? number      plain value at macro 0 (default: param min)
---@field to? number        plain value at macro 1 (default: param max); to < from inverts
---@field curve? "lin"|"exp" default "lin"; "exp" needs non-zero from/to with the same sign

---@class Macro  Host-automatable control (max 16). Its name shows up in the DAW.
---@field id string
---@field name string
---@field default? number  0..1; default: derived from the first target's param default
---@field targets MacroTarget[]

---@class AnimateState
---@field time number  seconds since editor opened
---@field dt number    seconds since last frame
---@field macro table<string, number>  normalized macro values by id
---@field param table<string, number>  normalized param values by id
---@field meter { in: number, out: number }

---@class ControlPanel  The patch's own UI: an SVG plus declarative bindings.
---@field svg string  path relative to the patch folder
---@field bindings? Binding[]
---@field animate? fun(s: AnimateState): table<string, number>  runs once per UI frame; returned keys become `var.<key>`

---@class Patch  A Lua script describing a chain, its params/macros and its ControlPanel. A Patch file returns this table.
---@field name string
---@field author? string
---@field params? Param[]
---@field macros? Macro[]
---@field chain? Block[]  effect chain, processed top to bottom (compiled to native DSP)
---@field theme? table<string, string>  keys become CSS classes .fill-<k> .stroke-<k> .stop-<k>
---@field control_panel? ControlPanel

---@class Block  Chain block. Units: dB, Hz, ms, % (0..100). Unknown fields are errors.

---@param id string @param o { db: Setting }
---@return Block
function gain(id, o) end
---@param id string @param o { type: "peak"|"lowshelf"|"highshelf"|"lowpass"|"highpass", freq: Setting, q?: Setting, gain?: Setting } q default 0.707; gain (dB) for peak/shelves
---@return Block
function eq(id, o) end
---@param id string @param o { amount: Setting, mix?: Setting } tanh saturator; amount = pre-gain dB (0..36), mix % (default 100)
---@return Block
function drive(id, o) end
---@param id string @param o { style?: "fet"|"opto"|"clean", threshold: Setting, ratio?: Setting, attack?: Setting, release?: Setting, knee?: Setting, makeup?: Setting, auto_makeup?: boolean, mix?: Setting } fet: peak, fast (4:1, 0.3/80 ms, knee 2); opto: RMS, program-dependent release (3:1, 10/250 ms, knee 8); clean (2:1, 5/100 ms, knee 4). auto_makeup gives back ~80% of the average reduction. Meter: gain reduction.
---@return Block
function comp(id, o) end
---@param id string @param o { target: Setting, range: Setting, speed?: Setting } slow leveler towards target dBFS within ±range dB (speed ms, default 300); doesn't ride up below target-40 dB. Meter: gain.
---@return Block
function rider(id, o) end
---@param id string @param o { attack: Setting, sustain?: Setting } transient shaper, dB boost/cut of attacks / sustain. Meter: gain.
---@return Block
function transient(id, o) end
---@param id string @param o { freq: Setting, low: Block[], high: Block[] } Linkwitz-Riley 4th-order band split; bands processed separately and summed
---@return Block
function split(id, o) end

---@class Binding

---@param id string @param o { source: Source, from: number, to: number, pivot?: Pivot } degrees
---@return Binding
function rotate(id, o) end
---@param id string @param o { source: Source, from: number[], to: number[] } {dx, dy}
---@return Binding
function translate(id, o) end
---@param id string @param o { source: Source, from?: number, to?: number, axis?: "both"|"x"|"y", pivot?: Pivot }
---@return Binding
function scale(id, o) end
---@param id string @param o { source: Source, from?: number, to?: number }
---@return Binding
function opacity(id, o) end
---@param id string @param o { source: Source, from: Color, to: Color }
---@return Binding
function fill(id, o) end
---@param id string @param o { source: Source, format?: string } param sources (and single-target macros) use the param's unit/decimals; format: %.Nf %d %%
---@return Binding
function text(id, o) end
---@param id string @param o { target: string, range?: number } vertical drag; target "macro.<id>" or "param.<id>" (a macro-driven param drags its macro); range = SVG units for 0..1 (default 200)
---@return Binding
function drag(id, o) end
