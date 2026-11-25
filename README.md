# ScriptFX sandbox

An effect plugin whose patches are **Luau scripts**: a patch *describes* its effect chain (native
DSP blocks), its host-automatable macros, its theme and its own UI, and Rust does the DSP and
rendering. The goal: an LLM in the plugin writes and adjusts patches, informed by an analysis of the
track's audio. VST3 + CLAP + standalone, Linux / Windows / macOS.

**Docs:** [`docs/STATUS.md`](docs/STATUS.md) (start here: where we are, next steps, decisions,
glossary) · [`docs/HISTORY.md`](docs/HISTORY.md) (what each session built) ·
[`docs/research/`](docs/research/) (concepts, research, [measurements](docs/research/findings.md)).

```
crates/patch         patch model (params, macros + modulation slots, chain), sandboxed Luau runtime + require, patch & UI API (src/*.luau)
crates/dsp           native DSP: compiles a patch's chain into a graph (no alloc/locks/Lua on the audio thread); `preview` = curves for the UI
crates/analysis      offline analysis of a capture → LLM-friendly JSON (loudness/LUFS, spectrum, dynamics, stereo, pitch)
crates/llm           the chat's LLM: backends (claude CLI, OpenAI-compatible API), prompt, staging + checks, commit/undo
crates/ui-components reactive Luau components → SVG → one cached layer per component (a patch's ControlPanel)
crates/plugin        package `scriptfx`: nice-plug plugin (VST3/CLAP/standalone) + the iced shell (src/shell/, one module per section)
crates/xtask         `cargo xtask bundle` (nice-plug-xtask)
vendor/nice-plug*    patched nice-plug (runtime parameter names), see vendor/nice-plug/PATCHES.md
vendor/baseview      patched baseview (X11 frame pacing, app resize), see vendor/baseview/PATCHES.md
vendor/iced_wgpu     patched iced_wgpu (frame timing, post-process pass for the zoom blur), see vendor/iced_wgpu/PATCHES.md
patches/             factory patches: tape-glow, clean-gain, bass-helper (band split, live block meters),
                     neon-saturator, junkyard-amp (component UI stress/fire tests), kitchen-sink (every
                     block, every modulator, 12 macros; no ControlPanel)
patches/.types       Luau type definitions (scriptfx.d.luau) → type checking + autocomplete; doubles as API spec for an LLM
```

## Build & run

```sh
export CARGO_TARGET_DIR=/tmp/scriptfx-target     # /home is nearly full: tmpfs build dir
# debug builds optimize dependencies (Cargo.toml [profile.dev.package."*"]): usable UI speed
cargo test --workspace
cargo xtask bundle scriptfx --release            # → $CARGO_TARGET_DIR/bundled/ScriptFX.{vst3,clap}

# install for local hosts (Reaper, Carla ≥ 2.6 for CLAP; Carla 2.5 = VST3 only)
cp -r $CARGO_TARGET_DIR/bundled/ScriptFX.vst3 ~/.vst3/ && cp $CARGO_TARGET_DIR/bundled/ScriptFX.clap ~/.clap/

# standalone
cargo build --release -p scriptfx --bin scriptfx
SCRIPTFX_PATCHES=$PWD/patches $CARGO_TARGET_DIR/release/scriptfx -b alsa   # -b dummy: UI only

# headless render + per-layer cost of a patch UI
cargo run --release -p ui-components --example render -- patches/tape-glow /tmp/out.png [scale] [bind=value ...]
```

**Patches location:** `$SCRIPTFX_PATCHES`, else the user data dir (`~/.local/share/ScriptFX/patches`,
`~/Library/Application Support/ScriptFX/patches`, `%APPDATA%\ScriptFX\patches`). The bundled
patches are compiled in and copied there on first run; existing files are never overwritten.
For patch development, point `SCRIPTFX_PATCHES` at this repo's `patches/`. Hot reload picks up
edits (the LED on Reload turns red on an error, the error shows as a card in the window), and the
last good patch keeps running. Factory files that left the bundle are removed if unmodified; old
Lua 5.4 files next to a `.luau` of the same name are renamed to `.lua.bak` (they break `require`).

**Listen** (chat sheet): waits for signal, records N seconds of the chain's input *and* output
(silence skipped), analyses both off the audio thread, posts the result into the chat and attaches
it to the next message. Captures are kept in `<data dir>/ScriptFX/captures/` (or
`$SCRIPTFX_CAPTURES`), one folder per capture with a subfolder per track:

```
index.json                       every capture, newest first
capture-<unix time>[-n]/
  capture.json                   when, how long, patch, tracks + a one-line digest per stream
  <track slug>/                  e.g. bass-di/, or track-1/ when the host names no track
    analysis.json                { "input": …, "output": … }
    input.wav  output.wav        f32 stereo at the host's rate
```

The last 20 captures are kept (`captures::KEEP`); audio is the expensive part, ~11.5 MB per
30 s stream. The settings sheet shows what the folder holds and clears it on demand
(**Listen · on this computer → Stored captures**). The **analysis data is meant to be fetched, not pasted**: the chat and the LLM see
`analysis::digest()` (one line per stream) and read sections through `captures::read()` — the
query the planned `get_analysis` tool calls. `cargo run --release -p analysis --example analyze
-- file.wav` analyses a WAV from the command line and prints its digest.

**AI** (off until picked in Settings): *Claude Code* (the installed CLI and its login; run without
tools, settings or MCP servers) or *API* (any OpenAI-compatible address; the key goes to the OS
keychain, per address). Settings lists the models the login or key can use. Changes are checked in a copy (loader, panel render, noise through the DSP,
`luau-lsp` if installed), backed up to `<data dir>/ScriptFX/history/`, then loaded; the chat has Undo.
Without the plugin: `cargo run -p llm --example ask -- patches tape-glow "make it darker" [model]`.

**Dev aids** (environment variables):

| Variable | Effect |
|---|---|
| `SCRIPTFX_DEV_PATCH=<folder>` | a fresh instance loads this patch instead of starting empty |
| `SCRIPTFX_DEV_VIEW=chain` | start in the chain view |
| `SCRIPTFX_DEV_SHEET=chat\|controls\|settings\|library\|editor\|entry\|snapshots\|save` | open it at start (`editor`: LFO 1's, in the dock; `save`: the snapshot menu's name field) |
| `SCRIPTFX_DEV_ZOOM_MS=2000` | slow down the chain → block modal zoom (default 280 ms) to look at it |
| `SCRIPTFX_DEV_RESCALE=1.5` | set the UI scale 1 s after opening |
| `SCRIPTFX_DEV_AUTOLISTEN=10` | start a 10 s listen when the editor opens |
| `SCRIPTFX_DEV_STATS=1` | fps, render CPU, GPU time, panel stats |
| `SCRIPTFX_FRAME_US=16667` | the shell's frame interval |
| `SCRIPTFX_TEST_INPUT=file.wav` | float WAV at the host rate, looped as input |
| `SCRIPTFX_CAPTURES=<folder>` | where Listen stores captures (default: the user data dir) |
| `SCRIPTFX_SNAPSHOTS=<folder>` | where snapshots are stored (default: `<data dir>/ScriptFX/snapshots`) |

**Testing the UI on Linux:** run the standalone off-screen on Xvfb (`Xvfb :99`, then
`DISPLAY=:99 WAYLAND_DISPLAY= WGPU_BACKEND=vulkan`); xdotool clicks and drags work there (they don't
reach XWayland windows on the desktop), screenshots with `import -window root`.
On a bare headless box (no desktop session) **`XDG_RUNTIME_DIR` must be set** — without it the
window opens 1×1 and stays empty. Needed: `Xvfb`, `xdotool`, ImageMagick, `x11-utils`, and a
Vulkan driver; Mesa's lavapipe (`libvulkan_lvp.so`) renders this fine in software, no GPU or
window manager required.

**CI/CD** (`.github/workflows/build.yml`, repo `github.com/FlmBus/script-fx`): pushes trigger tests
and, if they pass, builds for Linux x86_64, Windows x86_64 and macOS universal: VST3 + CLAP +
standalone, **diagnostics** on (`--features diagnostics`: a log file, see `crates/plugin/src/diag.rs`),
debug symbols, test patches and per-platform helper scripts (`.github/package/<platform>/`:
install, run the standalone, collect a report zip). Branch pushes: workflow artifacts. **Push to
`main`: the rolling pre-release
[nightly](https://github.com/FlmBus/script-fx/releases/tag/nightly)** (no login needed to
download). Tags `v*`: a release. macOS builds are ad-hoc signed (not notarized, the install script
removes the quarantine); Windows builds aren't code-signed.

## Patch model

Patches are **Luau** (typed Lua; mlua's `luau` feature). A patch folder is a Luau module: its
`init.luau` (or `init.lua`) builds the patch with a small API and returns it. It `require`s its
other files (only inside the patch folder; from `init.luau` they're `"@self/ui"`, Luau's rule for
init files, elsewhere `"./x"`) and passes the real objects into the UI. Constructors take the identity positionally
(id, display name) and every setting by name in an options table:

```lua
local ui = require("@self/ui")

-- Blocks own their knobs (params): `tape.params.amount`, `tone.params.freq`. Settings given as
-- numbers are the knobs' start values.
local tape = Blocks.Drive.new("tape", { amount = 6 })
local tone = Blocks.LowPass.new("tone", { freq = 8000 })
local comp = Blocks.FETCompressor.new("comp", { threshold = -18 })   -- metered: the UI can show it
local chain = Chain.new({ tape, tone, comp })
chain:add(Blocks.Gain.new("out", { db = -3 }))   -- it's code: loops and helper functions work

-- Host-automatable (max 16), name shows up in the DAW; drives 1..n params.
local character = Macro.new("character", "Character", { default = 0.3 })
  :drives(tape.params.amount, { from = 0,     to = 18 })
  :drives(tone.params.freq,   { from = 16000, to = 2500, curve = "exp" })

return Patch.new("Tape Glow", {
  chain = chain, macros = { character },
  theme = { colors = { accent = "#ff8a3d", bg = "#17141c" }, fonts = { title = Font.new("fonts/Title.ttf") } },
  panel = ui.Panel.new({ character = character, drive = tape.params.amount, gr = comp }),
})
```
**Theme** = the patch's design tokens, shared by the library card and the ControlPanel (`p.theme`):
`colors` (`"#rrggbb"`), `fonts` (`Font.new(path)`: .ttf/.otf, .svg SVG font, .fnt BMFont image font)
and `images` (`Image.new(path)`: .svg/.png) with the patch's own names. The shell uses
`colors.accent/bg/ink`, `fonts.title` and `images.logo` (shown instead of the title); `library = { accent,
bg, ink, font, logo }` overrides them for the library card only. Every part of `Patch.new` is a value,
so it can live in its own file: `theme = require("./theme")`.

```lua
-- ui/init.luau (components in ui/*.luau): components get the objects as typed props
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

- **Params are block knobs**, objects, not strings: macros and UI components reference them
  directly (`tape.params.amount`). Every param has a base value of its own (editable, saved with
  the project).
- **Modulation works like Serum's** ([`docs/research/serum-modulation.md`](docs/research/serum-modulation.md)):
  each macro → param link is a slot with an amount (−100…+100 % of the knob's travel), uni/bi,
  curve and bypass (the amount's sign is the direction; bipolar swings ±amount/2). The slots' deltas add up on the knob's base in normalized space, are
  clamped, and only then go through the knob's lin/log scale. Several macros may drive one knob.
  `drives(param, { from, to })` sets the knob's base to `from` and the amount to the way from
  `from` to `to`; in the shell, each slot gets a meta knob (amount, menu) and the knob shows the
  range arc and a live dot. Slot edits are saved per DAW project.
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
  factors multiply, max 8×). Nonlinear blocks (`BlockKind::is_nonlinear`: the distortion family) default to
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
  ControlPanel is 840×560 logical units (`UI.WIDTH/HEIGHT`), scaled by the shell. A patch without
  one shows its macros as plain knobs in its theme (the shell's Surface view). Planned: this
  becomes the default panel, and panels get renamed to surfaces.
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
