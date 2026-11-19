-- Bass Helper: colours and the signal-flow strip geometry, shared by the components and the
-- panel layout.

export type Box = { x: number, y: number, w: number, h: number, label: string }

return {
  BG = "#101418", BG2 = "#1b2229", PANEL = "#26303a", EDGE = "#3b4a57",
  ACCENT = "#f2c14e", COOL = "#5fb3d9", HOT = "#ef6f50", TEXT = "#e8edf2", DIM = "#7d8b98",
  STRIP = { y = 100, h = 96 },
  BOXES = {
    punch = { x = 50,  y = 104, w = 96,  h = 88, label = "PUNCH" },
    fet   = { x = 166, y = 104, w = 104, h = 88, label = "FET" },
    opto  = { x = 290, y = 104, w = 104, h = 88, label = "OPTO" },
    low   = { x = 432, y = 104, w = 196, h = 41, label = "LOW · RIDER" },
    high  = { x = 432, y = 151, w = 196, h = 41, label = "HIGH · GRIT · FIZZ" },
    eq    = { x = 666, y = 104, w = 56,  h = 88, label = "EQ" },
    out   = { x = 742, y = 104, w = 48,  h = 88, label = "OUT" },
  },
}
