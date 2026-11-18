# How Kontakt does instrument UIs (and the engine behind them)

_Research notes, 2026-11-18. Orientation for ScriptFX's patch/UI model; see also
[`patch-code-and-sharing.md`](patch-code-and-sharing.md)._

**Sources and confidence.** Facts with a link come from NI's manuals and developer docs, or (marked)
from community tutorials and forums. A few points are from memory and marked _unverified_; check them
before relying on them. NI's docs are spread over the KSP manual, the Kontakt manual and the Komplete
UI developer site, and some pages only appeared as fragments in search.

## 1. Terminology and architecture

| NI term | What it is | Our closest equivalent |
|---|---|---|
| **Instrument** (`.nki`) | one playable/processing unit: sample engine (groups, zones), effects, modulators, up to **5 script slots** with KSP, and a scripted UI | a **Patch** |
| **Multi** (`.nkm`) | several instruments loaded together (MIDI/audio routing between them) | — (one patch per plugin instance) |
| **Instrument Bank** (`.nkb`) | up to 128 instruments switched by MIDI program change | — |
| **Rack** | the area of Kontakt's window where loaded instruments sit (Rack view) | the shell's stage |
| **Library** | a product (collection of instruments + samples + resources); commercial ones are "Kontakt Player libraries" (licensed, run in the free Kontakt Player) | a folder of patches |
| **Resource container** (`.nkr`) | packed `Resources/` folder: `pictures/`, `scripts/`, `data/`, `ir_samples/`, fonts…, everything but samples ([resource container](https://docs.native-instruments.com/online-guides/ksp-manual/en/resource-container), [community](https://adsrsounds.com/kontakt-tutorials/use-a-resource-container-to-store-kontakt-files)) | the patch folder |
| **Performance View** | the instrument's scripted UI, shown in the rack in place of the raw engine editor | the **ControlPanel** |
| **Snapshot** (`.nksn`, Kontakt 6+) | saved state of an instrument's persistent values | **Snapshot** (planned) |
| **Creator Tools** | separate app: debugger, Instrument Editor (Lua scripts that build instruments: map samples, rename groups), GUI Designer for performance views ([Building in Kontakt](https://www.native-instruments.com/en/products/komplete/samplers/kontakt-8/building-in-kontakt/)) | our LLM authoring + `luau-lsp` |

([file formats](https://docs.native-instruments.com/online-guides/kontakt-manual/en/file-formats))

**Interop between shell and instrument:**
- Kontakt (the shell) owns the window, browser, rack, instrument header (name, volume/pan,
  output, MIDI channel, tune) and the **engine**: sampler, built-in effects, modulators.
- An instrument doesn't add DSP code. It **configures** Kontakt's engine modules (data) and
  **scripts** them with KSP. KSP runs as an event processor and parameter controller; the five script
  slots form a chain in which each slot sees the MIDI events the slots before it produced
  ([Sound on Sound](https://www.soundonsound.com/techniques/customise-kontakt-patches-how-create-scripts-ni-kontakt-part-1),
  [Script Editor](https://native-instruments.com/ni-tech-manuals/kontakt-manual/en/the-script-editor)).
- The UI is declared by the script (KSP widgets, or since 2025 Komplete UI) and drawn by Kontakt.
- Third-party libraries **can't add native code**: no custom DSP, no third-party plugins inside. All
  sound comes from NI's modules. That's why libraries are safe to share and sell (see the sharing
  research).

> This is structurally our model: a closed catalogue of native blocks, configured by data, controlled
> by a sandboxed script, with a script-declared UI drawn by the host.

## 2. Graphics: formats and assets

- **Raster only (classic):** PNG (and TGA) in `Resources/pictures/`. Every picture needs a **`.txt`
  sidecar** with the same name, or Kontakt ignores it; wallpapers, icons and picture fonts too
  ([community](https://danielrdehaan-website.pages.dev/garden/knowledge/sampling-instruments/kontakt/kontakt-resources/)).
  Fields (from community templates; NI publishes no full official reference that I could find):
  - `Has Alpha Channel: yes|no`
  - `Number of Animations: N`: number of frames; Kontakt splits the image evenly
  - `Horizontal Animation: yes|no`: direction of the frame strip
  - `Vertical Resizable` / `Horizontal Resizable`: whether width/height can be changed with
    `$CONTROL_PAR_WIDTH` etc.
  - `Fixed Top/Bottom/Left/Right`: 9-slice margins for resizable images
  - If the sidecar is missing, Kontakt creates a default one that you then edit
    ([forum](https://community.native-instruments.com/discussion/comment/79367)).
- **Sprite sheets / filmstrips, one file per control type:** a knob is one PNG with N frames (e.g.
  101 positions) stacked vertically; switches are 2- to 6-frame strips (off / on / hover / pressed…).
  Most "pretty" Kontakt UIs are **pre-rendered artwork** (3D-rendered knobs, photographed textures),
  not live drawing ([tutorial](https://www.adsrsounds.com/kontakt-tutorials/animate-a-label-with-a-press-of-your-keyboard/)).
- **Fonts:** custom fonts are bitmap fonts made from a PNG via `get_font_id()`
  ([KSP UI commands](https://www.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-commands.html)).
- **Wallpaper:** a PNG as wide as the performance view and its height + 70 px (the header area); an
  offset can be set with `set_skin_offset`
  ([tutorial](https://danielrdehaan-website.pages.dev/garden/knowledge/sampling-instruments/kontakt/kontakt-basic-user-interface-creation/)).
- **Size:** set with `set_ui_width_px` / `set_ui_height_px`. Max width was 632 px for a long time,
  now 1000 px ([NI community](https://community.native-instruments.com/discussion/comment/45745),
  [KSP UI commands](https://docs.native-instruments.com/online-guides/ksp-manual/en/user-interface-commands)).
  Fixed pixel sizes, so **instrument UIs are all close to the same size**, the same decision we made
  with 840×560.
- **HiDPI:** Kontakt 7 scales to the OS DPI, but classic instrument graphics are raster and get
  resampled (blurry), and old libraries were never updated
  ([Kontakt 7 FAQ](https://support.native-instruments.com/hc/fr/articles/6554022324637-Kontakt-7-FAQ),
  [community](https://community.native-instruments.com/discussion/comment/90383)). NI's answer was a
  new UI technology ("Native UI", released as Komplete UI).
- **Komplete UI images:** PNG and JPEG, sprite sheets with row/column counts, a resizable flag, and
  **an `@2x` variant next to every image for HiDPI** ([Image](https://developer.native-instruments.com/komplete-ui/docs/Packages/ui/Components/Image/)).

## 3. How much can a library influence its UI?

### Before 2025: KSP widgets

The script declares a fixed set of **widget types** ([UI widgets](https://native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-widgets),
[control parameters](https://docs.native-instruments.com/online-guides/ksp-manual/en/control-parameters)):
- **Value controls:** `ui_knob` (Kontakt's own look; barely skinnable), `ui_slider` (skinnable with a
  filmstrip, so in practice the custom knob), `ui_switch`, `ui_button`, `ui_menu`, `ui_value_edit`,
  `ui_text_edit`.
- **Display:** `ui_label` (text and/or picture, animatable by frame), `ui_level_meter` (attached to a
  bus), `ui_waveform` (shows a zone, slices, a table; flags for bipolar, MIDI drag; `attach_zone`).
- **Special:** `ui_table` (step editor), `ui_xy` (XY pad with several cursors), `ui_file_selector`,
  `ui_mouse_area` (drag-and-drop), `ui_panel` (grouping, z-order).

What a script can set per widget (`set_control_par` with `$CONTROL_PAR_*`): position, size, picture,
picture state (frame), font, text, alignment, colours for some widgets, z-layer, hide/show, tooltip
(`$CONTROL_PAR_HELP`), automation id and name, value range and default.

What a script **cannot** do: draw. There's no canvas, no paths, no shapes. Everything visible is a
widget showing a picture frame or text. Animation means the script switching frames, e.g. on
`on listener` timer ticks. Custom looks come from artwork, and complex effects from stacking labels
and frames.

Two side routes:
- **GUI Designer** (Creator Tools, Kontakt 6+): a WYSIWYG editor for performance views, saved as
  `.nckp` and loaded with `import_nckp` _(from memory, unverified)_.
- **Community tooling:** the Sublime KSP compiler and macro systems exist because raw KSP is verbose.

### Since 2025: Komplete UI

Kontakt 8.2 (Feb 2025) shipped Komplete UI 1.0
([version history](https://native-instruments.com/ni-tech-manuals/kontakt-manual/en/version-history)):
- **Komplete Script**, a new language: declarative UI code plus imperative logic, components and
  **modifiers**, callbacks as properties (`on_submitted: fun () {...}`), **reactivity**: property
  expressions, computed values and paint callbacks re-run when their dependencies change
  ([language tour](https://developer.native-instruments.com/komplete-ui/docs/LanguageTour/),
  [changelog](https://developer.native-instruments.com/komplete-ui/docs/CHANGELOG/)).
- **Layout:** `VStack`, `HStack`, `ZStack`, a `Position` modifier for absolute placement, a `Frame`
  modifier for sizes. Instrument size is set in Instrument Options; no performance view needed
  ([KSP → Komplete UI](https://developer.native-instruments.com/komplete-ui/docs/KSPtoKompleteUI/)).
- **Drawing:** `Canvas` with a paint handler that gets a `Painter` and the bounds. It offers
  `draw_path(Path)` and `draw_pie(frame, start_angle, end_angle)`, and the paint function re-runs when
  the state it reads changes. NI warns about performance and says to prefer other components where
  possible ([Canvas](https://developer.native-instruments.com/komplete-ui/docs/UI%20Module/Components/Canvas/)).
- **Primitives:** `Rectangle`, `Text`, `Image`, … Controls like buttons are **composed** from them; no
  one-to-one mapping to KSP widgets.
- **Packages:** `ui` (components, modifiers, `ui.Path`), `kontakt`, and the optional `kontakt_controls`
  (ready-made Knob/Slider/Switch that bind to KSP controls); NI explicitly says it "is not part of the
  framework" ([Kontakt Controls](https://developer.native-instruments.com/komplete-ui/docs/Kontakt%20Controls%20Package/),
  [Knob](https://developer.native-instruments.com/komplete-ui/docs/KontaktControls/Components/Knob/)).
- **Entry point:** the main module exports the UI (`export var main = ...`); top-level code runs once
  at load (no `on init`).
- **Tooling:** official VS Code extension (highlighting, hover docs, snippets), Kontakt Templates as
  starter content (8.11).
- **Status:** feature toggles under Options → Developer; parts are marked experimental
  ([Get Started](https://developer.native-instruments.com/komplete-ui/docs/GetStarted/)).

**The bridge to KSP:** all UI→engine data goes through **KSP UI controls**. A Komplete UI component
binds by `control_id` to a KSP control that still has to be declared (`declare ui_slider $my_slider
(0, 100)` and `expose_controls`). NI says this is "currently" the only exchange path
([KSP → Komplete UI](https://developer.native-instruments.com/komplete-ui/docs/KSPtoKompleteUI/)). So
the KSP control is the **parameter**, and Komplete UI is purely its **view**.

## 4. Parameters, automation, persistence

- **Parameters = KSP UI controls.** Knobs, sliders, switches and XY cursors are automatable by
  default (`$CONTROL_PAR_ALLOW_AUTOMATION`, init only); automation id and name are set per control
  (`$CONTROL_PAR_AUTOMATION_ID`, `…_NAME`; `set_control_par_arr` for XY cursors)
  ([control parameters](https://docs.native-instruments.com/online-guides/ksp-manual/en/control-parameters)).
  Kontakt exposes a fixed pool of generic automation slots to the host and the script assigns controls
  to them _(pool size unverified)_, the same contract as our 16 fixed macro slots.
- **No separate "macro" concept.** A "macro" is just a KSP control whose `on ui_control` callback
  sets several engine parameters (`set_engine_par`), often with curves done in script.
- **Engine parameters:** every module parameter (filter cutoff, effect mix, modulation intensity…)
  is reachable from KSP with `set_engine_par` / `get_engine_par` / `get_engine_par_disp` (display
  string) and `$ENGINE_PAR_*` ids, addressed by group, slot and generic index
  ([engine parameter commands](https://www.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameter-commands)).
- **Persistence:** `make_persistent` / `make_instr_persistent` mark variables and controls as saved
  with the DAW project and snapshots; `on persistence_changed` runs after state is loaded (instrument
  load, snapshot recall) ([callbacks](https://docs.native-instruments.com/online-guides/ksp-manual/en/callbacks),
  [community](https://community.native-instruments.com/discussion/comment/7780)).
- **Hardware integration (NKS):** Komplete Kontrol / Maschine hardware shows pages of 8 parameters
  per instrument, mapped from the automatable controls _(details unverified)_.

## 5. Modulation: LFOs, envelopes, envelope followers

- **Modulators are engine modules**, attached to a target parameter in the instrument. Internal
  sources: LFO, envelopes (AHDSR, DBD, flexible), glide, **step modulator**, **envelope follower**;
  external sources: pitch bend, aftertouch, MIDI CC, constant, random…
  ([modulation sources](https://www.native-instruments.com/ni-tech-manuals/kontakt-manual/en/modulation-sources),
  [modulation in Kontakt](https://native-instruments.com/ni-tech-manuals/kontakt-manual/en/modulation-in-kontakt)).
- **A modulation assignment** = source + target + intensity (and options like invert/shape).
- **From KSP:** a modulator is found by name (`get_mod_idx(group, "LFO 1 -> CUTOFF 1")`), and its
  assignment with `get_target_idx`; then intensity and modulator settings are set through
  `set_engine_par` (`$ENGINE_PAR_MOD_TARGET_INTENSITY`…). Names must be unique per group
  ([engine parameter commands](https://www.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameter-commands),
  [community guide](https://community.native-instruments.com/discussion/comment/37)).
- **Scripts can't create modulators** at runtime, only control the ones the instrument was built with
  ([VI-Control](https://vi-control.net/community/threads/how-to-create-modulation-modules-from-ksp.120360/)).
  Modulation topology is static data; the script only moves its knobs.
- Modulation runs per voice and at audio or control rate inside the engine. The script never computes
  modulation sample by sample.

## 6. MIDI, timing and events

KSP is **callback-driven** ([callbacks](https://docs.native-instruments.com/online-guides/ksp-manual/en/callbacks)):
- `on init`: declarations (all `declare` statements), UI setup.
- `on note` / `on release`: note on/off. The script can ignore, change and generate notes
  (`play_note`, `change_note`, `ignore_event`), which is how arpeggiators, chord tools and legato
  scripts work.
- `on controller` (CC, pitch bend, channel pressure), `on poly_at`, `on rpn` / `on nrpn`.
- `on ui_control($x)`: a control changed (one callback per control); `on ui_update`.
- `on listener`: **timer and transport** signals (ms ticks, beat ticks, transport start/stop), used for
  tempo-synced script logic and UI animation.
- `on persistence_changed`, plus `on async_complete` for asynchronous engine operations _(latter from
  memory)_.
- MIDI file objects let scripts load and play MIDI files (Kontakt 8's phrase/riff tools build on this).

## 7. What this means for ScriptFX

| Kontakt | ScriptFX today | Take-away |
|---|---|---|
| closed engine modules, configured by data | closed block catalogue (`Blocks.*`), configured by the patch | ✔ same model; it's what makes sharing safe |
| KSP controls = parameters, Komplete UI = their view, bound by id | Params/Macros as objects, components `bind` them | ✔ same split; ours binds by reference, not string id |
| pre-rendered filmstrips (classic) | SVG drawn live, cached per layer | consider an **image-strip component** (cheap, pretty, `@2x`) |
| reactive components + `Canvas` with paint callbacks + `Path` (2025) | reactive components, SVG output | ✔ we're already there; a path-based Canvas could be our "immediate" variant |
| fixed UI size (≤ 1000 px wide) | fixed 840×560 | ✔ same decision |
| HiDPI pain with raster art → `@2x` assets | SVG (resolution-independent) | ✔ our advantage; for bitmaps, accept `@2x` |
| modulators are static engine modules; script only adjusts them | planned: modulators | **design hint:** modulators as declared *blocks* (LFO, follower, step) with targets, not script-computed signals |
| fixed pool of host automation slots, assigned by script | 16 fixed macro slots | ✔ same contract |
| `on listener` timer/beat ticks | `animated` + `p.time`; host tempo/beat in the DSP | maybe expose transport (beat, playing) to UI components |
| VS Code extension, templates | luau-lsp + definitions file | ✔ same idea |
| no third-party native code in libraries | no native code in patches | ✔ keep it that way for shareable patches |

**The main lesson:** Kontakt's pretty UIs come mostly from **artwork** (filmstrips, wallpapers),
not live drawing, and NI only added a vector canvas in 2025, inside a sandboxed language. A
safe-primitives route (canvas/paths, image strips) is the industry's own answer, not a compromise.
