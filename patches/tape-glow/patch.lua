-- Tape Glow: one "Character" macro drives drive + tone at once, a free "wow"
-- param that only the UI uses, every binding kind, and an animate() hook.

local reel = 0
local glow = 0

return {
  name = "Tape Glow",
  author = "sandbox",

  -- Internal controls (unlimited). Chain blocks and UI bindings refer to them as "param.<id>".
  params = {
    { id = "drive",  name = "Drive",  min = 0,   max = 24,    default = 6,    unit = " dB", decimals = 1 },
    { id = "tone",   name = "Tone",   min = 500, max = 20000, default = 8000, unit = " Hz", decimals = 0 },
    { id = "mix",    name = "Mix",    min = 0,   max = 100,   default = 100,  unit = "%",   decimals = 0 },
    { id = "output", name = "Output", min = -24, max = 12,    default = 0,    unit = " dB", decimals = 1 },
    { id = "wow",    name = "Wow",    min = 0,   max = 100,   default = 25,   unit = "%",   decimals = 0 },
  },

  -- Host-automatable macros (max 16). Each drives one or more params.
  -- A macro without `default` takes it from its first target's default.
  macros = {
    { id = "character", name = "Character", default = 0.3, targets = {
        { param = "drive", from = 0,     to = 18 },
        { param = "tone",  from = 16000, to = 2500, curve = "exp" },   -- darker as it gets dirtier
    } },
    { id = "mix",    name = "Mix",    targets = { { param = "mix" } } },
    { id = "output", name = "Output", targets = { { param = "output" } } },
  },

  -- The effect chain: plain data, compiled to native DSP. "param.<id>" values use the
  -- param's plain value, so units must match (dB, Hz, %).
  chain = {
    drive("tape",  { amount = "param.drive", mix = "param.mix" }),
    eq("tone",     { type = "lowpass", freq = "param.tone", q = 0.707 }),
    gain("output", { db = "param.output" }),
  },

  -- Keys become CSS classes in the SVG (.fill-accent, .stroke-accent, .stop-accent)
  -- and also style the plugin's own (egui) chrome.
  theme = {
    bg     = "#17141c",
    bg2    = "#2a2230",
    panel  = "#3a3140",
    edge   = "#5a4d60",
    accent = "#ff8a3d",
    text   = "#f3e6d8",
    dim    = "#8c7f94",
  },

  control_panel = {
    svg = "panel.svg",

    bindings = {
      -- knobs show the params; drive & tone are both driven by Character
      rotate("drive_pointer",  { source = "param.drive",  from = -135, to = 135, pivot = "drive_knob" }),
      rotate("tone_pointer",   { source = "param.tone",   from = -135, to = 135, pivot = "tone_knob" }),
      rotate("mix_pointer",    { source = "param.mix",    from = -135, to = 135, pivot = "mix_knob" }),
      rotate("output_pointer", { source = "param.output", from = -135, to = 135, pivot = "output_knob" }),
      rotate("wow_pointer",    { source = "param.wow",    from = -135, to = 135, pivot = "wow_knob" }),
      text("drive_value",  { source = "param.drive" }),
      text("tone_value",   { source = "param.tone" }),
      text("mix_value",    { source = "param.mix" }),
      text("output_value", { source = "param.output" }),
      text("wow_value",    { source = "param.wow" }),
      text("character_value", { source = "macro.character" }),   -- multi-target → percent

      -- dragging drive OR tone moves the Character macro; wow is a free param
      drag("drive_knob",  { target = "macro.character" }),
      drag("tone_knob",   { target = "macro.character" }),
      drag("mix_knob",    { target = "macro.mix" }),
      drag("output_knob", { target = "macro.output" }),
      drag("wow_knob",    { target = "param.wow", range = 120 }),

      -- VU meter needle follows the output level
      rotate("vu_needle", { source = "meter.out", from = -50, to = 50, pivot = { 500, 172 } }),

      -- driven by animate()
      rotate("reel_l", { source = "var.reel", from = 0, to = 360 }),
      rotate("reel_r", { source = "var.reel", from = 0, to = 360 }),
      opacity("glow",  { source = "var.pulse", from = 0.05, to = 0.9 }),
      fill("sat_led",  { source = "var.sat", from = "#2a211c", to = "$accent" }),
      text("sat_text", { source = "var.sat_pct", format = "SAT %d%%" }),
    },

    -- Runs on the UI thread once per frame (sandboxed, instruction-budgeted).
    -- Returns extra values that bindings can use as `var.<name>`.
    animate = function(s)
      local speed = 0.15 + s.param.drive * 0.6                      -- reel revolutions / second
      local wobble = 1 + s.param.wow * 0.6 * math.sin(s.time * 5.3)  -- wow = speed flutter
      reel = (reel + s.dt * speed * wobble) % 1
      local target = s.meter.out * (0.4 + 0.6 * s.param.drive)
      glow = glow + (target - glow) * math.min(1, s.dt * 8)
      local breath = 0.5 + 0.5 * math.sin(s.time * 1.3)
      local sat = math.min(1, glow * 1.5)
      return { reel = reel, pulse = glow * 0.8 + breath * 0.2, sat = sat, sat_pct = sat * 100 }
    end,
  },
}
