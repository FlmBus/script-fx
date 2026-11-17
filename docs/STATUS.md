# ScriptFX: status, decisions & next steps

_Last updated: 2026-11-18. Read this first when picking the project back up. Technical
details, measurements and run instructions are in [`README.md`](../README.md)._

## Vision

"Max for Live with an LLM": an effect plugin where **patches are Lua scripts** that
*declaratively* describe an effect chain built from native blocks, the host-facing macros,
and the patch's own UI. An LLM (prompt / chat inside the plugin) writes and adjusts these
patches, informed by an analysis of the actual audio on the track (and the mix).

## Glossary (used in code, UI and docs)

| Term | Meaning | Code |
|---|---|---|
| **Shell** | the plugin itself and its own UI (frame, header, drawer, status bar) | `crates/plugin/src/shell.rs` |
| **Patch** | a Lua script describing a chain, its params/macros, theme and ControlPanel | crate `patch`, `Patch`, `patches/<name>/patch.lua` |
| **Snapshot** | saved values of a patch (macros + free params). Not implemented yet | — |
| **ControlPanel** | a patch's own UI: SVG + declarative bindings (+ optional `animate`) | Lua key `control_panel`, `ControlPanelSpec`, `ui_svg::SvgControlPanel` |
| **Chain / Block** | declarative DSP description / one building block | `patch::chain`, `BlockSpec` |
| **Graph** | compiled chain running on the audio thread | `dsp::Graph` |
| **Param** | internal value of a patch (unlimited, saved with the project) | `ParamSpec` |
| **Macro** | host-automatable control of a patch, drives 1..n params | `MacroSpec` |
| **Slot** | one of the 16 fixed host parameters macros are mapped onto | `MACRO_SLOTS` |
| **Binding** | ControlPanel element ↔ value | `Binding` |
| **Theme** | a patch's colours (ControlPanel only, never the shell) | `Theme` |
| **Listen / Capture / Analysis** | the action / recorded audio / analysis result | `listen.rs`, `crates/analysis` |
| **Passive mode** | the instance only listens (no patch, passthrough) and shares its audio with other instances. First use: the master; later other channels | not implemented yet |
| **Chat** | conversation with the LLM, captures as attachments | not implemented yet |

Avoid: "preset" (ambiguous between patch and snapshot), "editor"/"chrome" (use shell), "panel" alone (use ControlPanel), "master mode" (use passive mode).

## Decisions made

| Topic | Decision | Why |
|---|---|---|
| Language | **Rust** | readable for a Go dev, compiler catches real-time mistakes (`assert_no_alloc`) |
| Plugin framework | **nih-plug, vendored + patched** (`vendor/nih-plug`) | works; the plugin layer is thin, so swapping it stays cheap. clack + clap-wrapper was the alternative, but lost its advantage once we went VST3-first |
| Formats | **VST3 first** (CLAP built for free) | all 3 machines (Fender Studio/Win, Studio One 6/Mac, Reaper+Carla/Linux) speak VST3. No AU, no AAX |
| Platforms | Linux x86_64, Windows x86_64, macOS universal (arm64 + x86_64) | via GitHub Actions |
| Lua's role | **describes only** (params, macros, chain, theme, UI bindings); never on the audio thread. Only exception: optional `ui.animate()` on the UI thread, sandboxed + instruction budget | real-time safety, LLM-friendly validation |
| Patch model | **params** (internal, unlimited, saved with project) + **macros** (max 16, host-automatable, each drives 1..n params with range + lin/exp curve, Ableton-rack style). A param is free *or* driven by exactly one macro | 8–16 macros are enough when macros can drive many params; hosts can't handle changing parameter lists |
| Host parameters | 16 fixed slots, **names follow the patch** (patched nih-plug: VST3 `kParamTitlesChanged`, CLAP `RESCAN_INFO`). **Verified in Reaper.** Automation stays on the slot across patch changes (same contract as Serum's macros) | plugins can't delete host automation; renaming is safe because hosts key automation by ID |
| DSP | declarative `chain` → `dsp::Graph` compiled off the audio thread, swapped lock-free (rtrb), old graphs freed off-thread | |
| Patch UI | **SVG + declarative bindings** (resvg/usvg, "patch & reparse"), theme → CSS classes | maps very well to the declarative model; secure (no scripts/network; image loading restricted to the patch folder) |
| Plugin's own UI | egui (to be redesigned, see TODO) | |
| Fonts | bundled Noto Sans (+ optional `<patch>/fonts/`) | system font scan took 7.5 s cold |
| Factory patches | compiled into the binary, installed to `<data dir>/ScriptFX/patches`, **updated unless user-modified** (`.factory-manifest`) | |
| Strict patches | unknown fields are errors | catches typos (`treshold`), important for LLM output |
| Listen / analysis | user-triggered capture of the chain's **input and output** (10/20/30 s, silence skipped) → **all** analyses always go to the LLM (~2.5–3k tokens). A summarising LLM pass is a later optimisation | |
| Mix context | only **current track + master** (no buses: buses/sends would skew a track-vs-bus comparison). The master instance runs in **passive mode** | universally applicable, fewer options |
| LLM integration (planned) | **Layer 1**: the user attaches a capture to the prompt. **Layer 2**: the LLM gets tools: `get_analysis(section, detail)` and `request_listen(where)` → closed loop: write a patch, listen to the output, compare, adjust | the LLM can't press play; the human always plays the track |

## Shell layout (decided, not built yet)

- **Own neutral style** (graphite, flat, one blue accent). **Patch themes never colour the shell**,
  so shell controls and patch controls are always distinguishable. The ControlPanel sits in a
  bezel inside the shell. _(The fixed shell style is already applied; the layout below isn't.)_
- **Header**: shell name · patch picker (with a small theme colour mark) · snapshot picker ◀ ▶ ·
  reload · 🎧 Listen · 💬 drawer toggle · ⚙
- **Centre**: the ControlPanel (scaled to fit)
- **Right drawer** (toggled, overlays the ControlPanel; fixed window size) with tabs **Chat**,
  **Controls** (macros + params), **Captures** (all analyses of the session, attachable to messages)
- **Status bar**: chain as a compact block strip (later: clickable for bypass/taps) · in/out meters · CPU · errors
- **Empty state** (fresh instance): "No patch loaded, the signal passes through untouched" +
  actions [Browse patches] [💬 Describe what you need] [🎧 Listen first]
- **Passive mode** view: no patch, no chat; status ("passive · sharing audio"), spectrum, meters,
  connected instances

```
┌───────────────────────────────────────────────────────────────────────────┐
│ ScriptFX │ ▣ Bass Helper ▾ │ snapshot: Init ▾ ◀ ▶ │ ⟳ │ 🎧 Listen │ 💬 │ ⚙ │
├──────────────────────────────────────────────────┬────────────────────────┤
│ ╔══════════════════════════════════════════════╗ │ Chat │ Controls │ Captures│
│ ║            ControlPanel (SVG)                ║ │ you: make it punchier  │
│ ║          patch's own colours/style           ║ │  📎 capture 10 s       │
│ ╚══════════════════════════════════════════════╝ │ [type a message…    ⏎] │
├──────────────────────────────────────────────────┴────────────────────────┤
│ chain: PUNCH › FET › OPTO › SPLIT › EQ › OUT  │ in ▮▮▮▯ out ▮▮▯ │ 0.7% CPU │
└───────────────────────────────────────────────────────────────────────────┘
```

## Patch UI: reactive Lua components (decided 2026-11-18, prototype built)

After several mockup rounds (canvas: "ScriptFX Shell Mockup", v1–v7):

- **Shell layout (v6/v7):** plain text library on the left (`▶ Patch` / indented snapshots,
  sections styled with the patch's background art + logo), the patch's ControlPanel fills the
  stage, a **two-row shell pill** at the top centre of every ControlPanel (row 1: snapshot ▾,
  save, load · row 2: reload, chat, settings), thin signal line below. Listen lives in the chat.
  Chat is per patch (drawer over the panel). Passive mode is offered on the "No patch loaded" screen.
  Shell style: native-looking (system font, dense, bevels, etched lines), not web-like.
- **Patch UI = reactive Lua components** (`crates/ui-components`): every visual part (background,
  logo, knobs, meters, displays, tabs) is `component { kind?, size?, animated?, on_click?, render(p) }`;
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
  strokes); keep filter regions tight (`svg.blur(std, pad)`, `svg.arc{glow}` does this); prefer a
  static background + small animated components over an animated background.

**Open / next:** first-open cost at 2x (150 ms) → persistent raster cache or background-thread
rendering; bool/enum params for toggles/selectors (toggles are local state only so far); `p.series`
data (spectrum) from the analysis engine; an LLM writing a component from the API doc alone
(not tested yet); type stubs for the component API; building the new shell layout itself.

## What exists (commits on `main`)

- `crates/patch`: model + sandboxed Lua loader + chain spec + validation
- `crates/dsp`: graph engine; blocks `gain eq drive comp(fet|opto|clean) rider transient split(LR4)`, block meters, no-alloc test, bench (`--example bench`)
- `crates/ui-svg`: SVG panel renderer + bindings (`rotate translate scale opacity fill text drag`)
- `crates/analysis`: loudness (EBU R128, verified against ffmpeg), 1/3-oct spectrum, tilt, centroid, energy shares, resonances, dynamics (percentiles, per-band spread, onsets, attack), stereo, pitch (YIN)
- `crates/plugin` (package `scriptfx`): egui chrome (patch picker, hot reload, macro/param sidebar, prompt field without LLM yet, 🎧 Listen + result screen + prompt chip), host-name updates, lock-free capture
- patches: `tape-glow`, `clean-gain`, `bass-helper` (punch → FET → opto → split: ride/grit → EQ; 7 macros)
- `vendor/baseview`: patched to not panic on GLX errors (fixed blank editor after close/reopen in Reaper)
- CI workflow written, **not run yet** (repo isn't on GitHub; `gh` isn't installed)

## Key findings (details in README)

- Performance: bass-helper (11 blocks) = **0.7 % of one core**; chain complexity is not a concern.
- SVG UI: full redraw on the CPU costs ~5 ms @1x / ~13 ms @2x for a 640×360 panel. Needs static-layer caching or a GPU renderer (vello).
- Lua `animate()` per frame: ~25 µs, so scripted UI animation is cheap.
- Linux: NVIDIA + XWayland GLX issues (standalone tiling crash; fixed for close/reopen via patched baseview). JACK standalone on PipeWire doesn't process; use `-b alsa`.
- egui `Slider::from_get_set` writes every frame; it needs a change check (otherwise it spams host automation).
- Chain model gaps: param boilerplate (16 params for 7 macros), no parallel/sidechain/modulators/lookahead, no oversampling, click on graph swap.
- Analysis caveats: "resonances" include notes/harmonics of tonal sources; onset detection misses soft re-attacks at equal level; the rider uses an absolute target level.

## TODO (prioritised)

### Next up
1. **Passive mode** (master instance): toggle; bypass the chain, keep a rolling ~30 s buffer; a track instance's listen pulls the master audio for the same time window (aligned via host transport position) and adds "track vs. mix" analysis (masking per band). In-process registry first (Reaper/Studio One/Fender Studio run plugins in-process); sandboxed hosts (Bitwig, Reaper "dedicated process") would need IPC. Warn if two instances claim master.
2. **Empty default state (Kontakt-style)**: a fresh instance loads **no patch**, has an **empty chain** (passthrough, does nothing to the sound) and an empty/neutral UI. Currently it auto-loads `tape-glow`.
3. **LLM integration**: prompt → patch generation (layer 1 with attached capture), then layer-2 tools (`get_analysis`, `request_listen`). Bring-your-own API key. Validate generated patches with the strict loader; feed errors back to the LLM.

### UI redesign (plugin chrome)
- **Chat view instead of a single prompt field**: most requests need clarification or follow-up adjustments. This also cleans up the general UI, and captures/analyses stay available over the course of a conversation (shown as message attachments).
- **Open question: persist chats?** Options: (a) not at all, (b) in the plugin state (travels with the DAW project; watch the state size), (c) in the data dir next to the patch (e.g. `<patch>/chat.json`, survives across projects), (d) both. Also: should a generated patch carry its chat history?
- Patch browser, macro/param editing, listen screen and analysis view integrated into the redesign.

### DSP / chain model
- **Multi-output taps**: additional plugin outputs that tap anywhere in the chain (e.g. `tap("di_clean", { output = 2 })` after the punch/comp stage) to get a partly processed DI signal back into the DAW. Needs: aux output buses in the plugin layout (nih-plug `AudioIOLayout.aux_output_ports`), a tap block in the chain spec, and host routing tests.
- Macro targets directly on block settings (`{ block = "fet", setting = "threshold", ... }`) to cut param boilerplate
- Crossfade on graph swap (no click on patch switch / hot reload)
- Parallel chains / sub-chain dry-wet, sidechain (e.g. low band keyed by …), modulators (LFO/envelope as sources), lookahead, oversampling for drive
- Rider: relative target (follow the input level) instead of an absolute dBFS

### Platform / build
- Push to GitHub (install `gh` or create the repo manually) and run CI; test on Windows (Fender Studio) and macOS (Studio One 6, including Retina @2x performance)
- Hide unused macro slots in the host (needs another nih-plug patch; flags + rescan)
- NVIDIA/GLX: check whether the standalone tiling crash is fixed by the baseview patch; consider a non-GL renderer for the chrome
- SVG performance: static-layer caching (`resvg::render_node`) or vello; re-measure @2x
- Licensing: VST3 export goes through `vst3-sys` (GPLv3), so binaries are GPLv3 unless that dependency is replaced
