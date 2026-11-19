-- Tabs: composition plus state passed down (the group table holds the active tab).

type TabProps = { label: string, index: number, group: { [string]: any } }

local TabButton = Component.define {
  name = "TabButton",
  kind = "button",
  on_click = function(_, props: TabProps) props.group.tab = props.index end,
  render = function(p: Props<TabProps>)
    local active = (p.group.tab or 1) == p.index
    return {
      SVG.rect { x = 0, y = 0, w = p.width, h = p.height, rx = 5, fill = active and "#2a1450" or "#00000000" },
      SVG.text { x = p.width / 2, y = p.height / 2 + 4, text = p.label, size = 10, weight = "bold", anchor = "middle", spacing = 2,
                 fill = active and "#ffffff" or (p.hover and "#e8d8ff" or "#9a8cc0") },
    }
  end,
}

local TabBar = Component.define {
  name = "TabBar",
  render = function(p: Props<{ tabs: { string }, group: { [string]: any } }>)
    local w = p.width / #p.tabs
    local parts = { SVG.rect { x = 0, y = 0, w = p.width, h = p.height, rx = 7, fill = "#0a0616", stroke = "#ffffff", width = 1, opacity = 0.6 } }
    for i, label in ipairs(p.tabs) do
      parts[#parts + 1] = TabButton.new { key = label, label = label, index = i, group = p.group, x = (i - 1) * w + 2, y = 2, w = w - 4, h = p.height - 4 }
    end
    return parts
  end,
}

return TabBar
