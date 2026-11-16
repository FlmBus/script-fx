# ScriptFX sandbox

PoCs for a plugin whose effect presets are Lua scripts: Lua *describes* macros, theme and
UI declaratively, and native Rust does the DSP and rendering.

```
crates/preset   preset model (params + macros), sandboxed Lua loader, animate() hook
crates/ui-svg   PoC #1: preset panel via resvg/usvg ("patch & reparse")
crates/plugin   package `scriptfx`: nih-plug plugin (VST3/CLAP/standalone), egui chrome + preset panel + mock DSP
crates/xtask    `cargo xtask bundle` (nih_plug_xtask)
vendor/nih-plug patched nih-plug (runtime parameter names), see vendor/nih-plug/PATCHES.md
presets/        tape-glow (multi-target macro, free param, animate()), clean-gain (pure declarative)
presets/.types  LuaLS stubs → autocomplete in presets; doubles as API spec for an LLM
```

## Build & run

```sh
export CARGO_TARGET_DIR=/tmp/scriptfx-target     # /home was full; tmpfs build dir
cargo test -p preset -p ui-svg
cargo xtask bundle scriptfx --release            # → $CARGO_TARGET_DIR/bundled/ScriptFX.{vst3,clap}

# install for local hosts (Reaper, Carla ≥ 2.6 for CLAP; Carla 2.5 = VST3 only)
cp -r $CARGO_TARGET_DIR/bundled/ScriptFX.vst3 ~/.vst3/ && cp $CARGO_TARGET_DIR/bundled/ScriptFX.clap ~/.clap/

# standalone (see "Linux gotchas" for the env vars)
SCRIPTFX_PRESETS=$PWD/presets __GLX_VENDOR_LIBRARY_NAME=mesa LIBGL_ALWAYS_SOFTWARE=1 \
  $CARGO_TARGET_DIR/release/scriptfx -b alsa

# headless render + benchmark of a preset
cargo run --release -p ui-svg --example render -- presets/tape-glow /tmp/out.png
```

**Presets location:** `$SCRIPTFX_PRESETS`, else the user data dir (`~/.local/share/ScriptFX/presets`,
`~/Library/Application Support/ScriptFX/presets`, `%APPDATA%\ScriptFX\presets`). The bundled
presets are compiled in and copied there on first run; existing files are never overwritten.
For preset development, point `SCRIPTFX_PRESETS` at this repo's `presets/`. Hot reload picks up
edits, errors show in the footer, and the last good preset keeps running.

**CI** (`.github/workflows/build.yml`): tests, then VST3 + CLAP bundles for Linux x86_64,
Windows x86_64 and macOS universal (arm64 + x86_64, ad-hoc signed, not notarized → remove
quarantine, see the README in the artifact).

## Preset model

```lua
return {
  name = "Tape Glow",
  params = {   -- internal controls, unlimited, saved with the project
    { id = "drive", name = "Drive", min = 0, max = 24, unit = " dB", dsp = "drive" },
    { id = "tone",  name = "Tone",  min = 500, max = 20000, unit = " Hz", dsp = "tone" },
    { id = "wow",   name = "Wow",   min = 0, max = 100, unit = "%" },          -- UI-only
  },
  macros = {   -- host-automatable (max 16), name shows up in the DAW
    { id = "character", name = "Character", default = 0.3, targets = {
        { param = "drive", from = 0, to = 18 },
        { param = "tone",  from = 16000, to = 2500, curve = "exp" },
    } },
  },
  theme  = { accent = "#ff8a3d", ... },          -- → CSS classes .fill-accent/.stroke-accent/.stop-accent + egui chrome
  ui = {
    svg = "panel.svg",
    bindings = {
      rotate("drive_pointer", { source = "param.drive", from = -135, to = 135, pivot = "drive_knob" }),
      drag("drive_knob", { target = "macro.character" }),
      text("drive_value", { source = "param.drive" }),
      opacity("glow", { source = "var.pulse", from = 0.05, to = 0.9 }),
    },
    animate = function(s) ... return { pulse = ... } end,   -- optional, UI thread, budgeted
  },
}
```

- A param is either **free** (own value, editable in the UI, persisted) or **driven by exactly one
  macro** (locked). Dragging a driven param's knob moves its macro.
- Host slots stay fixed ("Macro 1..16" IDs); the preset decides their names and meaning.
  Automation stays on the slot across preset changes (same contract as e.g. Serum's macros).
- Sources: `macro.<id>`, `param.<id>`, `meter.in|out`, `time`, `var.<name>` (from `animate`).
- Bindings: `rotate translate scale opacity fill text drag`.

## Findings: resvg/usvg (PoC #1)

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
  restricted to the preset folder. DTDs are disabled for our own parse.
- Lua: only `table/string/math/utf8`, no `load/dofile/require`, a 16 MB memory cap, and an instruction
  budget of 200k per call (an infinite loop in `animate` disables animation instead of hanging the UI).

**Performance: the weak spot.** Release build, 640×360 panel:

| preset | lua animate | patch | usvg parse | resvg render | total @1x | total @2x |
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
also makes rendering differ between machines. Now the plugin bundles Noto Sans (≈2 ms) and presets can ship `fonts/`.

## Integration findings (nih-plug + egui)

- resvg pixmap → `egui::TextureHandle` → painted in the CentralPanel works; egui widgets around it.
- Preset theme also styles the chrome (egui `Visuals`), macro sliders are auto-generated with
  preset units, and `animate()` vars are visible for debugging.
- **Host params are fixed**: 16 slots; presets map their macros onto them in order. DAWs
  don't tolerate parameter lists that change at runtime, but renaming is fine (automation is
  keyed by ID). The vendored nih-plug is patched to push names (VST3 `kParamTitlesChanged`,
  CLAP `RESCAN_INFO`). **Needs testing per host** (Reaper, Carla, Studio One 6, Fender Studio).
- **egui gotcha**: `Slider::from_get_set` calls the setter *every frame* (clamping). Without a
  change check this spams host automation gestures and overwrote preset defaults.
- The preset name is persisted with the host project. Routing (macro → DSP) is applied in
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

- [ ] Verify host name updates: Reaper (VST3 + CLAP), Carla, Studio One 6 (mac), Fender Studio (win)
- [ ] Static/dynamic layer caching in `ui-svg` (or try vello) and re-measure @2x
- [ ] PoC #2: same `preset` crate → egui-native panel (JSON theme), compare authoring DX
- [ ] PoC #3 (optional): femtovg
- [ ] Investigate baseview GLX on NVIDIA
