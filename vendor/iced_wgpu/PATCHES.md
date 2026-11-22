# Local patches to iced_wgpu

Vendored from crates.io: `iced_wgpu` 0.14.0 (MIT). Applied via `[patch.crates-io]`. Changes are
marked `PATCH(scriptfx)`.

## 1. Frame timing (`src/frame_stats.rs`, `src/lib.rs`, `src/window/compositor.rs`)

- The device requests `TIMESTAMP_QUERY` + `TIMESTAMP_QUERY_INSIDE_ENCODERS` when the adapter has
  them (iced requests no optional features).
- Each frame is wrapped in two GPU timestamps (read back a few frames late, no stall); CPU time of
  `Renderer::present` and the present interval are measured too.
- `iced_wgpu::frame_stats::get()` returns them (dev readout `SCRIPTFX_DEV_STATS=1`). The per-patch
  GPU budget of the raster UI plan needs GPU timing; unpatched iced can't provide it.

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
- Why here: iced can't render a widget subtree to a texture, and the baseview shell doesn't
  support window screenshots, so a full-frame pass in the renderer is the only place for it.
