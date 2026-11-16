# ScriptFX: status, decisions & next steps

_Last updated: 2026-11-16. Read this first when picking the project back up. Technical
details, measurements and run instructions are in [`README.md`](../README.md)._

## Vision

"Max for Live with an LLM": an effect plugin where **presets are Lua scripts** that
*declaratively* describe an effect chain built from native blocks, the host-facing macros,
and the preset's own UI. An LLM (prompt / chat inside the plugin) writes and adjusts these
presets, informed by an analysis of the actual audio on the track (and the mix).

## Decisions made

| Topic | Decision | Why |
|---|---|---|
| Language | **Rust** | readable for a Go dev, compiler catches real-time mistakes (`assert_no_alloc`) |
| Plugin framework | **nih-plug, vendored + patched** (`vendor/nih-plug`) | works; the plugin layer is thin, so swapping it stays cheap. clack + clap-wrapper was the alternative, but lost its advantage once we went VST3-first |
| Formats | **VST3 first** (CLAP built for free) | all 3 machines (Fender Studio/Win, Studio One 6/Mac, Reaper+Carla/Linux) speak VST3. No AU, no AAX |
| Platforms | Linux x86_64, Windows x86_64, macOS universal (arm64 + x86_64) | via GitHub Actions |
| Lua's role | **describes only** (params, macros, chain, theme, UI bindings); never on the audio thread. Only exception: optional `ui.animate()` on the UI thread, sandboxed + instruction budget | real-time safety, LLM-friendly validation |
| Preset model | **params** (internal, unlimited, saved with project) + **macros** (max 16, host-automatable, each drives 1..n params with range + lin/exp curve, Ableton-rack style). A param is free *or* driven by exactly one macro | 8–16 macros are enough when macros can drive many params; hosts can't handle changing parameter lists |
| Host parameters | 16 fixed slots, **names follow the preset** (patched nih-plug: VST3 `kParamTitlesChanged`, CLAP `RESCAN_INFO`). **Verified in Reaper.** Automation stays on the slot across preset changes (same contract as Serum's macros) | plugins can't delete host automation; renaming is safe because hosts key automation by ID |
| DSP | declarative `chain` → `dsp::Graph` compiled off the audio thread, swapped lock-free (rtrb), old graphs freed off-thread | |
| Preset UI | **SVG + declarative bindings** (resvg/usvg, "patch & reparse"), theme → CSS classes | maps very well to the declarative model; secure (no scripts/network; image loading restricted to the preset folder) |
| Plugin's own UI | egui (to be redesigned, see TODO) | |
| Fonts | bundled Noto Sans (+ optional `<preset>/fonts/`) | system font scan took 7.5 s cold |
| Factory presets | compiled into the binary, installed to `<data dir>/ScriptFX/presets`, **updated unless user-modified** (`.factory-manifest`) | |
| Strict presets | unknown fields are errors | catches typos (`treshold`), important for LLM output |
| Listen / analysis | user-triggered capture of the chain's **input and output** (10/20/30 s, silence skipped) → **all** analyses always go to the LLM (~2.5–3k tokens). A summarising LLM pass is a later optimisation | |
| Mix context | only **current track + master** (no buses: buses/sends would skew a track-vs-bus comparison). The master instance runs in **"passive mode"** | universally applicable, fewer options |
| LLM integration (planned) | **Layer 1**: the user attaches a capture to the prompt. **Layer 2**: the LLM gets tools: `get_analysis(section, detail)` and `request_listen(where)` → closed loop: write a preset, listen to the output, compare, adjust | the LLM can't press play; the human always plays the track |

## What exists (commits on `main`)

- `crates/preset`: model + sandboxed Lua loader + chain spec + validation
- `crates/dsp`: graph engine; blocks `gain eq drive comp(fet|opto|clean) rider transient split(LR4)`, block meters, no-alloc test, bench (`--example bench`)
- `crates/ui-svg`: SVG panel renderer + bindings (`rotate translate scale opacity fill text drag`)
- `crates/analysis`: loudness (EBU R128, verified against ffmpeg), 1/3-oct spectrum, tilt, centroid, energy shares, resonances, dynamics (percentiles, per-band spread, onsets, attack), stereo, pitch (YIN)
- `crates/plugin` (package `scriptfx`): egui chrome (preset picker, hot reload, macro/param sidebar, prompt field without LLM yet, 🎧 Listen + result screen + prompt chip), host-name updates, lock-free capture
- presets: `tape-glow`, `clean-gain`, `bass-helper` (punch → FET → opto → split: ride/grit → EQ; 7 macros)
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
2. **Empty default state (Kontakt-style)**: a fresh instance loads **no preset**, has an **empty chain** (passthrough, does nothing to the sound) and an empty/neutral UI. Currently it auto-loads `tape-glow`.
3. **LLM integration**: prompt → preset generation (layer 1 with attached capture), then layer-2 tools (`get_analysis`, `request_listen`). Bring-your-own API key. Validate generated presets with the strict loader; feed errors back to the LLM.

### UI redesign (plugin chrome)
- **Chat view instead of a single prompt field**: most requests need clarification or follow-up adjustments. This also cleans up the general UI, and captures/analyses stay available over the course of a conversation (shown as message attachments).
- **Open question: persist chats?** Options: (a) not at all, (b) in the plugin state (travels with the DAW project; watch the state size), (c) in the data dir next to the preset (e.g. `<preset>/chat.json`, survives across projects), (d) both. Also: should a generated preset carry its chat history?
- Preset browser, macro/param editing, listen screen and analysis view integrated into the redesign.

### DSP / chain model
- **Multi-output taps**: additional plugin outputs that tap anywhere in the chain (e.g. `tap("di_clean", { output = 2 })` after the punch/comp stage) to get a partly processed DI signal back into the DAW. Needs: aux output buses in the plugin layout (nih-plug `AudioIOLayout.aux_output_ports`), a tap block in the chain spec, and host routing tests.
- Macro targets directly on block settings (`{ block = "fet", setting = "threshold", ... }`) to cut param boilerplate
- Crossfade on graph swap (no click on preset switch / hot reload)
- Parallel chains / sub-chain dry-wet, sidechain (e.g. low band keyed by …), modulators (LFO/envelope as sources), lookahead, oversampling for drive
- Rider: relative target (follow the input level) instead of an absolute dBFS

### Platform / build
- Push to GitHub (install `gh` or create the repo manually) and run CI; test on Windows (Fender Studio) and macOS (Studio One 6, including Retina @2x performance)
- Hide unused macro slots in the host (needs another nih-plug patch; flags + rescan)
- NVIDIA/GLX: check whether the standalone tiling crash is fixed by the baseview patch; consider a non-GL renderer for the chrome
- SVG performance: static-layer caching (`resvg::render_node`) or vello; re-measure @2x
- Licensing: VST3 export goes through `vst3-sys` (GPLv3), so binaries are GPLv3 unless that dependency is replaced
