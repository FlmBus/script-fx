# ScriptFX: status, decisions & next steps

_Last updated: 2026-11-19 (decisions: global chats, graph editor; before: new shell in iced on nice-plug, in the "ScriptFX Shell" design system). Read this first when picking the project back up. Technical
details, measurements and run instructions are in [`README.md`](../README.md)._

## Where we left off

0. **Top bar = title bar + sheet openers** (2026-11-19, after the polish pass). "ScriptFX"
   wordmark centred like a window title; on the right Controls · Chat · ⚙ (all sheet openers in
   one place; Controls is disabled without a patch). New **settings sheet** (`shell/settings.rs`):
   theme (Auto/Light/Dark), interface size (− 100 % +), passive listener (disabled, "coming
   later"); these left the top bar. The notch is now reload | snapshots only. Icons: gear
   (Lucide, ISC) for settings, sliders for the patch controls; the old sunburst icon is gone.
   `SCRIPTFX_DEV_SHEET=settings` opens it. Design-system README not updated for this yet.
   - **Library = a sheet on the left** ("Patches" at the left of the top bar), so the title and
     the notch share the window's centre axis. Sheets have two slots now: left (library) and
     right (controls, chat, settings), mutually exclusive (Flemming); the notch and the empty
     card centre between them. A fresh instance (no patch) opens the library. The window is
     **836 × 640** (was 1040): the panel keeps its 816 × 544, the sidebar column is gone.
     `SCRIPTFX_DEV_SHEET=library`. Idea (Flemming): general sheets left, panel sheets right.
   - **Panel input:** a click (left or right) into the panel closes open sheets and the value
     entry; Escape too. Sheets mark their area as occupied, so clicks on their blank space don't
     fall through to the panel. **Knobs:** wheel turns them (2 % per notch), **Shift** = 5× finer
     for wheel and drag; **right-click** opens a value entry next to the knob (current value
     selected; typed in the control's unit, "6", "6 dB", "-3,5"; percent for a macro driving
     several params; Enter applies, Escape/click elsewhere cancels; a non-number shows a hint).
     Parsing: `patch::parse_number`, `ParamSpec::parse_plain`, `Patch::macro_from_text` (tested);
     pointer logic tested on Neon (`stage::tests`). Not checked by hand: real wheel/Shift/right-
     click in a host (hosts may eat Escape or keystrokes). `SCRIPTFX_DEV_SHEET=entry`.
0. **Polish pass on the shell** (2026-11-19, Flemming's list; two sessions, merged).
   - **Top bar across the window** (`shell/top_bar.rs`, 34 px): "ScriptFX" left; right: UI
     scale stepper, passive-mode switch (disabled, "soon" badge), **Chat** (global, see Decisions).
     The library header shrank to 36 px to sit under it.
   - **Letterbox gone:** the stage is now exactly 816×544 (3:2) at 1040×640, so the 840×560 panel
     fills it; test `stage_fits_the_panel` guards it.
   - **Notch = one row** of equal-height controls in groups split by hairlines: reload + LED |
     snapshot selector, save, load | patch controls. Chat moved out to the top bar.
   - **Chat is global:** opens from the top bar, also with no patch loaded (the empty card slides
     aside); unloading a patch keeps it open. Still one chat ("Main"), not saved yet.
   - **Passive mode** left the empty screen (now in the top bar).
   - **UI scale 50–200 % in 25 % steps** (`user_settings.rs`: `<data dir>/ScriptFX/settings.json`,
     keeps unknown keys, atomic write). Resizes the window (iced's `scale_factor` → host resize),
     doesn't scale inside a fixed one. Live resizing needed a baseview patch (fixed-size windows
     silently dropped app resizes; `vendor/baseview/PATCHES.md` #2). Checked in the standalone:
     opening at 150 %, and live 100 → 150 % and 150 → 75 % (`SCRIPTFX_DEV_RESCALE`).
     **Not checked:** in a host (resize via ConfigureNotify → `request_resize`), and first open at
     ≠ 100 % there: nice-plug-iced's `Editor::size()` reports the size without the zoom, which
     may need a vendored patch.
   - Neon Saturator meters: no box, bars and scale straight on the background.
   - **Theme switch: System / Light / Dark** (top bar, per user in `settings.json` as `"theme"`,
     default System). `theme.rs` holds two palettes with the same tokens (`DARK`, `LIGHT`);
     sections read the current one via `theme::c()`, so a switch restyles everything next frame.
     The sheets follow the mode too (dark sheets in dark, warm paper in light; Flemming's call
     after trying dark sheets on the light frame). Patch colours (library cards, ControlPanels)
     never change. System reads the OS through `dark-light` (XDG portal on Linux) on a helper
     thread every 3 s, never on the UI thread; checked here (portal says dark → dark). Shadows go
     through `theme::shadow()` (softer in light). The design system "ScriptFX Shell" has both
     themes now (42 colour tokens incl. hover/focus/edge/signal states, contrast notes); its
     README's layout parts (notch contents, no top bar) are still from before this pass.
   - **Fractional UI scales (125/150 %):** the notch is one canvas silhouette (flares + body as a
     single path, quarter circles as Béziers) under the controls, so no seam lines where the
     flares meet the body. Icons are rasterised at the exact physical size (resvg, cached) and
     shown as pixel-snapped images instead of iced SVGs, which blurred or lost thin strokes.
   - **Knob glow cut off** (Neon, Bass Helper): `SVG.arc{glow}` sized its blur filter relative
     to the arc's own bounding box, so a short arc (knob turned down) clipped its glow into a box,
     worst on hover. The filter region is now absolute (`userSpaceOnUse`, circle + 3σ), in the
     shared prelude, so every patch's glowing arcs get it. Second cause (dragging): the engine
     clipped each layer to its component's box, and the drag glow reaches ~21 px past an arc
     10 px from the edge. Layers may now overflow their box by up to 32 px (hit areas unchanged),
     but never the panel's edges (Junkyard's filtered background smeared over the shell;
     `no_layer_leaves_the_panel` checks all factory patches at 1×, 1.5×, 2×).
   - Library scrolling: thin scrollbar appears with 15 patches; wheel scrolling needs a hand check.
0. **New shell built** (iced on nice-plug, design system "ScriptFX Shell", skin Matte + Paper
   chat; research + spike: [`research/shell-ui-toolkit.md`](research/shell-ui-toolkit.md)).
   - Framework moved **nih-plug → nice-plug 0.4.2** (vendored; our runtime-parameter-name patch
     ported, `vendor/nice-plug/PATCHES.md`). `initialize()` is `activate()` now.
   - `crates/plugin/src/shell/`: `library` (cards in the patch's colours), `notch` (snapshot
     selector, save/load disabled until snapshots exist, reload + sync LED, chat, settings),
     `stage` (ControlPanel layers as iced images, pointer widget for knob drag/click/double-click
     reset, rounded window corners forced on every panel), `chat` (Listen inside; capture analysis
     posted as a markdown table and attached to the next message; no LLM yet, it says so),
     `controls` (Settings: macros, params, hot reload), `signal_line` (from the chain; routing
     blocks as lanes; amber LED while a metered block works), `empty` (fresh instance = no patch,
     passthrough), `session` (library scan, load/reload/unload, hot reload, values), `slide`
     (sheet slide-in), `theme` (design tokens). The egui shell and the egui GPU spike are gone.
   - Fresh instances start **empty** now (no auto-load). `SCRIPTFX_DEV_PATCH` loads one for dev.
   - Frame pacing on this machine: baseview's X11 Present pacing never fires under XWayland →
     vendored baseview always uses its frame timer; iced_baseview's poll timer must be slow
     (250 ms), at 1/64 s it fired ~1000×/s. Result: ~70 fps, 9–13 % CPU with an animated panel
     (`vendor/baseview/PATCHES.md`). vsync stays off (it would block the host's UI thread).
   - `vendor/iced_wgpu`: frame timing incl. GPU timestamps (`SCRIPTFX_DEV_STATS=1`).
   - Factory install now removes factory files that left the bundle and renames Lua 5.4
     leftovers (`x.lua` next to `x.luau` broke `require`) to `.lua.bak`.
   - Checked: standalone (all five patches, chat/controls sheets, empty state) and VST3 in Carla.
     **Not checked by hand yet:** Reaper, clicking/typing (synthetic input doesn't reach XWayland
     windows here), close/reopen, two instances, HiDPI, Windows/macOS.
0. **Block catalogue built** (50 blocks; modulators — LFO/envelope as sources — are next, to be
   planned properly first).
   - Utility `Pan Width MonoBass Haas Invert DCBlock Meter`, Routing `MidSide LeftRight`; then
     Distortion `Waveshaper Wavefolder Clipper Rectifier Bitcrusher RingMod Tape`, Filter
     `MorphFilter LadderFilter CombFilter Formant Resonator DynamicEq`, Modulation `Chorus Flanger
     Phaser Tremolo AutoPan`, Space `Delay Reverb Shimmer`, Pitch `PitchShift Granular`, Rhythm
     `Stutter`, Dynamics `Limiter Gate DeEsser`. See README "Patch model" for details and costs.
   - Not built: `Ducker` (needs a sidechain input). Stutter is macro-triggered only (no automatic
     "every n bars" yet; would need the host's beat position).
   - New option kinds in the prelude: string enums (`"mode:half|full"`), `transfer` (curve function
     or list → sampled), `notes` (names → Hz), `sync` (note length → beats; host tempo in `Ctx`).
   - Things the tests caught: MonoBass image shift (HP phase), de-esser `x − HP(x)` not cancelling
     (now a dynamic shelf), delay feedback chain latency shifting the first echo (now written into
     the past), shimmer outer loop > 1 (scaled by 1 − line gain + soft limit).
   - **Block categories as doc comments** in `scriptfx.d.luau` (`--- Utility · …`): luau-lsp shows them
     on hover/completion, the LLM reads the same file; `crates/patch/tests/block_docs.rs` checks that
     every prelude block is documented with a known category. No runtime tag field (nothing uses it).
   - **Oversampling per block, implemented once**: every block takes `oversampling` (nonlinear ones
     automatically ≥ 88 kHz); a generic `Oversampled` node wraps it, the block only sees a higher
     `sr`. Linear-phase FIR → latency (35 samples at 2×): nodes report it, routing blocks delay
     shorter branches, the plugin reports the total to the host. Neighbours with the same factor
     share one up/down pair. (First built whole-graph by mistake; the user meant "every block can".)
   - Block model: `BlockSpec { id, oversampling, kind: BlockKind }` (header + kind).
   - MonoBass keeps mid and side phase-aligned (allpass on mid).
   - Ideas, not built: a shell-level "quality" switch that scales the automatic factors (rebuild +
     swap the graph; fixed latency possible by padding to the highest factor); merging/SIMD for
     cheaper filters.
1. **Ported patches to Luau** (decided after a spike, see below). Patches are `init.luau` +
   UI files it `require`s; no more `ui.lua` entry point.
   - **References instead of strings in the UI:** `init.luau` passes the real objects into the UI:
     `Patch.new(name, { ..., panel = ui.Panel.new({ tape = tape_macro, gr = fet_block }) })`;
     components use `bind = p.tape` (Macro/Param) and `source = p.gr` (metered block) or
     `Meters.IN/OUT`. Objects carry `__ref` ("macro.tape") which the engine/shell resolve as before.
   - **Relative `require`** (Luau require-by-string), restricted to the patch folder by our
     `PatchRequirer` (`crates/patch/src/runtime.rs`); `.luaurc` files are ignored at runtime.
     UI files can be split freely (e.g. `./components/knob`).
   - **One runtime for DSP and UI** (`patch::runtime::Runtime`): both evaluate `init.luau` (the UI
     side keeps the state; hot reload clears mlua's require cache and re-evaluates, local
     component state survives). Both preludes live in the `patch` crate (`prelude.luau`, `ui_prelude.luau`).
   - **Sandbox:** Luau sandbox mode (read-only globals/builtins), `loadstring/getfenv/setfenv` removed,
     memory cap, interrupt budget 500k/call (measured peak of factory patches: 28.5k, Junkyard).
     The Luau **parser** is locale-sensitive (`0.08` fails under de_DE), covered by the existing
     `CNumeric` guard around every call.
   - **Types:** `patches/.types/scriptfx.d.luau` (definitions) + `patches/.luaurc` (strict). All
     patches type-check with zero findings; `cargo test -p patch --test typecheck` runs
     `luau-lsp analyze` (skipped without luau-lsp). Strict mode was chosen because nonstrict misses a
     string where a Macro belongs and a non-metered block as `source`. Conventions this needs:
     annotate `render = function(p: Props<{ ... }>)`, annotate helper function params, use records
     instead of mixed tuples, own props must not reuse input names (`value`, `display`, `level`, …).
   - Renders are pixel-identical to the Lua 5.4 versions except Junkyard's hand-lettering jitter
     (Luau's `math.random` is a different generator). Lua time per frame is slightly lower.
   - Decided against: an `extend`/parts mechanism for components. A standard component library
     (`@std/...` served by our requirer) is possible but not decided.
2. Before that (same session): the Lua API redesign (builder objects, namespaces, separate block
   types, `BandSplit` N-band + `Parallel`, fixed 840×560 panel, old SVG panels removed).
3. **Not yet checked in a host:** everything since the locale fix. Headless renders + all tests pass.
4. **Open:** shell-pill safe area (with the v7 shell build); unknown *extra* props aren't caught by
   the type checker; luau-lsp in CI (needs an install step in `build.yml`); the LLM loop should run
   `luau-lsp analyze` on generated patches and feed type errors back. Factory install leaves the old
   `patch.lua`/`ui.lua`/`panel.svg` files in the user data dir (ignored, harmless).

## Vision

"Max for Live with an LLM": an effect plugin where **patches are Lua scripts** that
*declaratively* describe an effect chain built from native blocks, the host-facing macros,
and the patch's own UI. An LLM (prompt / chat inside the plugin) writes and adjusts these
patches, informed by an analysis of the actual audio on the track (and the mix).

## Glossary (used in code, UI and docs)

| Term | Meaning | Code |
|---|---|---|
| **Shell** | the plugin itself and its own UI (frame, header, drawer, status bar) | `crates/plugin/src/shell.rs` |
| **Patch** | a Luau script describing a chain, its params/macros, theme and panel (`return Patch.new(...)`) | crate `patch`, `Patch`, `patches/<name>/init.luau` |
| **Snapshot** | saved values of a patch (macros + free params). Not implemented yet | — |
| **ControlPanel** | a patch's own UI: reactive Luau components, fixed 840×560; the `panel` of `Patch.new` | UI files required by `init.luau`, `ui_components::Ui` |
| **Component** | one visual part of a ControlPanel; each instance is a cached layer | `Component.define`, `X.new {}` |
| **Chain / Block** | DSP description / one building block (`Chain.new`, `Blocks.<Type>.new(id, {...})`) | `patch::chain`, `BlockSpec` |
| **Graph** | compiled chain running on the audio thread | `dsp::Graph` |
| **Param** | internal value of a patch (unlimited, saved with the project) | `ParamSpec` |
| **Macro** | host-automatable control of a patch, drives 1..n params | `MacroSpec` |
| **Slot** | one of the 16 fixed host parameters macros are mapped onto | `MACRO_SLOTS` |
| **Bind / Source** | what a component reads: `bind` = a Macro/Param object (value, draggable), `source` = a metered block or `Meters.IN/OUT` | `__ref`, `patch::Source` |
| **Theme** | a patch's colours (ControlPanel only, never the shell) | `Theme` |
| **Listen / Capture / Analysis** | the action / recorded audio / analysis result | `listen.rs`, `crates/analysis` |
| **Passive mode** | the instance only listens (no patch, passthrough) and shares its audio with other instances. First use: the master; later other channels | not implemented yet |
| **Chat** | conversation with the LLM, captures as attachments. **Global** (not per patch): one default chat, more can be opened | not implemented yet |

Avoid: "preset" (ambiguous between patch and snapshot), "editor"/"chrome" (use shell), "panel" alone (use ControlPanel), "master mode" (use passive mode).

## Decisions made

| Topic | Decision | Why |
|---|---|---|
| Language | **Rust** | readable for a Go dev, compiler catches real-time mistakes (`assert_no_alloc`) |
| Plugin framework | **nice-plug** (community continuation of nih-plug), vendored + patched (`vendor/nice-plug`, `vendor/nice-plug-core`) | nih-plug is in maintenance mode; nice-plug maintains the iced adapter. Before: nih-plug (vendored); clack + clap-wrapper lost its advantage once we went VST3-first |
| Formats | **VST3 first** (CLAP built for free) | all 3 machines (Fender Studio/Win, Studio One 6/Mac, Reaper+Carla/Linux) speak VST3. No AU, no AAX |
| Platforms | Linux x86_64, Windows x86_64, macOS universal (arm64 + x86_64) | via GitHub Actions |
| Lua's role | **describes only** (params, macros, chain, theme) and renders the UI components (UI thread, sandboxed + instruction budget); never on the audio thread | real-time safety, LLM-friendly validation |
| Patch model | **params** (internal, unlimited, saved with project) + **macros** (max 16, host-automatable, each drives 1..n params with range + lin/exp curve, Ableton-rack style). A param is free *or* driven by exactly one macro | 8–16 macros are enough when macros can drive many params; hosts can't handle changing parameter lists |
| Host parameters | 16 fixed slots, **names follow the patch** (patched nice-plug: VST3 `kParamTitlesChanged`, CLAP `RESCAN_INFO`). **Verified in Reaper.** Automation stays on the slot across patch changes (same contract as Serum's macros) | plugins can't delete host automation; renaming is safe because hosts key automation by ID |
| DSP | declarative `chain` → `dsp::Graph` compiled off the audio thread, swapped lock-free (rtrb), old graphs freed off-thread | |
| Patch UI | **reactive Lua components** → SVG → one cached layer each (resvg), fixed 840×560 | see "Patch UI" below; the earlier SVG + bindings panel was removed |
| Block tags / categories | Doc comments in the definitions file (`--- <Category> · …`), checked by a test | LSP hover/completion and LLM search from one place; no runtime field until something needs it |
| Oversampling | Per block via one generic wrapper (`oversampling` on every block, auto for nonlinear ones); linear-phase FIR + latency compensation (PDC) | only blocks that need it pay; linear phase + branch delays keep parallel blends from comb filtering; PDC is needed anyway for lookahead/pitch blocks |
| Script language | **Luau** (mlua `luau` feature) with luau-lsp type checking (strict) | relative `require` (per-patch UI files, no name collisions), typed props across files, built-in sandbox; LuaLS couldn't resolve per-folder requires or infer prop types |
| Lua API style | builder objects; identity positional, settings in an options table; namespaced; enums as strings; separate block types instead of a `type`/`style` option | readable, one way to do things, typo-proof for LLMs |
| Plugin's own UI (shell) | **iced** (`nice-plug-iced`, wgpu), one module per section | clean, sectioned code; wgpu (no GL); see `research/shell-ui-toolkit.md` |
| Fonts | bundled Noto Sans (+ optional `<patch>/fonts/`) | system font scan took 7.5 s cold |
| Factory patches | compiled into the binary, installed to `<data dir>/ScriptFX/patches`, **updated unless user-modified** (`.factory-manifest`) | |
| Strict patches | unknown fields are errors | catches typos (`treshold`), important for LLM output |
| Listen / analysis | user-triggered capture of the chain's **input and output** (10/20/30 s, silence skipped) → **all** analyses always go to the LLM (~2.5–3k tokens). A summarising LLM pass is a later optimisation | |
| Mix context | only **current track + master** (no buses: buses/sends would skew a track-vs-bus comparison). The master instance runs in **passive mode** | universally applicable, fewer options |
| Chat model | **Global chats**, stored in the user data dir (with model auth/config); one default chat, more allowed (decided 2026-11-19) | per-patch chats would have to be saved per patch; a chat can span several patches |
| Patch kinds | **Graph patches** get a visual graph editor: a new, data-only chain definition next to the coded Luau patches (decided 2026-11-19; design open, see "Direction") | Luau code can't be edited visually (loops/helpers don't round-trip) |
| LLM integration (planned) | **Layer 1**: the user attaches a capture to the prompt. **Layer 2**: the LLM gets tools: `get_analysis(section, detail)` and `request_listen(where)` → closed loop: write a patch, listen to the output, compare, adjust | the LLM can't press play; the human always plays the track |

## Shell layout

Designed in the canvas "ScriptFX Shell Mockup" (page "Shell", skin Matte + Paper chat) and the
design system "ScriptFX Shell" (tokens, components). **Built** (2026-11-18) in iced, see "Where we
left off". The macro/param sliders live in the controls sheet (Settings in the notch).

## Patch UI: reactive Lua components (decided 2026-11-18, prototype built)

After several mockup rounds (canvas: "ScriptFX Shell Mockup", v1–v7):

- **Shell layout (v6/v7):** plain text library on the left (`▶ Patch` / indented snapshots,
  sections styled with the patch's background art + logo), the patch's ControlPanel fills the
  stage, a **two-row shell pill** at the top centre of every ControlPanel (row 1: snapshot ▾,
  save, load · row 2: reload, chat, settings), thin signal line below. Listen lives in the chat.
  Chat is per patch (drawer over the panel). Passive mode is offered on the "No patch loaded" screen.
  Shell style: native-looking (system font, dense, bevels, etched lines), not web-like.
- **Patch UI = reactive Lua components** (`crates/ui-components`): every visual part (background,
  logo, knobs, meters, displays, tabs) is `Component.define { kind?, size?, animated?, on_click?, render(p) }`;
  `render(p)` returns SVG strings and child instances. One cached image per component instance;
  it re-renders only when its inputs change (props, bound value, meter, hover/drag, local state,
  `time` if animated). Children are separate layers (a knob drag never re-renders its parents).
  Behaviour (drag, fine, reset, click) is the shell's, per `kind`. Lua runs sandboxed; the output
  is SVG for resvg (no scripts, images only from the patch folder). Fonts: images/SVG instead of
  patch fonts (bundled Noto for text).
- **Stress test results** (`cargo run --release -p ui-components --example stress`, Neon, 840×560):

  | scenario | @1x | @2x |
  |---|---|---|
  | idle (animated LED only) | 0.12 ms | 0.26 ms |
  | drag a knob | 0.8 ms | 1.9 ms (max 4.5) |
  | playing (meters + drag + LED) | 1.4 ms | 2.7 ms (max 6.7) |
  | first frame / rescale (window open) | 44 ms | 150 ms |
  | 120 knobs, one dragged / all automated | 0.35 / 34 ms | – |
  | animated full-size background (worst case) | 4.2 ms | 17.6 ms |
  | spectrum display 256 pts every frame | 2.0 ms | – |

  Faulty components (Lua error, endless loop, invalid SVG, memory hog) show an error box; the
  rest keeps running. Hot reload keeps local state. In the plugin: ~1.35 ms/frame while playing.
- **Authoring rules learned:** split static and dynamic parts into separate components (knob body
  vs. value arc, meter frame vs. bars); avoid blur filters on large areas (fake glows with stacked
  strokes); keep filter regions tight (`SVG.blur(std, pad)`, `SVG.arc{glow}` does this); prefer a
  static background + small animated components over an animated background.

- **Fire test: "Junkyard Amp"** (skeuomorphic wood/tape/chicken-head knobs, everything procedural
  in Lua, no image files): looks right, ~300 SVG elements in the static background (33 ms once @1x,
  90 ms @2x). It exposed a cost problem that an LLM would hit too: dynamic sub-components sized like
  their parent rasterise lots of empty pixels. **Fix in the engine: every layer is cropped to its
  content bounding box** (only that area is rasterised). Drag @2x went from 11.3 ms to 4.0 ms;
  Neon's rasterised pixels halved. Plus: drop shadows on moving parts as offset copies, not blur.
  `cargo run --release -p ui-components --example render -- <patch> out.png [scale] [bind=v] [hover=x,y]`
  (`BENCH_DRAG=<bind>` times a drag).

- **Locale bug (fixed):** never rely on the process locale – the host owns it (Qt hosts set the
  user's locale). All Lua runs under `patch::locale::CNumeric` (per-thread C numeric locale). Keep
  this in mind for anything formatted for SVG, files or the LLM.

**Open / next (components):** first-open cost at 2x (150 ms) → persistent raster cache or
background-thread rendering; bool/enum params for toggles/selectors (toggles are local UI state
only so far); `p.series` data (spectrum) from the analysis engine; test an LLM writing a patch + UI from the
type stubs alone;
hit areas are rectangles; `measure_text()` helper if layouts need text widths.

## What exists (commits on `main`)

- `crates/patch`: model + sandboxed Luau runtime (`runtime.rs`: sandbox, budget, patch-folder `require`) + patch & UI API (`src/prelude.luau`, `src/ui_prelude.luau`) + chain spec + `tests/typecheck.rs` (luau-lsp)
- `crates/dsp`: graph engine; blocks `Gain`, `PeakEq/LowShelf/HighShelf/LowPass/HighPass`, `Drive`,
  `Compressor/FETCompressor/OptoCompressor`, `Rider`, `Transient`, utilities (`Pan Width MonoBass Haas
  Invert DCBlock Meter`), routing (`MidSide LeftRight BandSplit Parallel`), and `src/fx/` (distortion,
  filter, modulation, space, pitch, rhythm, dynamics: 28 blocks behind the `Fx` trait); per-block
  oversampling + latency compensation; host tempo for `sync`; `tests/blocks.rs` behaviour tests
  (`oversample.rs`); block meters, no-alloc test (bundled patches + `tests/fixtures`), bench (`--example bench`)
- `crates/ui-components`: **reactive component UIs** (renders the patch's `panel`; the Lua side
  `Component.define`/`SVG.*`/`Util.*` lives in `patch/src/ui_prelude.luau`; engine with per-layer cache, content-bbox crop, hit testing, local state, error isolation, bundled
  fonts, sandboxed images); examples `stress` (benchmarks) and `render` (PNG + per-layer cost); tests incl. locale regression
- `crates/analysis`: loudness (EBU R128, verified against ffmpeg), 1/3-oct spectrum, tilt, centroid, energy shares, resonances, dynamics (percentiles, per-band spread, onsets, attack), stereo, pitch (YIN)
- `crates/plugin` (package `scriptfx`): nice-plug plugin, iced shell (`src/shell/`, see "Where we left off"), host-name updates, lock-free capture, Listen; dev env vars `SCRIPTFX_PATCHES`, `SCRIPTFX_DEV_PATCH`, `SCRIPTFX_DEV_SHEET`, `SCRIPTFX_DEV_STATS`, `SCRIPTFX_DEV_RESCALE`, `SCRIPTFX_TEST_INPUT`, `SCRIPTFX_DEV_AUTOLISTEN`
- patches (all with component UIs): `tape-glow`, `clean-gain`, `bass-helper`, `neon-saturator`, `junkyard-amp`
- `vendor/baseview` (0.4.2): X11 frame pacing by timer; `vendor/iced_wgpu` (0.14.0): frame timing; `vendor/nice-plug*`: runtime param names
- `spikes/shell-iced`: the shell spike (throwaway, own workspace)
- CI workflow written, **not run yet** (repo isn't on GitHub; `gh` isn't installed)

## Key findings (details in README)

- Performance: bass-helper (11 blocks) = **0.7 % of one core**; chain complexity is not a concern.
- Old SVG panel: full redraw ~5 ms @1x / ~13 ms @2x. Solved by the component system (per-layer cache): ~1.4 ms @1x / ~2.7 ms @2x while playing.
- Hosts control the process locale → Lua number formatting must be forced to "C" per call (done).
- Linux: the shell renders with wgpu/Vulkan, so the old GLX problems are gone. XWayland: baseview's Present pacing never fires (patched to its frame timer). JACK standalone on PipeWire doesn't process; use `-b alsa`.
- Chain model gaps: no sidechain/modulators/lookahead, click on graph swap. (Param boilerplate is smaller now that params are objects declared inline, e.g. in loops.)
- Analysis caveats: "resonances" include notes/harmonics of tonal sources; onset detection misses soft re-attacks at equal level; the rider uses an absolute target level.

## TODO (prioritised)

### Flemming's list (2026-11-19)
- ~~Scrollable library~~ (scrollbar checked with 15 patches; wheel by hand still open).
- ~~Notch layout~~ (done 2026-11-19: one row, see "Where we left off").
- ~~Passive mode is too hidden~~ (done 2026-11-19: switch in the top bar; the mode itself isn't built).
- **LLM research:**
  - cheap or ideally free language models for this job (writing Luau patches, reading analysis JSON);
  - how to implement it: support all providers (one API layer) or pick a free one for now;
  - chat model: **decided 2026-11-19: global.** One default chat, more can be opened; chats, model
    auth and LLM config live in the user data dir, not in the patch or the DAW project. The chat
    button moves out of the notch into a top bar, next to passive mode.
  - **Adapter decision (2026-11-19):** own `Backend`/tool layer in a new `crates/llm`; patch-domain
    tools (`read_patch`, `write_patch`, `validate_patch`, `delete_patch`, later `render_panel`) are
    ours and run either in-process over HTTP (genai, or a minimal OpenAI-compatible client) or inside
    an installed CLI (`claude -p`, `codex exec`) for plan-backed quotas; ACP
    (`agent-client-protocol`) later for "any installed agent". Verification is always the loader +
    diff, not the backend. Research, candidates, the file-access reasoning (studio Mac has no AI
    software) and the Claude subscription/ToS caveat: [`research/llm-adapter.md`](research/llm-adapter.md).
- **Standard library of ready-to-use, generic panels** (patches without their own UI get a good
  panel for free).
- ~~Dark bars at the top/bottom of panels~~ (done 2026-11-19: stage is exactly 3:2).
- ~~Neon Saturator meters~~ (done 2026-11-19).
- ~~UI scaling~~ (done 2026-11-19: resizes the window; host checks open, see "Where we left off").

### Next up
1. **Hands-on check of the new shell in Reaper and Carla**: knob drag/double-click on all five
   panels, chat typing (Enter/Shift+Enter, selection, clipboard, dead keys), Listen in the chat,
   top bar (chat toggle, UI scale −/+ live and reopening at ≠ 100 %, host accepts the resize),
   library wheel scrolling with many patches,
   controls sheet sliders (host automation gestures, names), hot reload + error card, unload →
   empty state, close/reopen ×10, two instances, HiDPI.
2. **Snapshots** (saved values of a patch): notch selector/save/load and the library rows are
   placeholders ("Init" = defaults) until then.
3. Bool/enum params, raster cache, shell-pill safe area (see "Open / next (components)").
4. **Passive mode**: toggle on the empty screen; bypass, keep a rolling ~30 s buffer; a track instance's listen pulls the passive instance's audio for the same time window (host transport position) and adds "track vs. mix" analysis. In-process registry first (Reaper/Studio One/Fender Studio run plugins in-process); sandboxed hosts need IPC. Channel names: CLAP track-info / VST3 IInfoListener (nice-plug has `track_info_updated`), fallback: user-named instances.
5. **LLM integration**: chat → patch generation (layer 1 with attached capture), then layer-2 tools (`get_analysis`, `request_listen`). Bring-your-own API key. Validate generated patches with the strict loader; feed errors back to the LLM.

### UI plan (decided 2026-11-18): raster layers + sprite sheets + shaders
[`docs/research/raster-ui-plan.md`](research/raster-ui-plan.md):
- The reactive component UI is scrapped until there's a solid plan.
- Artwork is PNG or SVG (SVG rasterised); sprite sheets are 2D filmstrips (`rows × columns`).
- Named layers rendered back to front: content (optional own effect), generated (shader) and
  adjustment (shader over everything below).
- A shell setting toggles effects and animations (see TODO "Effects off").
- First step: spike the GPU compositing path (egui glow paint callbacks, naga WGSL → GLSL) in Reaper
  and Carla.

### Direction: how much code do patches get? (open; decide before more UI work)
Research (precedents, Kontakt, WebAssembly/WebGL, threat model, spikes, decision matrix):
[`docs/research/patch-code-and-sharing.md`](research/patch-code-and-sharing.md). Short version:
ReaScripts and Max for Live share fully unsafe code; Kontakt (the closest product, with Komplete UI's
reactive components and Canvas paint callbacks) and Roblox share code safely through the API boundary.
Recommendation: grow visuals through safe primitives (canvas/paths, filmstrips, maybe shaders); put
anything unsafe behind an opt-in "trusted patch" capability.

**Decided (Flemming, 2026-11-19): there will be a graph editor for patches**, implemented as its own
kind of chain definition (data, no logic), next to the coded Luau patches. How exactly is open:
- format and where it lives (a Luau table of data, or JSON; a file in the patch folder);
- how the two kinds relate: can a graph patch have a coded ControlPanel, or does it always get a
  generic one? Can a Luau patch be converted to a graph (only when it has no logic)?
- macros/params in the graph (nodes, or a side panel);
- what the LLM writes for which kind; the graph is easier to validate and diff;
- the editor itself (a shell view; nodes from the block catalogue, routing blocks as containers?).

Three routes:
1. **Stay as is (middle ground):** sandboxed Luau, declarative chains, component UIs drawn as SVG.
   Programmatic flexibility balanced against safety from malicious patches.
2. **Graph only, no code:** drop the coded ControlPanels in favour of a generic node-graph view
   (Max for Live / FL Studio Patcher style).
   - Graphs are defined statically (data, no code). That isn't necessarily bad; it depends on the
     plugin's vision.
   - Simpler, more stable, easier to use, more performant and resource-efficient.
   - Cheaper for patch authors, because everything can be generic.
   - Users edit the graph to change sound/behaviour, or to tap the chain anywhere (e.g. a meter).
3. **Full code-your-plugin sandbox:** go all out on UIs.
   - Patches draw to a canvas or GL directly, or use web views etc.
   - Rules out sharing patches between users: a patch could contain malware.

Open question: **will people want to share patches?**
- Flemming builds this primarily for himself and wouldn't share or import other people's patches.
- For others it could be a cool feature.

Idea: a **locked-down mode** for foreign patches. It loads no potentially unsafe code (e.g. only the
declarative chain, params and macros) and shows a generic UI instead of the coded one.

Notes:
- **Visual editing is the real gain of route 2.** Today the Luau code is the source of truth. The
  exported `BlockSpec` tree is static, but only a *result* of running the code, so a graph view on
  top of today's model could display the chain but not edit it: edits can't flow back into code
  with loops, helpers and computed values.
- A visually editable graph needs the graph data itself to be the source of truth (e.g. a
  JSON/Luau-table patch file without logic). Users, the editor and the LLM all write that same data.

### Effects off switch + GPU safety (TODO, part of the raster UI work)
- **Shell setting "Effects off":** disables shaders, effects and animations for all patches.
  Fallback: layers without their effects, static frames, generated layers as a still image or plain
  colour. Persisted per user (not per project).
- **Automatic fallback per patch:** measure GPU time per frame; if a patch exceeds its budget, or a
  pass doesn't finish (fence timeout, e.g. a runaway shader), switch that patch to "effects off" and
  show why, instead of retrying every frame.
- **Validation:** reject shader loops without a constant bound (or inject an iteration cap); cap
  passes per patch and the render scale.
- Background: [`raster-ui-plan.md`](research/raster-ui-plan.md) (stress test, runaway shader test).

### Analysis / debug block (idea)
A block that shows a general-purpose analysis (level, spectrum, stereo, dynamics) of the audio going
into it, anywhere in the chain. The Listen feature could tap into the chain through these blocks,
not only at the plugin's input/output.

### UI redesign (plugin chrome)
- **Chat** (decided 2026-11-19: **global**, one default chat + optional more, opened from the top bar; captures as message attachments; Listen lives in the chat). The sheet stays a drawer over the patch window.
- **Persist chats: decided 2026-11-19, in the user data dir** (with model auth and LLM config; keys
  in the OS keychain per `research/llm-adapter.md`). Open: should a generated patch link back to
  the chat that made it?
- Shell follow-ups: letter-spacing (IN/OUT, tracked titles) isn't supported by iced text;
  accessibility labels for icon buttons (no iced equivalent yet); full font files (the bundled
  ones are Latin subsets, "→" falls back); per-node clip LEDs (no per-block peaks); library card
  background art (cards use a gradient from the theme colours).
- Done 2026-11-18/19: thin overlay scrollbars (paper on the sheets, frame on the library).
  **Theme restructured** (2026-11-19): `theme = { colors, fonts, images, library }`. `colors`
  (`#rrggbb`), `fonts` (`Font.new(path)`: ttf/otf/svg/fnt) and `images` (`Image.new(path)`:
  svg/png) are the patch's own tokens, shared by the library card and the ControlPanel
  (`p.theme`); the shell uses `colors.accent/bg/ink`, `fonts.title`, `images.logo`;
  `theme.library = { accent, bg, ink, font, logo }` overrides them for the card only. Strict
  keys and errors that name the field; paths checked at load. Factory: Neon (Orbitron), Tape
  Glow (Playfair Display italic), Junkyard (label-tape BMFont), Clean Gain (SVG font), Bass
  Helper (SVG wordmark logo); licences next to the fonts.
- **Shell UI reboot: decided 2026-11-18, iced** (`nice-plug-iced`, wgpu), one module per shell
  section (Elm architecture: state, messages, update, view) so the UI code stays clean while we
  tinker. [`research/shell-ui-toolkit.md`](research/shell-ui-toolkit.md). Not GPUI (no embedding
  into host windows, owns the event loop, no hook for our GPU passes). Fallback: Slint. Migrate
  nih-plug → **nice-plug** first. Earlier shortlist was Slint + vizia; on
  recheck the Slint bridge is WIP/OpenGL-only and vizia's bridge is now third party (nightly).
  Independent of the patch UI plan (the shell only hosts the compositor's render hook).
  Spike (`spikes/shell-iced`) → **built into the plugin** (see "Where we left off").

### Patch isolation (assets and state)
Principle (Flemming, 2026-11-19): patches are isolated from the shell and from each other.
- **Isolated:** Lua runtime per patch (sandbox, memory cap, instruction budget); `require`, panel
  images and `theme` paths confined to the patch folder; panel SVG text uses a per-patch font
  database (bundled + the patch's `fonts/`); patch fonts in the shell get private family names
  (patch folder + content hash), so they can't restyle the shell or each other (fixed 2026-11-19).
- **Not yet:** chat log and the attached capture are per plugin instance (chats are global by
  design now, so surviving a patch switch is right; they still need saving to the user data dir);
  captures go to one shared folder; param values of
  a patch are dropped on switch (fine now, needed per patch once snapshots exist).
- **Entry file renamed `patch.luau` → `init.luau`** (2026-11-19): less opinionated (it doesn't
  have to contain the whole patch). The patch folder is loaded as a Luau module (`require` of the
  folder): `init.luau`, else `init.lua`, both = error. Luau's init rule applies (same in the
  runtime and in luau-lsp): inside `init.luau` the patch's own files are `require("@self/ui")`;
  `./x` would be next to the folder (blocked). Old unmodified `patch.luau` files in the user data
  dir are removed by the factory install.
- **Patch UIs split into `ui/` module folders** (2026-11-19): `ui/init.luau` (the Panel) requires
  one file per component family (`@self/knob`, …); shared colours/geometry in `ui/style.luau`
  (Junkyard: drawing helpers in `ui/craft.luau`). Renders are pixel-identical to before; the type
  check now walks subfolders.
- **Backlog: assets as a manifest** (discussed 2026-11-19, parked: the attack surface is small,
  patches can only load files from their own folder). Idea: `Font.new(id, path)` /
  `Image.new(id, path, { columns, frames })` objects, declarable in any patch file but only while
  the patch loads; the registered set is the manifest; panels take asset objects only (no paths or
  family names), loaders accept registered files only. Patch files stay free-form (one file for a
  minimal patch, `require`d modules for bigger ones); no JSON (no references, no types).
- Patch fonts beyond TTF/OTF: **SVG fonts and BMFont image fonts work for the library title**
  (Clean Gain: SVG font, Junkyard Amp: label-tape BMFont); ControlPanel text in them is next.
  [`research/patch-fonts.md`](research/patch-fonts.md).

### DSP / chain model
- **Multi-output taps**: additional plugin outputs that tap anywhere in the chain (e.g. `tap("di_clean", { output = 2 })` after the punch/comp stage) to get a partly processed DI signal back into the DAW. Needs: aux output buses in the plugin layout (`AudioIOLayout.aux_output_ports`), a tap block in the chain spec, and host routing tests.
- Macro targets directly on block settings (`{ block = "fet", setting = "threshold", ... }`) to cut param boilerplate
- Crossfade on graph swap (no click on patch switch / hot reload)
- Sidechain input (host aux bus): then `Ducker`, keyed compressors, "low band keyed by …"; modulators (LFO/envelope/step sequencer as sources driving params, planned next)
- **Sidechain in + out to/from other tracks** (Flemming, 2026-11-19; mechanism open). In: the
  host's sidechain input bus (VST3/CLAP aux in, the DAW routes it), usable as a key or modulation
  source anywhere in the chain. Out: aux output buses the DAW routes to other tracks (overlaps with
  "multi-output taps"), and/or direct instance-to-instance sharing in-process (same registry as
  passive mode). Check how each host routes plugin sidechains (Reaper, Studio One, Fender Studio).
- Rider: relative target (follow the input level) instead of an absolute dBFS

### Platform / build
- Upstream our patches: runtime param names (nice-plug), X11 frame pacing / poll-timer rate
  (baseview / iced_baseview), timestamp queries (iced_wgpu). Re-vendor on nice-plug updates.
- Push to GitHub (install `gh` or create the repo manually) and run CI; test on Windows (Fender Studio) and macOS (Studio One 6, including Retina @2x performance)
- Hide unused macro slots in the host (needs another nice-plug patch; flags + rescan)
- Licensing: nice-plug exports VST3 through the `vst3` crate (MIT/Apache-2.0), not `vst3-sys`
  (GPLv3): our binaries are no longer forced to be GPLv3. Check the remaining dependencies before
  picking a license.
