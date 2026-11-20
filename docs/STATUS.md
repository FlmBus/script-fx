# ScriptFX: status, decisions & next steps

_Last updated: 2026-11-20. Read this first when picking the project back up. How to build, run and
write patches: [`README.md`](../README.md). What each session built: [`HISTORY.md`](HISTORY.md).
Concepts, research and measurements: [`research/`](research/)._

## Where we left off

**All of this is on branch `notch-drawer` (worktree `.claude/worktrees/notch-drawer`), uncommitted.**
Built 2026-11-19/20 (part of it by another agent, opencode, then reviewed and fixed). Concept:
[`research/chain-editor.md`](research/chain-editor.md). Not checked in a real host yet; tested in
the standalone off-screen.

- **Views** (notch: Surface | Chain | Matrix, sliding tab highlight in every tab bar, `shell/tabs.rs`):
  - **Surface**: the patch's ControlPanel, or without one its macros as plain knobs in the patch's
    theme (`shell/surface.rs`: `colors.bg/accent/ink`, title = logo or name in `fonts.title`).
  - **Chain**: the audio tree on a dotted ground, containers as cards with slots; the line
    **snakes** through rows (left→right, down, right→left, …) when wider than the window.
  - **Block page** (click a block; zoom transition with a post-process radial blur in vendored
    `iced_wgpu`): header, a live **display** (`shell/block_display.rs`, curves from `dsp::preview`
    = the DSP's own formulas: filter/EQ response, shaper transfer, dynamics curve + GR, delay
    taps; other blocks: one line of text, **what they show is open**), the block's knob cells.
  - **Matrix**: not built.
- **Modulation = Serum's model** (decision below): slots with amount −1…+1, uni/bi, curve tension,
  invert, bypass (`patch::ModSettings`); `Macro:drives(param, { from, to })` converts (base =
  `from`, amount = normalized from→to; exact on log knobs; factory patches unchanged). DSP reads
  slot settings live (one packed `AtomicU64` each); saved per DAW project (persist key
  `mod-slots`), cleared on a patch switch.
- **Knob cell** (block pages; Serum's layers): ticks lit to the base value; on the outer ring the
  focused slot's **range arc** (base → base ± amount, both ways when bipolar) with a drag handle,
  and the **live dot** (soft white) at the value after all slots; the pointer stays at the base.
  **Meta knobs** in the top line, one per slot: press = focus its arc, drag = amount, double-click
  = the patch's setting, wheel, right-click = menu (amount, uni|bi, curve, invert, bypass, reset).
  Patch panels show the heard value in their readouts (they can't draw arcs).
- **Input:** no hand cursor over knobs/faders; up/down cursor over meta knobs; the cursor is
  **hidden and the pointer locked in place** while dragging (`shell/hide_cursor.rs`,
  `shell/pointer_lock.rs`: X11 via x11rb tested; Windows/macOS code written, **not compiled**).
  A press anywhere on the page closes everything floating (`shell/press.rs`, `dismiss()`).
- **Macros** left the drawer for the **macro rail** ("Macros" in the top bar, left slot shared
  with the library; 2×3 pages, sliding page change). The drawer holds only the modulator tabs.
- **Diagnostics build** (`--features diagnostics`, `crates/plugin/src/diag.rs`): a log file per
  process (macOS `~/Library/Logs/ScriptFX/`) with a system report, GPU/audio setup, patch errors,
  panics with backtraces and a 10 s heartbeat (audio callback timing vs. budget, frame timing).
  CI workflow `mac-diagnostics.yml` builds it for Apple Silicon with symbols, the standalone,
  helper scripts (`.github/diagnostics/`: install, run, collect a report zip) and `test-patches/`.
  First test on the studio's M4 Mac mini: 2026-11-21.
- **Open details:** a macro's value text with several macros on one knob uses the default base,
  not the current one; at most 3 meta knobs fit a knob's top line; save/restore of slot settings
  in a host; does XWayland on Hyprland allow the pointer warp?

## Next steps (prioritised)

1. **Commit the `notch-drawer` work** and merge it (two days of uncommitted work in a worktree).
2. **Layout: block knobs and macros must be on screen together** (Flemming, 2026-11-20): drag &
   drop of a source onto a knob is impossible while block pages and the macro rail exclude each
   other. Copying Serum's modulation means copying its layout too, at least partly; maybe the
   macros go back into the drawer (sources along the bottom, destinations above, as in Serum).
3. **The macro surface becomes the default panel** for patches without their own, and **panels
   are renamed to surfaces** everywhere (API `ui.Panel`, code, docs) (decided, Flemming). Covers
   the old "standard library of generic panels" wish.
4. **Modulators** (Follow, LFO 1–3, Step; concept in `research/chain-editor.md`): engine in `dsp`,
   the drawer's modulator panels, drag & drop onto knobs (new slots), then the **Matrix** view.
5. **Block pages:** displays for the remaining blocks; more input types (switches, choices).
6. **Hands-on check in Reaper and Carla**: everything above plus knob drag/double-click on all
   panels, chat typing (Enter/Shift+Enter, selection, clipboard, dead keys), Listen, UI scale
   (live and reopening at ≠ 100 %, host accepts the resize), library wheel scrolling, controls
   sheet (automation gestures, names), hot reload + error card, unload, close/reopen ×10, two
   instances, HiDPI.
7. **Snapshots** (saved values of a patch): the notch selector/save/load and the library rows are
   placeholders ("Init" = defaults) until then.
8. **Passive mode**: bypass, keep a rolling ~30 s buffer; a track instance's listen pulls the
   passive instance's audio for the same time window (host transport position) and adds "track vs.
   mix" analysis. In-process registry first (Reaper/Studio One/Fender Studio run plugins
   in-process); sandboxed hosts need IPC. Channel names: CLAP track-info / VST3 IInfoListener
   (nice-plug has `track_info_updated`), fallback: user-named instances.
9. **LLM integration**: chat → patch generation (layer 1 with an attached capture), then layer-2
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
| **Chain / Block** | DSP description / one building block (`Chain.new`, `Blocks.<Type>.new(id, {...})`) | `patch::chain`, `BlockSpec` |
| **Chain view / Block page** | the view of the chain's tree / one block's page (display + knobs) | `shell/chain_view.rs`, `shell/block_page.rs` |
| **Graph** | compiled chain running on the audio thread | `dsp::Graph` |
| **Param** | a block's knob; has a base value (saved with the project) | `ParamSpec` |
| **Macro** | host-automatable control of a patch, drives 1..n params | `MacroSpec` |
| **Host slot** | one of the 16 fixed host parameters macros are mapped onto | `MACRO_SLOTS` |
| **Modulation slot** | one source → param link: amount, uni/bi, curve, invert, bypass | `patch::Modulation`, `ModSettings` |
| **Meta knob / range arc / live dot** | a slot's mini knob on its target / the slot's range on the knob / the value after modulation | `shell/knob.rs` |
| **Modulator** | Follow, LFO 1–3, Step: sources that live outside the chain. Not built | — |
| **Drawer / Macro rail / Notch / Sheet** | modulator tabs at the bottom / the macros' card on the left / view switch + snapshots at the top / side panels (library, controls, chat, settings) | `shell/drawer.rs`, `macro_rail.rs`, `notch.rs`, `sheet.rs` |
| **Matrix** | every modulation slot as a table. Not built | — |
| **Bind / Source** | what a component reads: `bind` = a Macro/Param object (value, draggable), `source` = a metered block or `Meters.IN/OUT` | `__ref`, `patch::Source` |
| **Theme** | a patch's design tokens (colours, fonts, images); used by its ControlPanel, library card and macro surface, never by the rest of the shell | `Theme` |
| **Listen / Capture / Analysis** | the action / recorded audio / analysis result | `listen.rs`, `crates/analysis` |
| **Passive mode** | the instance only listens (no patch, passthrough) and shares its audio with other instances. Not built | — |
| **Chat** | conversation with the LLM, captures as attachments; **global**. The UI exists, the LLM doesn't yet | `shell/chat.rs` |

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
| Lua's role | **describes only** (params, macros, chain, theme) and renders the UI components (UI thread, sandboxed + instruction budget); never on the audio thread | real-time safety, LLM-friendly validation |
| Patch model | **params** = the blocks' knobs (unlimited, base value saved with the project) + **macros** (max 16, host-automatable, each drives 1..n params) | 8–16 macros are enough when macros can drive many params; hosts can't handle changing parameter lists |
| Modulation | **Serum's model** (2026-11-20, source of truth: [`research/serum-modulation.md`](research/serum-modulation.md)): every source → param link is a slot (amount −1…+1 of the knob's travel, uni/bi, curve, invert, bypass); deltas sum on the knob's base in normalized space, clamped, then the param's scale; any number of slots per knob. Sources today: macros; modulators (Follow, LFO 1–3, Step) planned. Slot edits saved per DAW project | the earlier unit-based from/to mappings didn't add up across lin/log knobs; Serum's model is proven and intuitive |
| Chain editor | a **tree** with container slots (not a free graph); views: Surface, Chain (+ block pages), Matrix; concept: [`research/chain-editor.md`](research/chain-editor.md) | cable salad at mid-size patches in the free-graph mockup |
| Host parameters | 16 fixed host slots, **names follow the patch** (patched nice-plug: VST3 `kParamTitlesChanged`, CLAP `RESCAN_INFO`). **Verified in Reaper.** Automation stays on the slot across patch changes (same contract as Serum's macros) | plugins can't delete host automation; renaming is safe because hosts key automation by ID |
| DSP | declarative `chain` → `dsp::Graph` compiled off the audio thread, swapped lock-free (rtrb), old graphs freed off-thread | |
| Patch UI | **reactive Luau components** → SVG → one cached layer each (resvg), fixed 840×560. Parked on 2026-11-18 for the raster plan, then revised and back: **still the plan**; raster layers / sprite sheets / shaders stay open ideas ([`research/raster-ui-plan.md`](research/raster-ui-plan.md)) | declarative, cached, sandboxed; the earlier SVG + bindings panel was removed |
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
| Patch kinds | **Graph patches** get a visual graph editor: a new, data-only chain definition next to the coded Luau patches (decided 2026-11-19; design open, see "Open directions") | Luau code can't be edited visually (loops/helpers don't round-trip) |
| LLM integration (planned) | **Layer 1**: the user attaches a capture to the prompt. **Layer 2**: the LLM gets tools: `get_analysis(section, detail)` and `request_listen(where)` → closed loop: write a patch, listen to the output, compare, adjust | the LLM can't press play; the human always plays the track |

## Open directions

### Patch code and graph patches (partly decided)
Research (precedents, Kontakt, WebAssembly/WebGL, threat model, spikes, decision matrix):
[`docs/research/patch-code-and-sharing.md`](research/patch-code-and-sharing.md). Short version:
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
  `ms | sync` on time). Still to design: switches
  (bypass, ping-pong), choices (uni/bi, filter type, LFO shape), maybe steppers.
- **Chat** (decided 2026-11-19: **global**, one default chat + optional more, opened from the top
  bar; captures as message attachments; Listen lives in the chat). Only one chat ("Main") so far,
  not saved.
- **Persist chats: decided 2026-11-19, in the user data dir** (with model auth and LLM config; keys
  in the OS keychain per `research/llm-adapter.md`). Open: should a generated patch link back to
  the chat that made it?
- Shell follow-ups: letter-spacing (IN/OUT, tracked titles) isn't supported by iced text;
  accessibility labels for icon buttons (no iced equivalent yet); full font files (the bundled
  ones are Latin subsets, "→" falls back); per-node clip LEDs (no per-block peaks); library card
  background art (cards use a gradient from the theme colours).
- Components, open/next: first-open cost at 2x (150 ms) → persistent raster cache or
  background-thread rendering; bool/enum params for toggles/selectors (toggles are local UI state
  only so far); `p.series` data (spectrum) from the analysis engine; test an LLM writing a patch +
  UI from the type stubs alone; hit areas are rectangles; `measure_text()` helper if layouts need
  text widths.

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
- Push to GitHub (install `gh` or create the repo manually) and run CI; test on Windows (Fender
  Studio) and macOS (Studio One 6, including Retina @2x performance).
- Compile-check (and try) Windows and macOS: the pointer lock's FFI (`shell/pointer_lock.rs`) has
  never been built there.
- Hide unused host slots in the host (needs another nice-plug patch; flags + rescan)
- Licensing: nice-plug exports VST3 through the `vst3` crate (MIT/Apache-2.0), not `vst3-sys`
  (GPLv3): our binaries are no longer forced to be GPLv3. Check the remaining dependencies before
  picking a license.
