# Shell layout: dock, don't float

_2026-11-21 mockup by Flemming (claude.ai artifact "ScriptFX Shell Layout"); copied into the repo
2026-11-25 as [`mockups/shell-layout/`](mockups/shell-layout/). The boards are the artifact's
`.dc.html` files: they need the artifact runtime (`support.js`, not included), so read them as
markup; `canvas.json` holds the board titles and the rules below. Status at the end._

## Rules (from the mockup)

1. The **stage** (the patch's ControlPanel, 960×540 = 16:9, drawn 1:1) is never covered by
   panels. Whatever needs room docks beside or below it, and the stage scales down uniformly
   (animated). Only two small shell controls sit on it: the notch and the view switch.
2. **Top bar**: patches + patch name (with reload), ScriptFX, chat, settings. No Macros button,
   no Controls sheet.
3. The **notch** is back, slimmer: one row of snapshot controls (‹ Init › · save · load), hanging
   from the top bar.
4. **Surface / Chain / Matrix**: three icon buttons floating in the stage's top-right corner
   (label on hover).
5. The **mod dock** below the stage is always there: macros (paged 1–8 / 9–16) and the five
   modulators, each with a live mini display.
6. Knobs show only small **route dots**. Selecting a source in the dock shows that source's rings
   and meta knobs on every knob (Serum's rule); everything else dims. Clicking it again opens its
   editor: the dock grows, the stage shrinks.
7. Window 984×708 (was 836×640 with an 840×560 panel at 97 %): wider, not taller. The dock gets
   its own 108 px; its chevron collapses it to a 32 px strip (button + a dot per modulator) and
   the window shrinks to 984×632.
8. Every source has a **title bar** in its colour (name + ≡ grip): the drag handle. Grab it: the
   bar lifts off as a tag tied to its tile by a thin cable; knobs that can take it get a dashed
   outline in its colour, the one under the tag turns solid and previews the range; on release
   the tag drops in and a meta knob is left. Circles stay reserved for knobs and meta knobs.

Gone with it: the macro rail and its top-bar button, the drawer, the Controls sheet, the two-row
notch, sheets over the panel (→ docked columns: library left, chat and settings right).

## Status (2026-11-25)

Decided with Flemming: **the mod dock first**, keeping the 840×560 panel and the block modal (the
mockup's block page predates it).

**Built** (`shell/dock.rs`): the dock below the stage (window 836×714, the stage keeps 816×544);
macro cells (title bar = drag handle, knob, slot, value, a meta knob per modulator on it; pages
1–8 / 9–16); modulator tiles (title bar = drag handle, live mini display, rate or level, "no
routes" faded); click = focus (outline, the others fade, the hint counts its routes), again = the
editor; Escape closes the editor, then the focus. A title-bar press released in place is a click,
not a drag. The macro rail, the drawer and the top bar's Macros button are gone. Rules 3 and 4
too: the notch is one row (reload | snapshots | bypass), the view switch three icons in the
stage's top-right corner.

**Deviation: the editor floats** (Flemming, 2026-11-25). Scaling the stage (rule 6) was built
first and didn't feel right. Weighed: an editor always visible (window taller than wide), a modal
like the block modals (blurs what you're modulating, can't sit beside an open block modal),
growing the window (host resize, jumps by ~190 px, too tall at 150 %). Chosen: a panel rising
from the dock over the stage's lower part, the stage untouched; it stays open while the stage is
used, steps aside while a source is dragged (anything under it takes the drop), and a block modal
moves up above it.

**Open**, in the mockup's order:
- Rule 6 on the knobs: route dots only, the focused source's rings and meta knobs, the rest dimmed
  (the dock has the focus state: `Dock::focus`).
- Rule 7: collapsing the dock (32 px strip, the window shrinks; needs a host resize request).
- Rule 8's drag look: the tag with its cable, dashed outlines (today: a chip next to the pointer,
  solid outlines).
- Rules 1–2 beyond the dock: the 960×540 stage (every ControlPanel re-laid out), docked
  library/chat/settings columns, no Controls sheet, the patch name in the top bar.
