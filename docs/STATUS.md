# ScriptFX: status, decisions & next steps

_Last updated: 2026-11-21. Read this first when picking the project back up. How to build, run and
write patches: [`README.md`](../README.md). What each session built: [`HISTORY.md`](HISTORY.md).
Concepts, research and measurements: [`research/`](research/)._

## Current state

The chain editor is on `main`: Surface and Chain views, block pages, the macro rail, the
modulators (Follow, LFO 1–3, Step) with their drawer panels, Serum-style routes with drag & drop
onto knobs, and the Matrix (every route, add/remove). See
[`research/chain-editor.md`](research/chain-editor.md) for the design and remaining deviations.
Tested off-screen in the standalone (unit tests + Xvfb walkthrough), not yet in a real host.

- **Surface** shows the patch's ControlPanel, or a themed macro surface when it has none.
  **Chain** shows the block tree; block pages have knob cells and DSP-based previews for some blocks.
- **Modulation**: two kinds of route, macro slots (`Patch::modulations`) and modulator
  assignments (`Patch::assignments`), one model for the shell (`patch::routes`: `Route`,
  `RouteSource`, add/remove). Normalized deltas sum onto the knob's base. Drag a macro's grip (the
  "M3" tag in the rail) or a modulator tab's grip (drawer) onto a block page knob, or a modulator
  onto a macro in the rail; each route is a meta knob in its source's colour (up to 5, then "+N");
  right-click = menu with Remove. Changing routes or modulator settings rebuilds the graph once
  per frame and the new graph **adopts** the playing one's state (tails, LFO phases:
  `Graph::adopt`), so it doesn't click. Saved with the project: slot settings (`mod-slots`) and
  the user's route/modulator edits against the patch file (`mod-edits`, `patch::ModEdits`,
  replayed on every load incl. hot reload). Not yet round-tripped in a host.
- **Modulator displays** are drawn from the settings with a marker at the audio thread's
  position (`Graph::modulator_phases`), like Serum: one LFO cycle (`dsp::modulators::morph`, what
  plays) with a dot, Step's bars with the playing step lit. Only Follow is a live trace. A trace
  sampled once per UI frame aliases any LFO above a few Hz into noise (2026-11-21).
- **Layers:** a page press closes the library, sheets, the drawer, the entry and menus; the macro
  rail stays (drag a macro onto a knob, then turn it). The drawer opens to 180 px and block pages
  lift their knobs above it. The chain ↔ page zoom blur keeps the notch, the drawer (at its
  current height) and the rail sharp (`postfx` has three keep rects).
- **Rates:** LFO and Step have `Hz | sync` (pills on the Rate knob; `sync = "1/8"` in Luau, a note
  length overriding `rate_hz`). Block rates (`sync` on Chorus, Delay, …) are still patch-only.
- **Filters:** LowPass/HighPass have a `slope` param (12/24/36/48 dB/oct, cascaded biquads with
  Butterworth stage Q's; `q` = resonance on the steepest stage). Params can be stepped
  (`ParamSpec::step`: values snap). The Matrix shows each route's source level and the
  destination's live value.
- **Pointer locking** on X11 was tested; the Windows/macOS implementations have not been
  compiled or tested.
- **Diagnostics build** (`--features diagnostics`, `crates/plugin/src/diag.rs`): a log file per
  process (macOS `~/Library/Logs/ScriptFX/`) with a system report, GPU/audio setup, patch errors,
  panics with backtraces and a 10 s heartbeat (audio callback timing vs. budget, frame timing).
  **CD** (`build.yml`): every push triggers tests and, if they pass, builds for Linux, Windows and
  macOS universal with symbols,
  the standalone, helper scripts (`.github/package/<platform>/`: install, run, collect a report
  zip) and `test-patches/`; `main` → rolling pre-release "nightly", tags `v*` → releases. First
  test on the studio's M4 Mac mini: 2026-11-21.
- **Dev builds:** dependencies are optimized in the dev profile (`[profile.dev.package."*"]`);
  unoptimized, a debug standalone rendered Tape Glow below 1 fps and looked frozen (2026-11-21).
  Ctrl+C in the standalone only sets a flag the GUI checks per frame; close the window instead.
- **Open details:** macro value text and typed values use the target's default base and only that
  macro's contribution, not the edited base or other macros; beyond five routes a knob shows "+N"
  (edit those in the Matrix); the block page's display slides under the notch while the drawer is
  open; test save/restore in a host and pointer warping under XWayland
  on Hyprland.

## Next steps (prioritised)

1. **Modulation, step 4 (polish):** Matrix filtering (by source/destination) and sorting; the
   open modulator's targets glow and chain blocks show dots in their modulators' colours (concept
   §§ drawer/chain); `Hz | sync` pills on block knobs (rates, delay times); a
   route's meta knob focused right after a drop; modulators onto Surface macros. Then a host
   round trip (save project, reopen) of routes and modulator edits.
2. **Surface naming:** the macro fallback already appears for patches without a panel. Rename
   panels to surfaces throughout the API (`ui.Panel`), code and docs (decided, Flemming).
3. **Block pages:** displays for the remaining blocks; more input types (switches, choices).
4. **Blocks for professional chains** ([`research/blocks.md`](research/blocks.md) §4, P1 first):
   sidechain (an internal detector chain on dynamics blocks, then an external key input), one
   versatile compressor (detector, topology, ratio curve, program-dependent timing, lookahead,
   link, saturation; FET/opto/vari-mu as presets), lookahead on dynamics, linear-phase EQ.
5. **Hands-on check in Reaper and Carla**: everything above plus knob drag/double-click on all
   panels, chat typing (Enter/Shift+Enter, selection, clipboard, dead keys), Listen, UI scale
   (live and reopening at ≠ 100 %, host accepts the resize), library wheel scrolling, controls
   sheet (automation gestures, names), hot reload + error card, unload, close/reopen ×10, two
   instances, HiDPI.
6. **Snapshots** (saved values of a patch): the notch selector/save/load and the library rows are
   placeholders ("Init" = defaults) until then.
7. **Passive mode**: bypass, keep a rolling ~30 s buffer; a track instance's listen pulls the
   passive instance's audio for the same time window (host transport position) and adds "track vs.
   mix" analysis. In-process registry first (Reaper/Studio One/Fender Studio run plugins
   in-process); sandboxed hosts need IPC. Channel names: CLAP track-info / VST3 IInfoListener
   (nice-plug has `track_info_updated`), fallback: user-named instances.
8. **LLM integration**: chat → patch generation (layer 1 with an attached capture), then layer-2
   tools (`get_analysis`, `request_listen`). Validate generated patches with the strict loader and
   `luau-lsp analyze`; feed errors back. Adapter decision (2026-11-19): own `Backend`/tool layer
   in a new `crates/llm`, in-process HTTP or an installed CLI (`claude -p`, `codex exec`), ACP
   later; research, candidates, the Claude subscription/ToS caveat:
   [`research/llm-adapter.md`](research/llm-adapter.md). Still open: which cheap/free models.

## Vision

"Max for Live with an LLM": an effect plugin where **patches are Lua scripts** that
*declaratively* describe an effect chain built from native blocks, the host-facing macros,
and the patch's own UI. An LLM (prompt / chat inside the plugin) writes and adjusts these
patches, informed by an analysis of the actual audio on the track (and the mix).

## Glossary (used in code, UI and docs)

| Term | Meaning | Code |
|---|---|---|
| **Shell** | the plugin itself and its own UI (top bar, notch, views, sheets, drawer) | `crates/plugin/src/shell/` |
| **Patch** | a Luau script describing a chain, its macros, theme and ControlPanel (`return Patch.new(...)`) | crate `patch`, `patches/<name>/init.luau` |
| **Snapshot** | saved values of a patch (macros + params). Not implemented yet | — |
| **ControlPanel** | a patch's own UI: reactive Luau components, fixed 840×560; the `panel` of `Patch.new`. To be renamed "surface" | `ui_components::Ui`, `shell/stage.rs` |
| **Surface** | the first view: the ControlPanel, or the macro surface (the macros as knobs) | `shell/surface.rs` |
| **Component** | one visual part of a ControlPanel; each instance is a cached layer | `Component.define`, `X.new {}` |
| **Chain / Block** | DSP description / one **building block**: the unit patches build professional effect chains from, so it exposes every parameter it can ([`research/blocks.md`](research/blocks.md)) (`Chain.new`, `Blocks.<Type>.new(id, {...})`) | `patch::chain`, `BlockSpec` |
| **Chain view / Block page** | the view of the chain's tree / one block's page (display + knobs) | `shell/chain_view.rs`, `shell/block_page.rs` |
| **Graph** | compiled chain running on the audio thread | `dsp::Graph` |
| **Param** | a block's knob; has a base value (saved with the project) | `ParamSpec` |
| **Macro** | host-automatable control of a patch, drives 1..n params | `MacroSpec` |
| **Host slot** | one of the 16 fixed host parameters macros are mapped onto | `MACRO_SLOTS` |
| **Modulation slot / route** | one source → param (or modulator → macro) link: amount (range size, sign = direction), uni/bi, curve, bypass | `patch::Modulation`, `ModAssignment`, `patch::Route`, `ModSettings` |
| **Meta knob / range arc / live dot** | a slot's mini knob on its target / the slot's range on the knob / the value after modulation | `shell/knob.rs` |
| **Modulator** | Follow, LFO 1–3, Step: sources that live outside the chain | `dsp::modulators`, `shell/drawer.rs` |
| **Drawer / Macro rail / Notch / Sheet** | modulator tabs at the bottom / the macros' card on the left / view switch + snapshots at the top / side panels (library, controls, chat, settings) | `shell/drawer.rs`, `macro_rail.rs`, `notch.rs`, `sheet.rs` |
| **Matrix** | every route as a table (live values, add/remove) | `shell/matrix.rs` |
| **Bind / Source** | what a component reads: `bind` = a Macro/Param object (value, draggable), `source` = a metered block or `Meters.IN/OUT` | `__ref`, `patch::Source` |
| **Theme** | a patch's design tokens (colours, fonts, images); used by its ControlPanel, library card and macro surface, never by the rest of the shell | `Theme` |
| **Listen / Capture / Analysis** | the action / recorded audio / analysis result | `listen.rs`, `crates/analysis` |
| **Passive mode** | the instance only listens (no patch, passthrough) and shares its audio with other instances. Not built | — |
| **Chat** | planned global conversation with the LLM, captures as attachments; today one in-memory chat per shell, no LLM | `shell/chat.rs` |

Avoid: "preset" (ambiguous between patch and snapshot), "editor"/"chrome" (use shell), "panel" alone
(use ControlPanel, later surface), "slot" alone (host slot, modulation slot, or a container's slot such as a band split's bands), "master mode" (use
passive mode).

## Decisions

| Topic | Decision | Why |
|---|---|---|
| Language | **Rust** | readable for a Go dev, compiler catches real-time mistakes (`assert_no_alloc`) |
| Plugin framework | **nice-plug** (community continuation of nih-plug), vendored + patched (`vendor/nice-plug`, `vendor/nice-plug-core`) | nih-plug is in maintenance mode; nice-plug maintains the iced adapter. Before: nih-plug (vendored); clack + clap-wrapper lost its advantage once we went VST3-first |
| Formats | **VST3 first** (CLAP built for free) | all 3 machines (Fender Studio/Win, Studio One 6/Mac, Reaper+Carla/Linux) speak VST3. No AU, no AAX |
| Platforms | Linux x86_64, Windows x86_64, macOS universal (arm64 + x86_64) | via GitHub Actions |
| Blocks | **Building blocks for professional chains**: expose every parameter a device or engineer would set; continuous ones are Params; styles are presets of one versatile block where possible (Flemming, 2026-11-21) | patches (and the LLM) can only build what the blocks allow; see [`research/blocks.md`](research/blocks.md) |
| Modulation amount | **One control**: the amount's sign is the direction (no separate invert); it is the size of the range for both polarities (bipolar swings ±amount/2) (Flemming, 2026-11-21) | one knob says everything; switching uni/bi doesn't change how far the knob moves |
| Lua's role | **describes only** (params, macros, chain, theme) and renders the UI components (UI thread, sandboxed + instruction budget); never on the audio thread | real-time safety, LLM-friendly validation |
| Patch model | **params** = the blocks' knobs (unlimited, base value saved with the project) + **macros** (max 16, host-automatable, each drives 1..n params) | 8–16 macros are enough when macros can drive many params; hosts can't handle changing parameter lists |
| Modulation | **Serum's model** (2026-11-20, source of truth: [`research/serum-modulation.md`](research/serum-modulation.md)): every source → param link is a slot (amount −1…+1 of the knob's travel, uni/bi, curve, bypass; no invert, see "Modulation amount"); deltas sum on the knob's base in normalized space, clamped, then the param's scale; any number of slots per knob. Sources: macros and the modulators (Follow, LFO 1–3, Step). Slot edits saved per DAW project | the earlier unit-based from/to mappings didn't add up across lin/log knobs; Serum's model is proven and intuitive |
| Chain editor | a **tree** with container slots (not a free graph); views: Surface, Chain (+ block pages), Matrix; concept: [`research/chain-editor.md`](research/chain-editor.md) | cable salad at mid-size patches in the free-graph mockup |
| Host parameters | 16 fixed host slots, **names follow the patch** (patched nice-plug: VST3 `kParamTitlesChanged`, CLAP `RESCAN_INFO`). **Verified in Reaper.** Automation stays on the slot across patch changes (same contract as Serum's macros) | plugins can't delete host automation; renaming is safe because hosts key automation by ID |
| DSP | declarative `chain` → `dsp::Graph` compiled off the audio thread, swapped lock-free (rtrb), old graphs freed off-thread | |
| Patch UI | **reactive Luau components** → SVG → one cached layer each (resvg), fixed 840×560. Parked on 2026-11-18 for the raster plan, then revised and back: **still the plan**; raster layers / sprite sheets / shaders stay open ideas ([`research/raster-ui-plan.md`](research/raster-ui-plan.md)) | declarative, cached, sandboxed; the earlier SVG + bindings panel was removed |
| Block tags / categories | Doc comments in the definitions file (`--- <Category> · …`), checked by a test | LSP hover/completion and LLM search from one place; no runtime field until something needs it |
| Oversampling | Per block via one generic wrapper (`oversampling` on every block, auto for nonlinear ones); linear-phase FIR + latency compensation (PDC) | only blocks that need it pay; linear phase + branch delays keep parallel blends from comb filtering; PDC is needed anyway for lookahead/pitch blocks |
| Script language | **Luau** (mlua `luau` feature) with luau-lsp type checking (strict) | relative `require` (per-patch UI files, no name collisions), typed props across files, built-in sandbox; LuaLS couldn't resolve per-folder requires or infer prop types |
| Lua API style | builder objects; identity positional, settings in an options table; namespaced; enums as strings; separate block types instead of a `type`/`style` option | readable, one way to do things, typo-proof for LLMs |
| Plugin's own UI (shell) | **iced** (`nice-plug-iced`, wgpu), one module per section | clean, sectioned code; wgpu (no GL) |
| Fonts | bundled Noto Sans (+ optional `<patch>/fonts/`) | system font scan took 7.5 s cold |
| Factory patches | compiled into the binary, installed to `<data dir>/ScriptFX/patches`, **updated unless user-modified** (`.factory-manifest`) | |
| Strict patches | unknown fields are errors | catches typos (`treshold`), important for LLM output |
| Listen / analysis | user-triggered capture of the chain's **input and output** (10/20/30 s, silence skipped); planned LLM integration sends **all** analyses (~2.5–3k tokens). A summarising LLM pass is a later optimisation | |
| Mix context | only **current track + master** (no buses: buses/sends would skew a track-vs-bus comparison). The master instance runs in **passive mode** | universally applicable, fewer options |
| Chat model (planned) | **Global chats**, stored in the user data dir (with model auth/config); one default chat, more allowed (decided 2026-11-19). Today there is one in-memory chat per shell | per-patch chats would have to be saved per patch; a chat can span several patches |
| Patch kinds | **Graph patches** get a visual graph editor: a new, data-only chain definition next to the coded Luau patches (decided 2026-11-19; design open, see "Open directions") | Luau code can't be edited visually (loops/helpers don't round-trip) |
| LLM integration (planned) | **Layer 1**: the user attaches a capture to the prompt. **Layer 2**: the LLM gets tools: `get_analysis(section, detail)` and `request_listen(where)` → closed loop: write a patch, listen to the output, compare, adjust | the LLM can't press play; the human always plays the track |

## Open directions

### Patch code and graph patches (partly decided)
Research (precedents, Kontakt, WebAssembly/WebGL, threat model, spikes, decision matrix):
[`research/patch-code-and-sharing.md`](research/patch-code-and-sharing.md). Short version:
ReaScripts and Max for Live share fully unsafe code; Kontakt (the closest product, with Komplete UI's
reactive components and Canvas paint callbacks) and Roblox share code safely through the API boundary.
Recommendation: grow visuals through safe primitives (canvas/paths, filmstrips, maybe shaders); put
anything unsafe behind an opt-in "trusted patch" capability.

**Decided (Flemming, 2026-11-19): there will be a graph editor for patches**, implemented as its own
kind of chain definition (data, no logic), next to the coded Luau patches. **Concept for the chain
editor, the drawer, modulation (Serum style, fixed set Follow / LFO 1–3 / Step), macros, the Matrix
and the knob cell: [`research/chain-editor.md`](research/chain-editor.md)** (tree with container
slots, not a free graph). Still open from the list below: the file format, panels for graph patches,
what the LLM writes:
- format and where it lives (a Luau table of data, or JSON; a file in the patch folder);
- how the two kinds relate: can a graph patch have a coded ControlPanel, or does it always get a
  generic one? Can a Luau patch be converted to a graph (only when it has no logic)?
- macros/params in the graph (nodes, or a side panel);
- what the LLM writes for which kind; the graph is easier to validate and diff;
- editing the chain (block catalogue, adding/removing and routing blocks); the current Chain view
  and block pages display the Luau result and edit parameter values, not the chain's source.

The graph data must be the source of truth for visual editing: a `BlockSpec` tree is only the result
of running Luau and cannot round-trip edits through loops, helpers and computed values.

Also open: whether to support sharing patches, and a **locked-down mode** for foreign patches that
loads only safe data and shows a generic UI. See the research for the threat model.

### Effects off switch + GPU safety (part of the raster/shader plan)
- **Shell setting "Effects off":** disables shaders, effects and animations for all patches.
  Fallback: layers without their effects, static frames, generated layers as a still image or plain
  colour. Persisted per user (not per project).
- **Automatic fallback per patch:** measure GPU time per frame; if a patch exceeds its budget, or a
  pass doesn't finish (fence timeout, e.g. a runaway shader), switch that patch to "effects off" and
  show why, instead of retrying every frame.
- **Validation:** reject shader loops without a constant bound (or inject an iteration cap); cap
  passes per patch and the render scale.
- Background: [`raster-ui-plan.md`](research/raster-ui-plan.md) (stress test, runaway shader test).

## Backlog by area

### Shell / UI
- **Chain editor: more input types for block/modulator params** (Flemming, 2026-11-19). The
  chain-editor mockup (claude.ai artifact "ScriptFX Chain Editor Mockup") uses knob cells only; a
  knob cell may carry a pill group at its top for the knob's mode (e.g. `Hz | sync` on rate,
  `ms | sync` on time). Still to design: block/modulator switches (bypass, ping-pong), choices
  (filter type, LFO shape), maybe steppers.
- **Global chats** (decided 2026-11-19): share chats between instances and persist them in the user
  data dir, with one default and optional additional chats. Today each shell has one in-memory
  "Main" chat. Store model auth/config as planned in `research/llm-adapter.md` (keys in the OS
  keychain). Open: should a generated patch link back to the chat that made it?
- Shell follow-ups: letter-spacing (IN/OUT, tracked titles) isn't supported by iced text;
  accessibility labels for icon buttons (no iced equivalent yet); full font files (the bundled
  ones are Latin subsets, "→" falls back); per-node clip LEDs (no per-block peaks); library card
  background art (cards use a gradient from the theme colours).
- Components, open/next: first-open cost at 2x (150 ms) → persistent raster cache or
  background-thread rendering; bool/enum params for toggles/selectors (toggles are local UI state
  only so far); `p.series` data (spectrum) from the analysis engine; test an LLM writing a patch +
  UI from the type stubs alone; hit areas are rectangles; `measure_text()` helper if layouts need
  text widths in patch components.

### Patch isolation (assets and state)
Principle (Flemming, 2026-11-19): patches are isolated from the shell and from each other.
- **Isolated:** Lua runtime per patch (sandbox, memory cap, instruction budget); `require`, panel
  images and `theme` paths confined to the patch folder; panel SVG text uses a per-patch font
  database (bundled + the patch's `fonts/`); patch fonts in the shell get private family names
  (patch folder + content hash), so they can't restyle the shell or each other (fixed 2026-11-19).
- **Not yet:** chat logs and attached captures are in memory per plugin instance; global chats still
  need sharing and persistence. Captures use one shared folder and second-resolution names, so
  concurrent captures can collide. Param values and slot settings are dropped on patch switch;
  per-patch state is needed once snapshots exist.
- **Backlog: assets as a manifest** (discussed 2026-11-19, parked: the attack surface is small,
  patches can only load files from their own folder). Idea: `Font.new(id, path)` /
  `Image.new(id, path, { columns, frames })` objects, declarable in any patch file but only while
  the patch loads; the registered set is the manifest; panels take asset objects only (no paths or
  family names), loaders accept registered files only. Patch files stay free-form (one file for a
  minimal patch, `require`d modules for bigger ones); no JSON (no references, no types).
- Patch fonts beyond TTF/OTF: SVG fonts and BMFont image fonts work for the library title (and
  the macro surface's title); ControlPanel text in them is next
  ([`research/patch-fonts.md`](research/patch-fonts.md)).

### DSP / chain
- **Editable LFO shape** (Flemming, 2026-11-21: later). Replace the fixed shapes with a drawable
  curve, as in Serum: points (x 0..1, y −1..1) with a curve per segment, at most ~32 (fixed array,
  allocation-free evaluation); sine/tri/saw/square/ramps as presets that fill the points; the
  drawer edits it (drag points, double-click add/remove, drag a segment to bend it, grid snap);
  Luau `shape = "saw"` or a point list. Random / sample & hold can't be a fixed shape: a separate
  mode if wanted. The display already draws the shape with a playback marker.
- **Envelope modulator (parked, Flemming 2026-11-19: low priority; removed from the fixed set
  Follow / LFO 1–3 / Step, see `research/chain-editor.md`).** An ADSR needs a *gate* (on + off), not just a
  trigger. Without MIDI: trigger from a realtime transient detector (fast vs. slow follower ratio +
  hold-off; the `Transient` block already detects attacks), gate either by **level** (stays open
  above a close threshold, hysteresis; new transients retrigger) or by **time** (fixed/synced
  length). Knobs then: attack, decay, sustain, release, sensitivity, gate (`level | time`); curve
  edited in the display. MIDI notes as a later trigger source (brings note on/off for free).
- **Multi-output taps**: additional plugin outputs that tap anywhere in the chain (e.g.
  `tap("di_clean", { output = 2 })` after the punch/comp stage) to get a partly processed DI signal
  back into the DAW. Needs: aux output buses in the plugin layout (`AudioIOLayout.aux_output_ports`),
  a tap block in the chain spec, and host routing tests.
- Crossfade on graph swap (no click on patch switch / hot reload)
- Sidechain input (host aux bus): then `Ducker`, keyed compressors, "low band keyed by …".
- **Sidechain in + out to/from other tracks** (Flemming, 2026-11-19; mechanism open). In: the
  host's sidechain input bus (VST3/CLAP aux in, the DAW routes it), usable as a key or modulation
  source anywhere in the chain. Out: aux output buses the DAW routes to other tracks (overlaps with
  "multi-output taps"), and/or direct instance-to-instance sharing in-process (same registry as
  passive mode). Check how each host routes plugin sidechains (Reaper, Studio One, Fender Studio).
- Rider: relative target (follow the input level) instead of an absolute dBFS

### Idea: analysis / debug block
A block that shows a general-purpose analysis (level, spectrum, stereo, dynamics) of the audio going
into it, anywhere in the chain. The Listen feature could tap into the chain through these blocks,
not only at the plugin's input/output.

### Platform / build
- Upstream our patches: runtime param names (nice-plug), X11 frame pacing / poll-timer rate / app
  resize (baseview / iced_baseview), timestamp queries and the post-process pass (iced_wgpu). Re-vendor on nice-plug updates.
- Verify the GitHub Actions builds and release packages; test on Windows (Fender Studio) and macOS
  (Studio One 6, including Retina @2x performance).
- Compile-check (and try) Windows and macOS: the pointer lock's FFI (`shell/pointer_lock.rs`) has
  never been built there.
- Hide unused host slots in the host (needs another nice-plug patch; flags + rescan)
- Licensing: nice-plug exports VST3 through the `vst3` crate (MIT/Apache-2.0), not `vst3-sys`
  (GPLv3): our binaries are no longer forced to be GPLv3. Check the remaining dependencies before
  picking a license.
