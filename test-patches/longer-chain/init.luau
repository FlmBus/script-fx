local blocks = {}
local kinds = { "HighPass", "Drive", "Gain", "LowPass", "Width", "Drive", "Gain", "HighPass", "Drive", "Gain", "LowPass", "Width", "Drive", "Gain", "HighPass", "Drive", "Gain", "LowPass", "Width", "Drive", "Gain", "Limiter" }
for i, k in kinds do
  table.insert(blocks, Blocks[k].new(string.lower(k) .. "_" .. i, {}))
end
return Patch.new("Longer Chain", { author = "test", chain = Chain.new(blocks) })
