---@meta
-- Type stubs for ScriptFX presets (LuaLS / lua-language-server).
-- Also a compact API description to hand to an LLM.

---@alias Source string  # "macro.<id>" | "param.<id>" (both 0..1) | "meter.in" | "meter.out" (0..1 = -60..0 dBFS) | "time" (s) | "var.<name>" (from animate)
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
---@field dsp? "drive"|"tone"|"mix"|"output"  mock-DSP control this param drives (max one param each)

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

---@class Ui
---@field svg string  path relative to the preset folder
---@field bindings? Binding[]
---@field animate? fun(s: AnimateState): table<string, number>  runs once per UI frame; returned keys become `var.<key>`

---@class Preset
---@field name string
---@field author? string
---@field params? Param[]
---@field macros? Macro[]
---@field theme? table<string, string>  keys become CSS classes .fill-<k> .stroke-<k> .stop-<k>
---@field ui? Ui

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
