# Local patches to baseview

Vendored from https://github.com/RustAudio/baseview at `9a0b42c` (MIT OR Apache-2.0), the
revision egui-baseview uses. Applied workspace-wide via `[patch]` in the root `Cargo.toml`
(this also replaces the `579130e` revision nih-plug's standalone wrapper asks for).
Examples, dev-dependencies and the `[workspace]` section were dropped. Changes are marked `PATCH(scriptfx)`.

## 1. No panics on GLX errors (`src/gl/x11.rs`)

`make_current`, `make_not_current` and `swap_buffers` used to `unwrap()` X errors. The panic
kills the editor's window thread, which leaves a blank plugin window. Seen with Reaper on
Linux (NVIDIA proprietary driver, Hyprland/XWayland):

- `GLXBadDrawable` when the editor is closed and reopened (the host destroys the parent
  window while a frame is being drawn)
- `GLXBadContext` in the standalone when the WM tiles/resizes the window

Now the error is logged once per call site to stderr and the frame is skipped.
