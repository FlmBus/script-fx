# ScriptFX: status, decisions & next steps

_Last updated: 2026-11-25. Read this first when picking the project back up. How to build, run and
write patches: [`README.md`](../README.md). What each session built: [`HISTORY.md`](HISTORY.md).
Concepts, research and measurements: [`research/`](research/)._

## Current state

The chain editor is on `main`: the Surface and Chain surfaces, block modals, the modulators
(Follow, LFO 1–3, Step), Serum-style routes with drag & drop onto knobs, and the Matrix (every
route, add/remove). Branch `mod-dock` (2026-11-25): the **mod dock** replaces the macro rail and
the drawer ([`research/shell-layout.md`](research/shell-layout.md)). See
[`research/chain-editor.md`](research/chain-editor.md) for the design and remaining deviations.
Tested off-screen in the standalone (unit tests + Xvfb walkthrough), not yet in a real host.

- **Surface** shows the patch's own UI, or the themed default macro surface when it declares none.
  **Chain** shows the block tree; clicking a block (or a container's header bar, both light up on
  hover) opens its **modal** (2026-11-23, mockup `research/mockups/block-modal.html`): the camera
  flies into the block, zooming by the size ratio of block and modal (~8× for a block, ≥ 1.4× for
  a container; scale runs geometrically, 280 ms ease-in-out, `shell/motion.rs`), the block grows into
  the fixed 626 × 266 modal and its label into the title; zoom blur while moving, a soft blur
  (`postfx` `soft`) behind the open modal. ‹ › beside the modal and ← → walk the chain (the camera
  pans; order = processing order, so a band split's low band comes first). ✕, Escape or a click
  beside it zoom back out. Knob cells and DSP-based displays for some blocks; the display takes the
  width the knobs leave, more than eight knobs scroll.
- **Modulation**: two kinds of route, macro slots (`Patch::modulations`) and modulator
  assignments (`Patch::assignments`), one model for the shell (`patch::routes`: `Route`,
  `RouteSource`, add/remove). Normalized deltas sum onto the knob's base. Drag a macro's grip (the
  title bar in the dock) or a modulator's title bar onto a block knob, or a modulator onto a
  macro in the dock; each route is a meta knob in its source's colour (up to 5, then "+N");
  right-click = menu with Remove. Changing routes or modulator settings rebuilds the graph once
  per frame and the new graph **adopts** the playing one's state (tails, LFO phases:
  `Graph::adopt`), so it doesn't click. Saved with the project: slot settings (`mod-slots`) and
  the user's route/modulator edits against the patch file (`mod-edits`, `patch::ModEdits`,
  replayed on every load incl. hot reload). Not yet round-tripped in a host.
- **Modulator displays** are drawn from the settings with a marker at the audio thread's
  position (`Graph::modulator_phases`), like Serum: one LFO cycle (`dsp::modulators::morph`, what
  plays) with a dot, Step's bars with the playing step lit. Only Follow is a live trace. A trace
  sampled once per UI frame aliases any LFO above a few Hz into noise (2026-11-21).
- **Mod dock** (branch `mod-dock`, `shell/dock.rs`): always below the stage, never over it.
  Macros 1–8 / 9–16 and the five modulators (live mini displays), each with a title bar in its
  colour = the drag handle. Click = focus (the hint counts its routes, the others fade), again =
  its editor: a panel rising from the dock over the stage's lower part (decided 2026-11-25 over
  scaling the stage, a modal, or growing the window: the stage stays usable, a block modal moves
  up above it). It stays open while the stage is used and steps aside while a source is dragged.
  Escape closes the editor, then the focus. Macros (dock and Surface) take modulators and show a
  meta knob per route. Window 836 × 714 (stage still 816 × 544). Not yet: the knobs' focus rule,
  collapsing, the mockup's drag tag.
- **Blur** (branch `mod-dock`): the zoom blur applies to the chain under the camera only, drawn
  as the renderer's backdrop (`shell/backdrop.rs`); everything drawn after it stays sharp.
- **Notch** (branch `mod-dock`): one row, reload | snapshots | **bypass** (the host's bypass
  parameter, `FxParams::bypass`: the input passes through delayed by the chain's latency, 10 ms
  crossfade; the graph keeps running). The view switch is three icons in the stage's top-right
  corner (name on hover).
- **Snapshots** (2026-11-25, `snapshots.rs`, `shell/snapshot_menu.rs`): a snapshot is the whole
  sound of a patch, everything the project saves for it: macros (by id), every param's value, the
  meta knobs that differ from the patch, the routing edits. Stored per user, apart from the patch
  code: `<data dir>/ScriptFX/snapshots/<patch folder>/<name>.json` (`SCRIPTFX_SNAPSHOTS`
  overrides; atomic writes). "Init" = the patch's defaults, never a file. Values are keyed by id,
  so a snapshot survives a patch growing or shrinking (missing → default). The notch: ‹ › step
  through Init and the saved ones (recall replaces the sound), the selector shows the name with a
  dot while the sound differs from it (compared 5×/s with float tolerance) and opens the menu
  (recall, Save as new…, Rename…, Revert, Export…, Delete… with a confirmation), Save overwrites
  the selected one or asks for a name on Init, Load imports a file (one saved from another patch
  is refused). The library lists Init + the saved ones per patch; a row loads the patch with it.
  The selection is saved with the project (`FxParams::snapshot`); a snapshot file that's gone
  falls back to Init. Import/export use native dialogs (`rfd` 0.17, XDG portal on Linux, no GTK;
  0.18 needs Rust 1.92) on their own thread, modal on macOS (AppKit's main thread). Checked on
  Xvfb: modified dot, save on Init (naming), invalid name refused, step, recall from the menu and
  from the library across patches, rename, delete. **Not checked:** the dialogs themselves (no
  portal on Xvfb; without one, Load silently does nothing: `rfd` reports it as a cancel), Windows,
  macOS, a host's project save/restore of the selection.
- **Layers:** a press on the surface closes the library, sheets, the entry and menus; the dock
  stays. The chain ↔ modal blur keeps the notch, the sheets and the modal sharp (through the
  stage's scale).
- **Rates:** LFO and Step have `Hz | sync` (pills on the Rate knob; `sync = "1/8"` in Luau, a note
  length overriding `rate_hz`). Block rates (`sync` on Chorus, Delay, …) are still patch-only.
- **Blocks (2026-11-21):** one versatile compressor (detector, topology, ratio curve, program,
  link, saturation, lookahead; Compressor/FET/Opto/VariMu are presets), `lookahead` on Gate,
  DeEsser, Transient, `phase = "linear"` on EQs, filters and BandSplit
  ([`research/blocks.md`](research/blocks.md) §4).
- **Filters:** LowPass/HighPass have a `slope` param (12/24/36/48 dB/oct, cascaded biquads with
  Butterworth stage Q's; `q` = resonance on the steepest stage). Params can be stepped
  (`ParamSpec::step`: values snap). The Matrix shows each route's source level (a bar) and
  its curve as a small graph (drag up/down to bend it, as in Serum).
- **Captures and passive mode (2026-11-25).** Listen writes one folder per capture with a
  subfolder per track (`captures.rs`: `capture.json`, `index.json`, `analysis.json` + the two
  WAVs per track; the last `KEEP` = 20 are kept, and the settings sheet shows the folder's size
  and clears it). The analysis is **fetched, not pasted**:
  `analysis::digest()` is one line per stream (~30 tokens vs ~1.1 k for the whole analysis) and
  `captures::read(id, track, stream, section)` serves single sections — the query the planned
  `get_analysis` tool will call, tested already. Derived loudness values (crest, PLR, K-system)
  left the JSON and became methods. **Passive mode** works: the switch in the settings sheet
  (and the card's own button) unloads the patch, disables the library and controls openers (the dock is hidden)
  and fills the window with the passive card; it is saved with the project (`FxParams::passive`).
  The host's track name arrives through `IcedNiceContext::new_track_info()` — no nice-plug patch
  needed — and titles the card.
- **LLM, layer 1** (2026-11-25, `crates/llm`, `assistant.rs`): chat message + loaded patch +
  values + attached capture (layer 1 still pastes the full analysis JSON) → reply and a validated patch change (edit or new), at most two repair
  rounds, Undo. One structured answer instead of tools, the same for both backends. The worker only
  proposes; the shell commits on the UI thread. An edit keeps the project's values except where a
  default moved (`Session::change_files`). The chat lives in `Shared`: survives closing the editor,
  not saved. Verified with the claude CLI in the standalone on Xvfb (edit, create, undo, cancel).
- **Pointer locking** on X11 was tested; the Windows/macOS implementations now **compile** (the
  CD builds both, 2026-11-23) but have never run.
- **Diagnostics build** (`--features diagnostics`, `crates/plugin/src/diag.rs`): a log file per
  process (macOS `~/Library/Logs/ScriptFX/`) with a system report, GPU/audio setup, patch errors,
  panics with backtraces and a 10 s heartbeat (audio callback timing vs. budget, frame timing).
  **CD** (`build.yml`): every push triggers tests and, if they pass, builds for Linux, Windows and
  macOS universal with symbols,
  the standalone, helper scripts (`.github/package/<platform>/`: install, run, collect a report
  zip) and `test-patches/`; `main` → rolling pre-release "nightly", tags `v*` → releases. **All
  three platforms build green** and the nightly pre-release publishes (last: `e804587`,
  2026-11-23). The packages have not been installed and run anywhere yet; the first test on the
  studio's M4 Mac mini was planned for 2026-11-21, result not recorded here.
- **Dev builds:** dependencies are optimized in the dev profile (`[profile.dev.package."*"]`);
  unoptimized, a debug standalone rendered Tape Glow below 1 fps and looked frozen (2026-11-21).
  Ctrl+C in the standalone only sets a flag the GUI checks per frame; close the window instead.
- **Open details:** macro value text and typed values use the target's default base and only that
  macro's contribution, not the edited base or other macros; beyond five routes a knob shows "+N"
  (edit those in the Matrix); test save/restore in a host and pointer warping under XWayland
  on Hyprland.

## Next steps (prioritised)

0. **Mod dock, rest** ([`research/shell-layout.md`](research/shell-layout.md) "Open"): the
   knobs show only the focused source's rings and meta knobs (route dots otherwise), collapsing
   the dock (window resize), the drag tag. Then decide on the rest of the shell layout (960×540
   stage, one-row notch, docked columns).
1. **Modulation, step 4 (polish):** Matrix filtering (by source/destination) and sorting; the
   open modulator's targets glow and chain blocks show dots in their modulators' colours (concept
   §§ drawer/chain); `Hz | sync` pills on block knobs (rates, delay times); a
   route's meta knob focused right after a drop; modulators onto Surface macros. Then a host
   round trip (save project, reopen) of routes and modulator edits.
2. **Surfaces** ([`GLOSSARY.md`](GLOSSARY.md), decided 2026-11-25). Done: the tier is collapsed —
   `shell/viewport.rs` holds `SurfaceId` and the switcher list, the shell's per-view special cases
   are per-surface traits (`owns_block_modal`), `notch::View` is gone. Remaining:
   **(a)** move the chain's state out of the shell struct (`page`, `motion`, `chain_content`) so
   each surface owns its own — the point of the exercise; **(b)** rename `ui.Panel` → `ui.Surface`
   in the Luau API, `has_panel` → surfaces, and `stage.rs`'s `Panel` with it (breaking, all six
   patches); **(c)** let a patch declare 0..n surfaces, which the switcher then lists.
3. **Block modals:** build the displays designed in
   [`research/mockups/block-displays.html`](research/mockups/block-displays.html), in the order of
   [`research/block-displays.md`](research/block-displays.md) (steps 1 and 2 done: the `dsp::preview`
   ones restyled, every block drawable from its knobs has a display; next live data through
   subscribed taps: Gain, Width, DC block still show a card). Band split and MonoBass have a `slope`
   (24/48 dB/oct, LR4/LR8); Haas takes negative delays (delays the other channel); knob groups as pages and choices
   (same doc, §3). Walking the chain must take rapid clicks on ‹ › (and
   repeated ← →): today a click is ignored until the running pan has finished; queue it or retarget the
   pan from where it is. Not yet checked:
   light mode, UI scale ≠ 100 %, a scrolled chain.
4. **Blocks for professional chains** ([`research/blocks.md`](research/blocks.md) §4): next the
   sidechain (an internal detector chain on dynamics blocks, then an external key input), the
   elementary layer (§5: DelayLine, TransientSplit, Feedback first), then the P2 list. The big
   blocks outgrow their modal: the compressor has 14 knobs, and `phase` and `lookahead` aren't
   shown at all — they need the knob groups and choices from step 3.
5. **Hands-on check in Reaper and Carla**: everything above plus knob drag/double-click on all
   panels, chat typing (Enter/Shift+Enter, selection, clipboard, dead keys), Listen, UI scale
   (live and reopening at ≠ 100 %, host accepts the resize), library wheel scrolling, controls
   sheet (automation gestures, names), hot reload + error card, unload, close/reopen ×10, two
   instances, HiDPI.
6. ~~**Snapshots**~~ built 2026-11-25 (see "Snapshots" above). Left: check the native file
   dialogs on Windows, macOS and in hosts; factory snapshots shipped with patches, if wanted.
7. **Multi-track Listen** (the real feature, with the chat; all decided 2026-11-25, Flemming).
   Clicking Listen lets the user **choose which instances to record**, default: only the current
   one. Every selected instance contributes its **input and output** with the **full** analysis
   (~1.2–1.5 k tokens per stream, so a 4-track capture is ~10–12 k of JSON; see "Compressing the
   analysis" in the backlog before that grows). Pieces:
   - **Instance registry**, process-wide (`Weak<Shared>` per instance, dropped with it). Only
     instances **in the same host process** are discoverable, so the selector lists ScriptFX
     instances, not DAW tracks: a track joins the comparison by having a (passive) instance on it.
     Same property as FabFilter Pro-Q's instance list. Sandboxed hosts would need IPC — not now.
   - **Names and colours from the host**: `TrackInfo { name, color }`
     (`vendor/nice-plug-core/src/plugin/track_info.rs`; VST3 `IInfoListener::setChannelContextInfos`,
     CLAP track-info) arrives through `Editor::track_info_updated`, so it only lands once an
     editor exists → cache it in `Shared` on first delivery and save it with the project;
     fallback: a user-editable name. The colour tints the selector row like the DAW's track.
   - **One common window**: the initiating instance's record start (ARMED→RECORDING on signal)
     drives every selected instance immediately, bypassing their own signal threshold — a quiet
     track must not stall the capture; it reports silence instead. Both ends stamp the transport
     position so the analyses can be trimmed to the same window.
   - **No rolling buffer** anywhere (see "Listening"): a capture only ever starts from a click.

   **Done 2026-11-25:** the storage and the retrieval query under all of this (`captures.rs`:
   one folder per capture with a subfolder per track, `index.json`, `captures::read(id, track,
   stream, section)`, retention of the last 20), `analysis::digest()` (one line per stream,
   ~30 tokens), and **passive mode** (see below). **Open:** the instance registry, the selector
   UI, the synchronised record start, and `get_analysis` itself (layer 2 in `crates/llm`).
8. **LLM integration, rest** (layer 1 built 2026-11-25, see above): layer-2 tools (`get_analysis`,
   `request_listen`), streaming replies, saved/shared global chats, a diff view, Codex/ACP
   ([`research/llm-adapter.md`](research/llm-adapter.md)). Check in a host, with a real API
   provider (only tested against a fake server) and the keychain on all three platforms.
   Still open: which cheap/free models.

## Vision

"Max for Live with an LLM": an effect plugin where **patches are Lua scripts** that
*declaratively* describe an effect chain built from native blocks, the host-facing macros,
and the patch's own UI. An LLM (prompt / chat inside the plugin) writes and adjusts these
patches, informed by an analysis of the actual audio on the track (and the mix).

## Glossary

Moved to [`GLOSSARY.md`](GLOSSARY.md): the terms used in code, UI and docs, the
**viewport → surface** model (settled 2026-11-25) and the list of words to avoid.

## Decisions

| Topic | Decision | Why |
|---|---|---|
| Language | **Rust** | readable for a Go dev, compiler catches real-time mistakes (`assert_no_alloc`) |
| Plugin framework | **nice-plug** (community continuation of nih-plug), vendored + patched (`vendor/nice-plug`, `vendor/nice-plug-core`) | nih-plug is in maintenance mode; nice-plug maintains the iced adapter. Before: nih-plug (vendored); clack + clap-wrapper lost its advantage once we went VST3-first |
| Formats | **VST3 first** (CLAP built for free) | all 3 machines (Fender Studio/Win, Studio One 6/Mac, Reaper+Carla/Linux) speak VST3. No AU, no AAX |
| Platforms | Linux x86_64, Windows x86_64, macOS universal (arm64 + x86_64) | via GitHub Actions |
| Blocks | **Building blocks for professional chains**: expose every parameter a device or engineer would set; continuous ones are Params; styles are presets of one versatile block where possible (Flemming, 2026-11-21) | patches (and the LLM) can only build what the blocks allow; see [`research/blocks.md`](research/blocks.md) |
| Modulation amount | **One control**: the amount's sign is the direction (no separate invert); it is the size of the range for both polarities (bipolar swings ±amount/2) (Flemming, 2026-11-21) | one knob says everything; switching uni/bi doesn't change how far the knob moves |
| Viewport terminology | **viewport → surface**, two tiers: the chain and the Matrix are surfaces like a patch's own UI, each owning its state; a patch may declare 0..n surfaces and the notch switcher lists them. "page" and "view" (as nouns) retired (Flemming, 2026-11-25) | one tier instead of three; per-view special cases become per-surface metadata, and the chain's modal state leaves the shell struct. See [`GLOSSARY.md`](GLOSSARY.md) |
| Lua's role | **describes only** (params, macros, chain, theme) and renders the UI components (UI thread, sandboxed + instruction budget); never on the audio thread | real-time safety, LLM-friendly validation |
| Patch model | **params** = the blocks' knobs (unlimited, base value saved with the project) + **macros** (max 16, host-automatable, each drives 1..n params) | 8–16 macros are enough when macros can drive many params; hosts can't handle changing parameter lists |
| Modulation | **Serum's model** (2026-11-20, source of truth: [`research/serum-modulation.md`](research/serum-modulation.md)): every source → param link is a slot (amount −1…+1 of the knob's travel, uni/bi, curve, bypass; no invert, see "Modulation amount"); deltas sum on the knob's base in normalized space, clamped, then the param's scale; any number of slots per knob. Sources: macros and the modulators (Follow, LFO 1–3, Step). Slot edits saved per DAW project | the earlier unit-based from/to mappings didn't add up across lin/log knobs; Serum's model is proven and intuitive |
| Chain editor | a **tree** with container slots (not a free graph); surfaces: Surface, Chain (+ block modals), Matrix; concept: [`research/chain-editor.md`](research/chain-editor.md) | cable salad at mid-size patches in the free-graph mockup |
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
| Listen / analysis | user-triggered capture of the chain's **input and output** (10/20/30 s, silence skipped), per selected instance (see "Mix context"); planned LLM integration sends **all** analyses (~2.5–3k tokens per stream). A summarising LLM pass is a later optimisation — and the first place to look once multi-track captures get big | |
| Listening | **Explicit only**: every capture is one the user started, for a window they chose. **Rejected: a rolling ~30 s buffer** anywhere, including in passive mode (Flemming, 2026-11-25) | the user can't know what's in a hidden buffer most of the time; an explicit capture is something they can reason about and point the LLM at |
| ARA | **No** (evaluated 2026-11-25). Revisit only as an *extra* source once the LLM loop works | ARA would make track analysis passive, but it reads a clip's **source audio**: not our chain's output (layer 2 needs that), not the plugins before us, nothing for the master (no clips), nothing for live input. So it wouldn't replace Listen, and the C++ SDK + document controller + a fifth nice-plug patch is a large, orthogonal build. Licensing is fine (Apache 2.0) and Reaper/Studio One do host ARA 2 |
| Mix context | **the user picks the instances** a Listen records, default: only the current one (Flemming, 2026-11-25). Each contributes input + output, full analysis. Supersedes the earlier "current track + master only" rule, which stays as *guidance*: the master is the useful second target, and a bus or send skews a track-vs-bus comparison | the earlier fixed pair was a guess at what's useful; the selector costs one UI and makes "compare these two guitars" possible too. Only in-process instances are reachable, so the choice is limited to tracks that carry a ScriptFX |
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
  and block modals display the Luau result and edit parameter values, not the chain's source.

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
- **Serum-matched design mockup** (Flemming, 2026-11-21: todo). Come up with a design that tightly
  matches Serum's, as a mockup first (like the chain-editor mockup artifact): the drawer's
  modulator panels (LFO shape editor with grid and playhead, envelope/step views), source drag
  handles, meta knobs and range arcs on knobs, the Matrix. First draft (2026-11-21): claude.ai
  canvas "ScriptFX Serum-style Modulation Mockup" (https://claude.ai/artifact/P6CjCAgVKgyp1QLbN2DqQA,
  private): block page + LFO shape editor, drag to assign, Matrix with aux/curve/inspector, knob
  anatomy, modulator panels. Reference:
  [`research/serum-modulation.md`](research/serum-modulation.md); pairs with the editable LFO
  shape (backlog "DSP / chain").
- **Chain editor: more input types for block/modulator params** (Flemming, 2026-11-19). Knob cells
  and mode pills (`Hz | sync`) exist; the pills are wired up for modulator rates only, and the
  knob groups/choices the big blocks need are next steps 3 and 4. **Still to design:** switches
  (bypass, ping-pong), choices (filter type, LFO shape), maybe steppers — and `ms | sync` on block
  times.
- **Chat: more feedback, rich elements** (Flemming, 2026-11-25; open what exactly). Candidates:
  steps of a running turn (thinking → checking → repairing, with what failed), a change card (which
  blocks/macros/values changed, code diff), before/after capture card instead of the table, A/B
  between the versions, the panel's render as a thumbnail.
- **Chat as a main view instead of a sheet** (Flemming, 2026-11-25: contemplating). More room; but the
  sheet keeps the panel in sight. Proposal: a fourth stage view, conversation left, the turn's rich
  elements right, the dock below stays playable. Mockup first.
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
- **Not yet:** chat logs and attached captures are in memory per plugin instance (sharing and
  persistence: "Global chats" above). Captures use one shared folder and second-resolution names, so
  concurrent captures can collide. Param values and slot settings are dropped on patch switch
  (save a snapshot to keep them).
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
- Crossfade on graph swap: `Graph::adopt` already carries tails and phases over when only routing
  changes, so route and modulator edits are clickless. A **changed chain** (patch switch, hot
  reload) still swaps hard.
- **Sidechain in + out to/from other tracks** (Flemming, 2026-11-19; mechanism open; the internal
  detector chain on dynamics blocks is next step 4). **In:** the host's sidechain input bus
  (VST3/CLAP aux in, the DAW routes it), usable as a key or modulation source anywhere in the
  chain; unlocks `Ducker`, keyed compressors, "low band keyed by …". **Out:** aux output buses the
  DAW routes to other tracks (same machinery as "multi-output taps" above), and/or direct
  instance-to-instance sharing in-process (same registry as passive mode). Check how each host
  routes plugin sidechains (Reaper, Studio One, Fender Studio).
- Rider: relative target (follow the input level) instead of an absolute dBFS

### Idea: analysis / debug block
A block that shows a general-purpose analysis (level, spectrum, stereo, dynamics) of the audio going
into it, anywhere in the chain. The Listen feature could tap into the chain through these blocks,
not only at the plugin's input/output.

### Platform / build
- Upstream our patches: runtime param names (nice-plug), X11 frame pacing / poll-timer rate / app
  resize (baseview / iced_baseview), timestamp queries and the post-process pass (iced_wgpu). Re-vendor on nice-plug updates.
- The GitHub Actions builds are green on all three platforms and the nightly pre-release publishes
  (verified 2026-11-23). **Open:** install and run the packages — Windows (Fender Studio) and
  macOS (Studio One 6, including Retina @2x performance).
- Run the pointer lock's FFI (`shell/pointer_lock.rs`) on Windows and macOS: both compile in CI
  now, but neither has been exercised.
- Hide unused host slots in the host (needs another nice-plug patch; flags + rescan)
- Licensing: nice-plug exports VST3 through the `vst3` crate (MIT/Apache-2.0), not `vst3-sys`
  (GPLv3): our binaries are no longer forced to be GPLv3. Check the remaining dependencies before
  picking a license.
