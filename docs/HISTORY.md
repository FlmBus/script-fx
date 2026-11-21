# ScriptFX: history

_What earlier sessions built and found, newest first. Moved here from STATUS "Where we left off"
(2026-11-20) so STATUS stays about the current state; nothing was shortened. Later work may have
changed details (e.g. the panel is now the "Surface" view, macros are Serum-style slots). Measurements
and findings: [`research/findings.md`](research/findings.md)._

## 2026-11-19 · Top bar = title bar + sheet openers; settings & library sheets; panel input

**Top bar = title bar + sheet openers** (2026-11-19, after the polish pass). "ScriptFX"
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

## 2026-11-21 · Modulators, routes, drag & drop, Matrix

Follow, LFO 1–3 and Step as allocation-free DSP (`dsp/src/modulators.rs`), parsed from Luau
(`modulators`, `assignments` in `Patch.new`). One route model for macro slots and assignments
(`patch/src/routes.rs`), drag & drop from the macro rail and the drawer tabs onto block page knobs
(and modulators onto macros), drawer panels per modulator, colour-coded meta knobs, Matrix with
add/remove. Route changes rebuild the graph and `Graph::adopt` hands over the running state.
Found on the way: an unoptimized debug standalone ran Tape Glow at < 1 fps (resvg/tiny-skia in
debug) and looked frozen; the dev profile now optimizes dependencies.

## 2026-11-19/20 · Chain editor

Built the Surface and Chain views, block pages with DSP-based previews and a zoom transition,
Serum-style modulation slots and knob cells, and the macro rail. The Matrix and modulators remain
planned; see [`STATUS.md`](STATUS.md) for the current state and open work.

## 2026-11-19 · Patch isolation, theme restructure, shell UI reboot decision

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
  tinker. Not GPUI (no embedding
  into host windows, owns the event loop, no hook for our GPU passes). Fallback: Slint. Migrate
  nih-plug → **nice-plug** first. Earlier shortlist was Slint + vizia; on
  recheck the Slint bridge is WIP/OpenGL-only and vizia's bridge is now third party (nightly).
  Independent of the patch UI plan (the shell only hosts the compositor's render hook).
  Spike (`spikes/shell-iced`) → **built into the plugin** (see "New shell built" below).

## 2026-11-19 · Polish pass on the shell

**Polish pass on the shell** (2026-11-19, Flemming's list; two sessions, merged).
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

## 2026-11-18 · New shell built (iced on nice-plug)

**New shell built** (iced on nice-plug, design system "ScriptFX Shell", skin Matte + Paper
chat; prototype in `spikes/shell-iced`).
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

## 2026-11-18 · Block catalogue (50 blocks), per-block oversampling

**Block catalogue built** (50 blocks; modulators — LFO/envelope as sources — are next, to be
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

## 2026-11-18 · Patches ported to Luau

**Ported patches to Luau** (decided after a spike, see below). Patches are `init.luau` +
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
- Before that (same session): the Lua API redesign (builder objects, namespaces, separate block
  types, `BandSplit` N-band + `Parallel`, fixed 840×560 panel, old SVG panels removed).
- **Not yet checked in a host:** everything since the locale fix. Headless renders + all tests pass.
- **Open:** shell-pill safe area (with the v7 shell build); unknown *extra* props aren't caught by
  the type checker; luau-lsp in CI (needs an install step in `build.yml`); the LLM loop should run
  `luau-lsp analyze` on generated patches and feed type errors back. Factory install leaves the old
  `patch.lua`/`ui.lua`/`panel.svg` files in the user data dir (ignored, harmless).

## 2026-11-18 · Shell layout and patch UI, as decided then

### Shell layout

Designed in the canvas "ScriptFX Shell Mockup" (page "Shell", skin Matte + Paper chat) and the
design system "ScriptFX Shell" (tokens, components). **Built** (2026-11-18) in iced (entry above). The macro/param sliders live in the controls sheet
(Settings in the notch, at the time).

### Patch UI: reactive Lua components (decided 2026-11-18, prototype built)

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
- Measurements of the component engine (stress test, Junkyard fire test, authoring rules, the locale
  bug): [`research/findings.md`](research/findings.md).
