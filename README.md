# ScriptFX sandbox

PoCs for a plugin whose effect patches are Lua scripts: Lua *describes* macros, theme and
UI declaratively, and native Rust does the DSP and rendering.

```
crates/patch         patch model (params + macros + chain), sandboxed Luau runtime + require, patch & UI API (src/*.luau)
crates/analysis      offline analysis of a capture → LLM-friendly JSON (loudness/LUFS, spectrum, dynamics, stereo, pitch)
crates/dsp           native DSP: compiles a patch's chain into a graph (no alloc/locks/Lua on the audio thread)
crates/ui-components reactive Lua components → SVG → one cached layer per component (the patch UI: its `panel`)
crates/plugin        package `scriptfx`: nih-plug plugin (VST3/CLAP/standalone), egui shell + component panel
crates/xtask         `cargo xtask bundle` (nih_plug_xtask)
vendor/nih-plug      patched nih-plug (runtime parameter names), see vendor/nih-plug/PATCHES.md
patches/             tape-glow (multi-target macro, reels/VU), clean-gain (minimal, fader),
                     bass-helper (punch → FET → opto → band split: ride / parallel grit → EQ; live block meters),
                     neon-saturator, junkyard-amp (component UI stress/fire tests)
patches/.types       Luau type definitions (scriptfx.d.luau) → type checking + autocomplete; doubles as API spec for an LLM
```

## Build & run

```sh
export CARGO_TARGET_DIR=/tmp/scriptfx-target     # /home was full; tmpfs build dir
cargo test --workspace
cargo xtask bundle scriptfx --release            # → $CARGO_TARGET_DIR/bundled/ScriptFX.{vst3,clap}

# install for local hosts (Reaper, Carla ≥ 2.6 for CLAP; Carla 2.5 = VST3 only)
cp -r $CARGO_TARGET_DIR/bundled/ScriptFX.vst3 ~/.vst3/ && cp $CARGO_TARGET_DIR/bundled/ScriptFX.clap ~/.clap/

# standalone (see "Linux gotchas" for the env vars)
SCRIPTFX_PATCHES=$PWD/patches __GLX_VENDOR_LIBRARY_NAME=mesa LIBGL_ALWAYS_SOFTWARE=1 \
  $CARGO_TARGET_DIR/release/scriptfx -b alsa

# headless render + per-layer cost of a patch UI
cargo run --release -p ui-components --example render -- patches/tape-glow /tmp/out.png [scale] [bind=value ...]
```

**Patches location:** `$SCRIPTFX_PATCHES`, else the user data dir (`~/.local/share/ScriptFX/patches`,
`~/Library/Application Support/ScriptFX/patches`, `%APPDATA%\ScriptFX\patches`). The bundled
patches are compiled in and copied there on first run; existing files are never overwritten.
For patch development, point `SCRIPTFX_PATCHES` at this repo's `patches/`. Hot reload picks up
edits, errors show in the footer, and the last good patch keeps running.

**Listen** (footer, 🎧): waits for signal, records N seconds of the chain's input *and* output
(silence skipped), analyses both off the audio thread, shows the result and can attach it to the
prompt. Saved to `<data dir>/ScriptFX/captures/capture-<time>/` (`analysis.json`, `input.wav`, `output.wav`).
Loudness verified against ffmpeg's `ebur128` (identical LUFS, ±0.1 dB true peak / LRA).
`cargo run --release -p analysis --example analyze -- file.wav` analyses a WAV from the command line.

Dev aids for the standalone: `SCRIPTFX_TEST_INPUT=file.wav` (float WAV at the host rate, looped as
input), `SCRIPTFX_DEV_AUTOLISTEN=10` (start a 10 s listen when the editor opens).

**CI** (`.github/workflows/build.yml`): tests, then VST3 + CLAP bundles for Linux x86_64,
Windows x86_64 and macOS universal (arm64 + x86_64, ad-hoc signed, not notarized → remove
quarantine, see the README in the artifact).

## Patch model

Patches are **Luau** (typed Lua; mlua's `luau` feature). `patch.luau` builds the patch with a
small API and returns it; it `require`s its UI files (relative paths, only inside the patch
folder) and passes the real objects into the UI. Constructors take the identity positionally
(id, display name) and every setting by name in an options table:

```lua
local ui = require("./ui")

local drive = Param.new("drive", "Drive", { min = 0, max = 24, unit = "dB", default = 6 })
local tone  = Param.new("tone",  "Tone",  { min = 500, max = 20000, unit = "Hz", default = 8000 })

-- host-automatable (max 16), name shows up in the DAW; drives 1..n params
local character = Macro.new("character", "Character", { default = 0.3 })
  :drives(drive, { from = 0,     to = 18 })
  :drives(tone,  { from = 16000, to = 2500, curve = "exp" })

local comp = Blocks.FETCompressor.new("comp", { threshold = -18 })   -- metered: the UI can show it
local chain = Chain.new({
  Blocks.Drive.new("tape", { amount = drive }),
  Blocks.LowPass.new("tone", { freq = tone }),
  comp,
})
chain:add(Blocks.Gain.new("out", { db = -3 }))   -- it's code: loops and helper functions work

return Patch.new("Tape Glow", {
  chain = chain, macros = { character }, theme = { accent = "#ff8a3d" },
  panel = ui.Panel.new({ character = character, drive = drive, gr = comp }),
})
```

```lua
-- ui.luau: components get the objects as typed props
export type PanelProps = { character: Macro, drive: Param, gr: MeteredBlock }
local Panel = Component.define({
  name = "Panel",
  render = function(p: Props<PanelProps>)
    return {
      Knob.new({ bind = p.character, x = 360, y = 362 }),
      GrMeter.new({ source = p.gr, x = 40, y = 100 }),
      Meter.new({ source = Meters.OUT, x = 760, y = 100 }),
    }
  end,
})
return { Panel = Panel }
```

- Params are objects, not strings: block settings, macros and UI components reference them
  directly; the patch collects every param it reaches (no separate list). A param is either
  **free** (own value, editable, persisted) or **driven by exactly one macro** (locked; dragging
  its knob moves the macro).
- Host slots stay fixed ("Macro 1..16" IDs); the `macros` list order assigns them. Automation stays
  on the slot across patch changes (same contract as e.g. Serum's macros).
- Blocks, by category (the definitions file documents each as `--- <Category> · …`, shown on
  hover/completion; a test keeps every block documented):
  - Utility: `Gain Pan Width MonoBass Haas Invert DCBlock Meter`
  - EQ: `PeakEq LowShelf HighShelf`
  - Filter: `LowPass HighPass MorphFilter LadderFilter CombFilter Formant Resonator DynamicEq`
  - Distortion: `Drive Waveshaper Wavefolder Clipper Rectifier Bitcrusher RingMod Tape`
    (`Waveshaper.curve` = a Luau function or a list, sampled once at load into a lookup table)
  - Modulation: `Chorus Flanger Phaser Tremolo AutoPan`
  - Space: `Delay Reverb Shimmer` (`Delay.feedback_chain` processes every repeat; reverb =
    8-line FDN with Householder feedback, freeze)
  - Pitch: `PitchShift Granular`
  - Rhythm: `Stutter` (repeats the last slice while `active` > 0.5, e.g. a macro)
  - Dynamics: `Compressor FETCompressor OptoCompressor Rider Transient Limiter Gate DeEsser`
  - Tempo sync: LFO/time blocks take `sync = "1/8" | "1/8." | "1/16t" | "2/1"` (host tempo and
    beat position via `Graph::set_transport`). `Tremolo`/`AutoPan` also take `phase_offset`
    (degrees); synced while the host plays, their phase is locked to the beat grid. Notes as Hz or names (`"C#3"`). Converted to plain data in the prelude.
  - Code: the bigger blocks live in `crates/dsp/src/fx/<category>.rs` behind the `Fx` trait;
    behaviour tests in `crates/dsp/tests/blocks.rs` (one measurable property per block);
    `tests/fixtures/everything` uses every block (allocation-free test, type check).
  - Cost at 48 kHz (one core): most blocks < 0.4 %; reverb 0.37 %, shimmer 0.55 %, tape 0.9 %
    (oversampled), granular at 100 grains/s × 200 ms 1.2 %; all 28 blocks of the fixture 5.2 %.
  - Latency: oversampled blocks (35 samples at 2×), `Tape` (its 4 ms wow/flutter delay),
    `Limiter` (lookahead) — compensated in routing blocks and reported to the host.
  - Routing: `MidSide LeftRight BandSplit Parallel`. `MidSide`/`LeftRight` run two sub-chains in
    mono (signal on both channels, so linked detectors work); `BandSplit` = N-band
    Linkwitz-Riley (phase-aligned, sums flat); `Parallel` = summed branches, or a crossfade with
    `mix` (2 branches).
- Oversampling per block, implemented once (`dsp/src/oversample.rs`, the `Oversampled` node): every
  block takes `oversampling = 1|2|4|8` (a routing block oversamples its whole subtree; nested
  factors multiply, max 8×). Nonlinear blocks (`BlockKind::is_nonlinear`, today `Drive`) default to
  ≥ 88 kHz (2× at 44.1/48 kHz, none at 88.2 kHz+). Neighbouring blocks with the same factor share
  one up/down pair; blocks without their own setting between two of them join in.
  - Filters: linear-phase FIR halfbands (Kaiser, 71/23/19 taps per 2× stage, polyphase), ≥ 90 dB
    rejection, flat to 18 kHz at 44.1 kHz. Latency at 2×: 35 samples (0.73 ms at 48 kHz).
  - Latency compensation: every node reports its latency; `Parallel`, `BandSplit`, `MidSide`,
    `LeftRight` delay their shorter branches; the plugin reports `Graph::latency()` to the host
    (`set_latency_samples`, also when a hot reload changes it). Odd remainders are padded inside the
    oversampled domain, so latency is always whole samples.
  - Cost: ≈ 0.19 % of one core per up/down pair at 2× (48 kHz) plus the blocks at twice the rate
    (Tape Glow 0.12 % → 0.41 %, Neon Saturator 0.31 % → 0.93 % with its four drives sharing one pair).
    Drive aliasing at 10.5 kHz (7.5 kHz, +24 dB): −25 dB → −65 dB.
- UI: `Component.define`, instances via `X.new(...)`, drawing via `SVG.*`. `bind` takes a Macro or
  Param, `source` a metered block (compressors, rider, transient) or `Meters.IN/OUT`. Every
  ControlPanel is 840×560 logical units (`UI.WIDTH/HEIGHT`), scaled by the shell.
- **Two layers of checking:**
  - *Editor / CI:* `luau-lsp` type-checks against `patches/.types/scriptfx.d.luau` (strict mode via
    `patches/.luaurc`): wrong prop types, missing props, a string where a Macro belongs, a
    non-metered block as `source`, typos in fields. `cargo test -p patch --test typecheck` runs it
    (skipped if `luau-lsp` isn't installed). Components annotate their props:
    `render = function(p: Props<{ label: string }>)`; own props must not reuse input names
    (`value`, `display`, `level`, …). Not caught: unknown *extra* props.
  - *Runtime:* everything is validated when the patch loads, errors point at the line: unknown
    options (`treshold`), missing required ones, wrong types, ids used twice, a block in two chains,
    a string `bind`. Important for LLM-written patches.
- Editor setup: [luau-lsp](https://github.com/JohnnyMorganz/luau-lsp), platform "standard",
  definitions `@scriptfx=patches/.types/scriptfx.d.luau`, no Rojo sourcemap.
  - VS Code: install the recommended extension (`JohnnyMorganz.luau-lsp`); `.vscode/settings.json`
    has the rest.
  - Neovim: the definitions file must be passed as a command-line flag
    (`luau-lsp lsp --definitions=@scriptfx=<repo>/patches/.types/scriptfx.d.luau`), because the
    server ignores `types.definitionFiles` sent as LSP settings. Use the repo root as the project
    root, not the folder holding `patches/.luaurc`.

## Findings: resvg/usvg (PoC #1, SVG panel + bindings — removed, superseded by components)

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

## Findings: declarative chains (bass-helper)

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

## Integration findings (nih-plug + egui)

- Component layers → one `egui::TextureHandle` each → painted in the CentralPanel; egui widgets around it.
- Macro sliders are auto-generated with patch units (dev sidebar).
- **Host params are fixed**: 16 slots; patches map their macros onto them in order. DAWs
  don't tolerate parameter lists that change at runtime, but renaming is fine (automation is
  keyed by ID). The vendored nih-plug is patched to push names (VST3 `kParamTitlesChanged`,
  CLAP `RESCAN_INFO`). **Needs testing per host** (Reaper, Carla, Studio One 6, Fender Studio).
- **egui gotcha**: `Slider::from_get_set` calls the setter *every frame* (clamping). Without a
  change check this spams host automation gestures and overwrote patch defaults.
- The patch name is persisted with the host project. Routing (macro → DSP) is applied in
  `initialize()` so audio is right even when the editor never opens.

## Linux gotchas found on this machine (NVIDIA + Hyprland/XWayland + PipeWire)

- **baseview's GLX context crashes on the NVIDIA proprietary driver** (`GLXBadContext` in
  `make_current`). Workaround: `__GLX_VENDOR_LIBRARY_NAME=mesa LIBGL_ALWAYS_SOFTWARE=1`.
  nih_plug_egui is locked to OpenGL. Needs a look at baseview or a softbuffer-based renderer
  before this is usable in a real DAW on such setups.
- Standalone with **JACK backend on PipeWire never called `process()`**, so parameters never
  updated. `-b alsa` works.
- Tiling WMs resize the standalone window, but the editor stays 900×560 (top-left).

## Next

- [ ] Passive mode (master instance) + pulling the master capture into a track's listen
- [ ] Macro targets on block settings directly (less param boilerplate)
- [ ] Crossfade on graph swap
- [ ] Verify host name updates: Reaper (VST3 + CLAP), Carla, Studio One 6 (mac), Fender Studio (win)
- [ ] Investigate baseview GLX on NVIDIA
