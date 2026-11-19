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
