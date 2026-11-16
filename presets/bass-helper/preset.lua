-- Bass Helper: transient shaping → FET → opto → band split
--   low:  fader-riding leveler
--   high: drive + fizz filter
-- → boom / boxy / click EQ → output
--
-- Three "big" macros (Transients, Stabilize, Drive) each drive several params;
-- Boom / Boxy / Click / Output map 1:1 onto bipolar EQ gains / output level.

return {
  name = "Bass Helper",
  author = "sandbox",

  params = {
    -- dynamics
    { id = "punch",       name = "Punch",          min = -9,   max = 9,     default = 0,    unit = " dB", decimals = 1 },
    { id = "fet_attack",  name = "FET attack",     min = 0.1,  max = 10,    default = 1,    unit = " ms", decimals = 1 },
    { id = "fet_thresh",  name = "FET threshold",  min = -40,  max = 0,     default = -6,   unit = " dB", decimals = 1 },
    { id = "opto_thresh", name = "Opto threshold", min = -40,  max = 0,     default = -6,   unit = " dB", decimals = 1 },
    { id = "fet_ratio",   name = "FET ratio",      min = 1,    max = 20,    default = 4,    unit = ":1",  decimals = 1 },
    { id = "opto_ratio",  name = "Opto ratio",     min = 1,    max = 10,    default = 3,    unit = ":1",  decimals = 1 },
    { id = "ride_range",  name = "Ride range",     min = 0,    max = 12,    default = 0,    unit = " dB", decimals = 1 },
    { id = "ride_target", name = "Ride target",    min = -36,  max = -6,    default = -16,  unit = " dB", decimals = 0 },
    -- bands
    { id = "xover",       name = "Crossover",      min = 80,   max = 600,   default = 250,  unit = " Hz", decimals = 0 },
    { id = "grit",        name = "Grit",           min = 0,    max = 30,    default = 0,    unit = " dB", decimals = 1 },
    { id = "grit_mix",    name = "Grit mix",       min = 0,    max = 100,   default = 0,    unit = "%",   decimals = 0 },
    { id = "fizz",        name = "Fizz filter",    min = 2000, max = 16000, default = 12000, unit = " Hz", decimals = 0 },
    -- tone
    { id = "boom",        name = "Boom",           min = -9,   max = 9,     default = 0,    unit = " dB", decimals = 1 },
    { id = "boxy",        name = "Boxy",           min = -9,   max = 9,     default = 0,    unit = " dB", decimals = 1 },
    { id = "click",       name = "Click",          min = -9,   max = 9,     default = 0,    unit = " dB", decimals = 1 },
    { id = "output",      name = "Output",         min = -24,  max = 12,    default = 0,    unit = " dB", decimals = 1 },
  },

  macros = {
    -- more attack: transient boost + let the FET's attack through
    { id = "transients", name = "Transients", default = 0.5, targets = {
        { param = "punch",      from = -9,  to = 9 },
        { param = "fet_attack", from = 0.1, to = 8, curve = "exp" },
    } },
    -- more even: both compressors work harder, the low band gets ridden
    { id = "stabilize", name = "Stabilize", default = 0.3, targets = {
        { param = "fet_thresh",  from = -4, to = -30 },
        { param = "fet_ratio",   from = 2,  to = 8 },
        { param = "opto_thresh", from = -4, to = -26 },
        { param = "opto_ratio",  from = 2,  to = 6 },
        { param = "ride_range",  from = 0,  to = 9 },
    } },
    -- grit on the upper band only, darker fizz filter as it gets dirtier
    { id = "drive", name = "Drive", default = 0.2, targets = {
        { param = "grit",     from = 0,     to = 28 },
        { param = "grit_mix", from = 0,     to = 85 },
        { param = "fizz",     from = 12000, to = 3500, curve = "exp" },
    } },
    -- bipolar band lifts/scoops (centre = flat)
    { id = "boom",   name = "Boom",   targets = { { param = "boom" } } },
    { id = "boxy",   name = "Boxy",   targets = { { param = "boxy" } } },
    { id = "click",  name = "Click",  targets = { { param = "click" } } },
    { id = "output", name = "Output", targets = { { param = "output" } } },
  },

  chain = {
    transient("punch", { attack = "param.punch" }),
    comp("fet",  { style = "fet",  threshold = "param.fet_thresh",  ratio = "param.fet_ratio", attack = "param.fet_attack", release = 90, auto_makeup = true }),
    comp("opto", { style = "opto", threshold = "param.opto_thresh", ratio = "param.opto_ratio", auto_makeup = true }),
    split("bands", { freq = "param.xover",
      low = {
        rider("rider", { target = "param.ride_target", range = "param.ride_range", speed = 250 }),
      },
      high = {
        drive("grit", { amount = "param.grit", mix = "param.grit_mix" }),
        eq("fizz",    { type = "lowpass", freq = "param.fizz", q = 0.707 }),
      },
    }),
    eq("boom",  { type = "peak", freq = 80,   q = 0.9, gain = "param.boom" }),
    eq("boxy",  { type = "peak", freq = 450,  q = 1.3, gain = "param.boxy" }),
    eq("click", { type = "peak", freq = 2800, q = 1.0, gain = "param.click" }),
    gain("out", { db = "param.output" }),
  },

  theme = {
    bg     = "#101418",
    bg2    = "#1b2229",
    panel  = "#26303a",
    edge   = "#3b4a57",
    accent = "#f2c14e",
    cool   = "#5fb3d9",
    hot    = "#ef6f50",
    text   = "#e8edf2",
    dim    = "#7d8b98",
  },

  ui = {
    svg = "panel.svg",
    bindings = {
      -- big macro knobs (multi-target → value text in percent)
      rotate("transients_ptr", { source = "macro.transients", from = -135, to = 135, pivot = "transients_knob" }),
      rotate("stabilize_ptr",  { source = "macro.stabilize",  from = -135, to = 135, pivot = "stabilize_knob" }),
      rotate("drive_ptr",      { source = "macro.drive",      from = -135, to = 135, pivot = "drive_knob" }),
      drag("transients_knob", { target = "macro.transients" }),
      drag("stabilize_knob",  { target = "macro.stabilize" }),
      drag("drive_knob",      { target = "macro.drive" }),
      text("transients_val", { source = "macro.transients" }),
      text("stabilize_val",  { source = "macro.stabilize" }),
      text("drive_val",      { source = "macro.drive" }),

      -- bipolar tone knobs (single target → dB text)
      rotate("boom_ptr",  { source = "param.boom",  from = -135, to = 135, pivot = "boom_knob" }),
      rotate("boxy_ptr",  { source = "param.boxy",  from = -135, to = 135, pivot = "boxy_knob" }),
      rotate("click_ptr", { source = "param.click", from = -135, to = 135, pivot = "click_knob" }),
      rotate("out_ptr",   { source = "param.output", from = -135, to = 135, pivot = "out_knob" }),
      drag("boom_knob",  { target = "macro.boom" }),
      drag("boxy_knob",  { target = "macro.boxy" }),
      drag("click_knob", { target = "macro.click" }),
      drag("out_knob",   { target = "macro.output" }),
      text("boom_val",  { source = "param.boom" }),
      text("boxy_val",  { source = "param.boxy" }),
      text("click_val", { source = "param.click" }),
      text("out_val",   { source = "param.output" }),

      -- crossover: free param (not a macro, saved with the project)
      rotate("xover_ptr", { source = "param.xover", from = -135, to = 135, pivot = "xover_knob" }),
      drag("xover_knob", { target = "param.xover", range = 150 }),
      text("xover_val", { source = "param.xover" }),

      -- signal-flow strip: live block meters
      -- comp meters = gain reduction (0..24 dB → bar width); punch/rider = gain, 0.5 = unity
      scale("fet_bar",   { source = "meter.fet",  axis = "x", pivot = { 148, 0 } }),
      scale("opto_bar",  { source = "meter.opto", axis = "x", pivot = { 238, 0 } }),
      translate("punch_mark", { source = "meter.punch", from = { -32, 0 }, to = { 32, 0 } }),
      translate("rider_mark", { source = "meter.rider", from = { -40, 0 }, to = { 40, 0 } }),
      opacity("grit_glow", { source = "param.grit_mix", from = 0, to = 1 }),

      -- in/out meters
      scale("in_bar",  { source = "meter.in",  axis = "y", pivot = { 0, 352 } }),
      scale("out_bar", { source = "meter.out", axis = "y", pivot = { 0, 352 } }),
    },
  },
}
