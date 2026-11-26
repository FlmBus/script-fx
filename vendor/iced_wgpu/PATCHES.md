# Local patches to iced_wgpu

Vendored from crates.io: `iced_wgpu` 0.14.0 (MIT). Applied via `[patch.crates-io]`. Changes are
marked `PATCH(scriptfx)`.

## 1. Frame timing and render diagnostics (`src/frame_stats.rs`, `src/lib.rs`, `src/window/compositor.rs`, `src/image/atlas.rs`)

- The device requests `TIMESTAMP_QUERY` + `TIMESTAMP_QUERY_INSIDE_ENCODERS` when the adapter has
  them (iced requests no optional features).
- Each frame is wrapped in two GPU timestamps (read back a few frames late, no stall); CPU time of
  `Renderer::present` and the present interval are measured too.
- `iced_wgpu::frame_stats::get()` returns them smoothed (dev readout `SCRIPTFX_DEV_STATS=1`). The
  per-patch GPU budget of the raster UI plan needs GPU timing; unpatched iced can't provide it.
- **Per-frame reporting** (2026-11-26, for `crates/plugin/src/diag.rs`): `frame_stats::on_frame`
  takes a hook that every presented frame is reported to, raw, as a `Frame`: the frame interval,
  `ui` (the app's `view` plus iced's layout and draw, from `mark_frame_start` to the swapchain
  image being asked for), `acquire` (waiting for that image), `render` (`Renderer::present`),
  `swap` (`frame.present()`), the GPU's own time, the images uploaded into the texture atlas and
  their size, how often the atlas grew (a grow copies all of it), the atlas's layers and live
  allocations, whether a post-process effect was on, and the uncaptured wgpu errors so far.
  The frame is reported from `swap_done`, after the image is handed over, so `swap` is included.
- **`frame_stats::phase`**: a marker for what the UI thread is doing (the app sets its own steps;
  the renderer sets `iced layout+draw`, `acquire`, `encode+submit`, `swap buffers`). The plugin's
  watchdog reads it to name the step a hanging frame is stuck in.
- **`frame_stats::gpu_report()`**: adapter, backend, driver, whether the adapter is a software one
  (`DeviceType::Cpu`), surface format, alpha and present mode, antialiasing, timestamp support,
  limits, and the other adapters that were available — one greppable block, built where the
  compositor picks them. iced logs these facts too, but scattered and only at info level.
- **Uncaptured wgpu errors** (`device.on_uncaptured_error`): logged at error level and counted,
  instead of surfacing as a panic somewhere later. A failing texture upload is exactly the kind of
  thing that shows up as garbage on screen on one platform only.

## 2. Post-process pass (`src/postfx.rs`, `src/lib.rs`)

- `iced_wgpu::postfx::set(Some(RadialBlur { center, rect, amount, soft, keep }))` turns on a zoom
  blur and/or a soft (disc) blur for the coming frames (logical coordinates); `set(None)` turns it
  off. The shell uses it for the chain ↔ block modal transition (zoom blur while it moves) and
  behind the open modal (soft blur).
- While an effect is set, `Renderer::present` draws the frame into an offscreen texture (same
  size and format as the surface, made on first use, remade on resize) and from there through a
  fullscreen WGSL pass into the surface: inside `rect`, 24 samples towards `center`, each spread
  on a golden-angle spiral over a disc of radius `soft` (clamped into `rect`, so nothing from
  outside bleeds in); up to eight `keep` areas (UI over the blurred content) and everything outside
  `rect` pass through.
- Without an effect, `present` is unchanged (no offscreen pass).
- **Backdrop** (2026-11-25): an app marks a layer as the backdrop (`postfx::mark_backdrop_start`
  right before `with_layer`, `mark_backdrop_end` right before it ends; `start_layer`/`end_layer`
  record the layer range, `reset` clears it). With a backdrop, `render` switches targets after its
  last layer: everything so far is blurred into the frame, the remaining layers are drawn on top
  (sharp, any shape). Layers aren't merged in such a frame (a merge could move content across the
  split). Without one, the whole frame goes through the blur and `keep` as before.
- Why here: iced can't render a widget subtree to a texture, and the baseview shell doesn't
  support window screenshots, so a full-frame pass in the renderer is the only place for it.
