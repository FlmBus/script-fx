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
