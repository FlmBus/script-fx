-- Bass Helper: signal-flow strip with live block meters on top, the three big
-- macros in the middle, tone/output/crossover below, in/out meters at the sides.
-- The patch's init.luau passes the macros, params and metered blocks in (see PanelProps).

local style = require("@self/style")
local Background = require("@self/background")
local block_meters = require("@self/block_meters")
local Knob = require("@self/knob")
local LevelMeter = require("@self/level_meter")
local GrMeter, GainMeter, GritLed = block_meters.GrMeter, block_meters.GainMeter, block_meters.GritLed
local BOXES, COOL, HOT = style.BOXES, style.COOL, style.HOT
type Box = style.Box

local function meter_at(b: Box, comp: Component<{}>, source: MeteredBlock): ComponentInstance
  return comp.new { key = b.label, source = source, x = b.x + 8, y = b.y + 44, w = b.w - 16, h = 32 }
end

export type PanelProps = {
  transients: Macro, stabilize: Macro, drive: Macro,
  boom: Macro, boxy: Macro, click: Macro, output: Macro,
  xover: Param, grit_mix: Param,
  punch: TransientBlock, fet: FETCompressorBlock, opto: OptoCompressorBlock, rider: RiderBlock,
}

local Panel = Component.define {
  name = "Panel",
  render = function(p: Props<PanelProps>)
    local items = {
      Background.new {},
      meter_at(BOXES.punch, GainMeter, p.punch),
      meter_at(BOXES.fet, GrMeter, p.fet),
      meter_at(BOXES.opto, GrMeter, p.opto),
      GainMeter.new { key = "rider", source = p.rider, x = BOXES.low.x + 110, y = BOXES.low.y + 6, w = 78, h = 32 },
      GritLed.new { bind = p.grit_mix, x = BOXES.high.x + 160, y = BOXES.high.y + 9 },
      LevelMeter.new { key = "in", label = "IN", source = Meters.IN, x = 48, y = 228 },
      LevelMeter.new { key = "out", label = "OUT", source = Meters.OUT, x = 762, y = 228 },
    }
    local big: { { label: string, bind: Bindable, color: string? } } = {
      { label = "TRANSIENTS", bind = p.transients },
      { label = "STABILIZE",  bind = p.stabilize },
      { label = "DRIVE",      bind = p.drive, color = HOT },
    }
    for i, k in big do
      items[#items + 1] = Knob.new { key = k.label, label = k.label, bind = k.bind, color = k.color, big = true,
                                     x = 140 + (i - 1) * 205, y = 232, w = 150, h = 166 }
    end
    local small: { { label: string, bind: Bindable, bipolar: boolean?, color: string? } } = {
      { label = "BOOM",   bind = p.boom, bipolar = true },
      { label = "BOXY",   bind = p.boxy, bipolar = true },
      { label = "CLICK",  bind = p.click, bipolar = true },
      { label = "OUTPUT", bind = p.output },
      { label = "XOVER",  bind = p.xover, color = COOL },
    }
    for i, k in small do
      items[#items + 1] = Knob.new { key = k.label, label = k.label, bind = k.bind, bipolar = k.bipolar, color = k.color,
                                     x = 140 + (i - 1) * 116, y = 414, w = 100, h = 118 }
    end
    return items
  end,
}

return { Panel = Panel }
