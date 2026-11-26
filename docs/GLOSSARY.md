# ScriptFX glossary

_The terms we use in code, UI and docs. One name per thing: if a word is in the "Avoid" list at the
bottom, it lost. The project's state and next steps are in [`STATUS.md`](STATUS.md); concepts and
research in [`research/`](research/)._

**Terminology for the viewport was settled on 2026-11-25** (Flemming): the three-tier
viewport → view → surface model was collapsed to two, and `page` was retired. The code is being
moved onto it; rows marked **⟳** name what the code still calls the thing today.

## The viewport and its surfaces

The **viewport** is the stage's content area — everything inside the gutter, under the notch and
above the dock. It shows exactly one **surface** at a time, and the switcher (one icon per surface,
floating in the stage's top-right corner) lists the surfaces available for the loaded patch.

```
viewport
└── surface                     the switcher lists them; the first one is the default
    ├── chain       built-in    the block tree; owns the block modal and its zoom
    ├── matrix      built-in    every route as a table
    └── patch surfaces   0..n   declared by the patch (Luau)
                                with none, the shell draws the default macro surface
```

There is **no separate tier** for the built-in surfaces: the chain and the Matrix are surfaces in
exactly the sense a patch's own UI is. The shell holds a list and an index, not a three-way match.

Why it is one tier:

- Each surface **owns its state and messages**. The block modal and its camera belong to the chain
  surface, not to the shell; the add row belongs to the Matrix. The shell does not know what a
  block is.
- A patch may declare **more than one surface** (no factory patch does, but nothing stops it), and
  the switcher just grows an entry.
- What used to be per-view special cases in the shell become **per-surface metadata**:
  `owns_block_modal` (only the chain has blocks to zoom into), and which surface is the default
  (where passive mode and a patch load land).

The residue is deliberate: the shell still needs to *name* the chain surface, because dragging a
macro or modulator onto a knob targets it. So surfaces are keyed by a `SurfaceId`
(`Chain | Matrix | Patch(n)`), not held in a free-form list.

**Surface** is the tier's word, not "view": `view` is Iced's — every widget has a `fn view()`, so a
type called `View` rendered by a method called `view()` is permanent noise.

## Terms

| Term | Meaning | Code |
|---|---|---|
| **Shell** | the plugin itself and its own UI (top bar, notch, viewport, sheets, dock) | `crates/plugin/src/shell/` |
| **Patch** | a Luau script describing a chain, its macros, theme and its surfaces (`return Patch.new(...)`) | crate `patch`, `patches/<name>/init.luau` |
| **Snapshot** | a saved sound of a patch: macros, params, meta-knob settings, routing edits; "Init" = its defaults | `snapshots.rs`, `shell/snapshot_menu.rs` |
| **Stage** | the patch window above the dock, in which the viewport sits | `shell/stage.rs` |
| **Viewport** | the stage's content area; shows one surface at a time | `shell/mod.rs` |
| **Surface** | one thing the viewport can show: the chain, the Matrix, or a patch's own UI | `shell/viewport.rs` |
| **Patch surface** | a patch's own UI: reactive Luau components, fixed 840×560 | ⟳ today `ControlPanel` / `ui.Panel`, `ui_components::Ui`, `shell/stage.rs` |
| **Default surface** | the shell's own macro surface (the macros as knobs, in the patch's theme), shown when a patch declares none | `shell/surface.rs` |
| **Component** | one visual part of a patch surface; each instance is a cached layer | `Component.define`, `X.new {}` |
| **Chain / Block** | DSP description / one **building block**: the unit patches build professional effect chains from, so it exposes every parameter it can ([`research/blocks.md`](research/blocks.md)) (`Chain.new`, `Blocks.<Type>.new(id, {...})`) | `patch::chain`, `BlockSpec` |
| **Chain surface / Block modal** | the surface showing the chain's tree / one block's modal (display + knobs), zoomed into from its node | `shell/chain_view.rs`, `shell/block_modal.rs`, `shell/motion.rs` |
| **Graph** | compiled chain running on the audio thread | `dsp::Graph` |
| **Param** | a block's knob; has a base value (saved with the project) | `ParamSpec` |
| **Macro** | host-automatable control of a patch, drives 1..n params | `MacroSpec` |
| **Host slot** | one of the 16 fixed host parameters macros are mapped onto | `MACRO_SLOTS` |
| **Modulation slot / route** | one source → param (or modulator → macro) link: amount (range size, sign = direction), uni/bi, curve, bypass | `patch::Modulation`, `ModAssignment`, `patch::Route`, `ModSettings` |
| **Meta knob / range arc / live dot** | a slot's mini knob on its target / the slot's range on the knob / the value after modulation | `shell/knob.rs` |
| **Modulator** | Follow, LFO 1–3, Step: sources that live outside the chain | `dsp::modulators`, `shell/dock.rs` |
| **Dock / Notch / Sheet** | macros and modulators below the stage (focus, a modulator's editor; replaced the drawer and the macro rail) / reload, snapshots and bypass at the top / the side panes (library, controls, chat, settings) | `shell/dock.rs`, `notch.rs`, `sheet.rs` |
| **Modulator editor** | the panel a focused modulator raises from the dock over the stage's lower part (was a drawer panel) | `shell/dock.rs` |
| **Matrix** | the surface showing every route as a table (live values, add/remove) | `shell/matrix.rs` |
| **Group page** | one of a block modal's knob groups, when it has more knobs than fit ([`research/block-displays.md`](research/block-displays.md)) — the **only** surviving use of "page" | `shell/block_modal.rs` |
| **Bind / Source** | what a component reads: `bind` = a Macro/Param object (value, draggable), `source` = a metered block or `Meters.IN/OUT` | `__ref`, `patch::Source` |
| **Theme** | a patch's design tokens (colours, fonts, images); used by its own surfaces, the library card and the default surface, never by the rest of the shell | `Theme` |
| **Listen / Capture / Analysis** | the action / recorded audio / analysis result | `listen.rs`, `crates/analysis` |
| **Passive mode** | an instance with **no patch loaded** (passthrough, controls disabled, a "passive listener" card instead of a surface) that exists to be a Listen target | `shell/passive.rs` |
| **Chat** | conversation with the LLM, captures as attachments; one in-memory chat per instance | `shell/chat.rs`, `assistant.rs`, `crates/llm` |

## Avoid

| Don't say | Say | Why |
|---|---|---|
| **page** (for a surface, or the backdrop) | surface | It meant three things at once: the block editor, the whole viewport content, and a modal's knob groups. Only **group page** survives |
| **block page** | block modal | It became a modal on 2026-11-23 ([`research/chain-editor.md`](research/chain-editor.md)) |
| **view** (as a noun for what's shown) | surface | `view` is Iced's method name; a `View` type beside a `view()` method reads as a mistake |
| **panel**, **ControlPanel** | patch surface | Two qualified uses survive: the **modulator editor**'s panel (`shell/dock.rs`) and *art* vocabulary inside a patch (a painted faceplate, `style.PANEL`). Bare "panel" is always ambiguous between those and the patch surface |
| **preset** | patch, or snapshot | Ambiguous between the two |
| **editor**, **chrome** | shell | — |
| **slot** alone | host slot, modulation slot, or container slot | Three different things (a band split's bands are container slots) |
| **master mode** | passive mode | — |
| **drawer**, **macro rail** | dock | Both were replaced by the mod dock on 2026-11-25 (`shell/dock.rs`); neither widget exists any more |
