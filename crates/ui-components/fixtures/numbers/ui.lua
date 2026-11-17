local N = component {
  name = "N",
  render = function()
    return {
      string.format('<rect x="0" y="0" width="10" height="10" opacity="%.2f"/>', 0.08),
      '<rect x="0" y="0" width="10" height="10" opacity="' .. 0.25 .. '"/>',
      svg.circle { cx = 5.5, cy = 5, r = 2, fill = "#fff", opacity = 0.5 },
    }
  end,
}
return { width = 20, height = 20, root = N {} }
