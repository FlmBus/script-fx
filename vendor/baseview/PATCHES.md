# Local patches to baseview

Vendored from crates.io: `baseview` 0.4.2 (MIT OR Apache-2.0), the version iced_baseview 0.6 uses.
Applied via `[patch.crates-io]`. Changes are marked `PATCH(scriptfx)`.

## 1. X11: always pace frames with the timer (`src/platform/x11/present.rs`)

baseview paces X11 frames with Present-extension MSC notifications. Under XWayland (NVIDIA,
Hyprland) these never arrive for our windows: with the original path the editor drew at ~5 fps
(only iced_baseview's poll timer drew). baseview's own fallback (a 15 ms frame timer, used when
Present isn't available) works, so it's now always used. `BASEVIEW_X11_PRESENT=1` restores the
Present path for testing.

Related finding (not a patch): iced_baseview's poll timer also triggers a redraw; at its default
interval (1/64 s) it fired ~1000×/s here (1000+ fps, 35–40 % CPU). The shell sets a 250 ms poll
interval (`crates/plugin/src/shell/mod.rs`). Result: ~70 fps, 9–13 % CPU with an animated panel.

## 2. X11: let the app resize a fixed-size window (`src/platform/x11/sizing.rs`)

Found with the shell's UI scale option (window didn't resize; iced already used the new scale, so
the mouse was offset). Two bugs in `resize_from_handler`:
- `SizingStrategy::adjust_size` returns the *current* size for a fixed (non-resizable) window, so
  every resize the app requested was dropped before reaching X11. Fixed means "the user can't
  drag-resize it", not "the app can't resize it": the requested size is now used as is (fixed
  windows have no min/max). `adjust_size` itself is unchanged (hosts asking to resize a fixed
  window still get "no").
- The size hints (min = max = size for a fixed window) were updated *after* the configure
  request, so the window manager would clamp it to the old size. Hints now go first (also in
  `resize_from_host`).
The resulting ConfigureNotify resizes iced and tells the host (`HostCallbacks::request_resize`).
