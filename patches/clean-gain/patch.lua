-- Clean Gain: purely declarative, no animate() script.

return {
  name = "Clean Gain",
  author = "sandbox",

  params = {
    { id = "gain", name = "Gain", min = -24, max = 12, default = 0, unit = " dB", decimals = 1 },
  },

  macros = {
    { id = "gain", name = "Gain", targets = { { param = "gain" } } },
  },

  chain = {
    gain("out", { db = "param.gain" }),
  },

  theme = {
    bg     = "#0f1a1d",
    panel  = "#1d2c31",
    edge   = "#35505a",
    accent = "#4fd1c5",
    hot    = "#ff5d73",
    text   = "#e6f4f1",
    dim    = "#6f8f97",
  },

  control_panel = {
    svg = "panel.svg",
    bindings = {
      translate("fader_cap", { source = "param.gain", from = { 0, 100 }, to = { 0, -100 } }),
      drag("fader_cap",      { target = "macro.gain", range = 200 }),
      text("gain_value",     { source = "param.gain" }),
      scale("meter_in_bar",  { source = "meter.in",  axis = "y", pivot = { 0, 250 } }),
      scale("meter_out_bar", { source = "meter.out", axis = "y", pivot = { 0, 250 } }),
      fill("meter_out_bar",  { source = "meter.out", from = "$accent", to = "$hot" }),
    },
  },
}
