# Findings and measurements

_Collected from README and STATUS (2026-11-20): what the proofs of concept measured and taught.
Numbers are from the dev machine (Linux, one core at 48 kHz unless noted) at the time; the code has
moved on since (e.g. Luau, nice-plug, iced), so treat them as orders of magnitude. Current state:
[`../STATUS.md`](../STATUS.md); what each session built: [`../HISTORY.md`](../HISTORY.md)._

## resvg/usvg (PoC #1: SVG panel + bindings; removed, superseded by components)

**Mapping of the declarative model: very good.**
- An SVG element `id` is the binding target. Designers keep using Inkscape/Figma; the Lua side
  only names IDs. Theme → CSS classes via usvg's `style_sheet` option works without touching the SVG.
- "Patch & reparse": bindings produce transform/style/text patches, which are spliced into the
  original source at byte offsets (roxmltree positions) and then fully re-parsed. This is lossless,
  so `xlink:href`, namespaces and existing `transform`/`style` attributes all survive. A DOM round-trip
  with xmltree would have dropped namespace prefixes.
- Hit areas for `drag` come from usvg bounding boxes and are refreshed after each render, so they
  follow moving parts.
- Validation at load time catches unknown macros/sources and escaping paths. Missing SVG IDs become warnings.

**Security: good, with two things to configure explicitly.**
- resvg has no scripting, network access or animation engine.
- usvg's *default* `<image href>` resolver reads arbitrary local files, so it is replaced with one
  restricted to the patch folder. DTDs are disabled for our own parse.
- Lua (now Luau): sandbox mode (read-only globals/builtins), no `io/os/loadstring/getfenv/setfenv`,
  `require` restricted to the patch folder, a 16 MB memory cap and an interrupt budget per call.

**Performance: the weak spot.** Release build, 640×360 panel:

| patch | lua animate | patch | usvg parse | resvg render | total @1x | total @2x |
|---|---|---|---|---|---|---|
| tape-glow (gradients, blur, text) | 22 µs | 15 µs | ~0.9 ms | ~4.2 ms | ~5.2 ms | ~13 ms |
| clean-gain (flat) | – | 6 µs | 0.5 ms | 0.45 ms | 0.9 ms | 1.4 ms |

- Lua per-frame scripting is essentially free (~25 µs), so a scripted UI thread is cheap.
- The cost is the full-canvas CPU rasterization each frame. Removing blur and glow saves only
  ~1 ms, so the canvas size dominates. Nothing renders when inputs are unchanged, but continuous
  animation (reels) means a render every frame.
- Next optimisation, if we keep resvg: split the document into static and dynamic runs, cache the
  static layers as pixmaps and re-render only the dynamic nodes (`resvg::render_node`). Or move to a
  GPU renderer with the same model (vello + vello_svg).

**Fonts:** `load_system_fonts()` took **7.5 s cold** (73 ms warm). That's unacceptable in a DAW and
also makes rendering differ between machines. Now the plugin bundles Noto Sans (≈2 ms) and patches can ship `fonts/`.

## Declarative chains (bass-helper)

Engine: patch `chain` → `dsp::Graph` built off the audio thread → handed over through a
lock-free queue (`rtrb`); replaced graphs travel back and are freed off the audio thread.
32-sample sub-blocks, settings smoothed. Verified allocation-free (`assert_no_alloc` test over all patches).

| patch | blocks | CPU (stereo 48 kHz, one core) |
|---|---|---|
| bass-helper | 11 (incl. split, 2 comps, rider, transient, drive, 4 EQ) | **0.7 %** |
| tape-glow | 3 | 0.1 % |

Macro sweeps on a synthetic bass line (`cargo run --release -p dsp --example bench`) show each
macro does what it says (Transients: crest 10 → 19 dB; Stabilize: crest 13 → 10 dB at constant
RMS; Drive: darker + denser; Boom/Click: band lifts).

What it took / what's missing:
- **Boilerplate**: every block setting a macro touches needs its own param
  (bass-helper: 16 params for 7 macros). Idea: let macro targets address block settings
  directly (`{ block = "fet", setting = "threshold", from = -4, to = -30 }`).
- Settings are plain linear values: no expressions (e.g. "threshold relative to input level").
  Auto-makeup had to become a block feature (static formula over-compensated by ~8 dB on quiet input).
- Topology: serial, N-band split, parallel branches with crossfade (all nestable). Missing:
  sidechain (e.g. low band keying the comp), modulators (LFO/envelope as sources), lookahead.
- Graph swap resets DSP state → a click on patch switch / hot reload (needs a crossfade).
- Drive aliased at high settings (fixed: per-block oversampling, see Patch model).

## Plugin framework + shell integration

- Shell: iced on nice-plug (see docs/research/shell-ui-toolkit.md). Component layers → one iced
  image each, positioned in the stage; a small pointer widget turns input into param changes.
- Macro sliders are auto-generated with patch units (the controls sheet).
- **Host params are fixed**: 16 slots; patches map their macros onto them in order. DAWs
  don't tolerate parameter lists that change at runtime, but renaming is fine (automation is
  keyed by ID). The vendored nice-plug is patched to push names (VST3 `kParamTitlesChanged`,
  CLAP `RESCAN_INFO`). **Needs testing per host** (Reaper, Carla, Studio One 6, Fender Studio).
- The patch name is persisted with the host project. Routing (macro → DSP) is applied in
  `activate()` so audio is right even when the editor never opens.

## Linux gotchas on the dev machine (NVIDIA + Hyprland/XWayland + PipeWire)

- The shell renders with wgpu (Vulkan), no GL: the old GLX problems on the NVIDIA proprietary
  driver are gone, the standalone needs no Mesa workaround.
- X11 frame pacing under XWayland: see vendor/baseview/PATCHES.md.
- Standalone with **JACK backend on PipeWire never called `process()`**, so parameters never
  updated. `-b alsa` works.
- Tiling WMs resize the standalone window, but the editor keeps its own size (top-left).
- Synthetic input (xdotool) doesn't reach XWayland windows on the desktop; test off-screen on
  Xvfb (`DISPLAY=:99`, `WGPU_BACKEND=vulkan`), where clicks, cursor shapes (XFixes) and pointer
  warps can all be checked.

## Component UI engine (2026-11-18)

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

## Analysis (Listen)

- Loudness matches ffmpeg's `ebur128` (identical LUFS, ±0.1 dB true peak / LRA).
- Caveats: "resonances" include notes/harmonics of tonal sources; onset detection misses soft
  re-attacks at equal level; the rider uses an absolute target level.
