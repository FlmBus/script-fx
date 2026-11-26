# Rendering: performance, platform problems, and how to see them

_Started 2026-11-26, after the first Windows and macOS runs of the installed packages._

## The reports

Flemming, 2026-11-26, from the packaged builds (same machine as the Linux dev box, dual boot, so
the hardware is identical across Linux and Windows):

- **Windows:** stuttering, "as if it isn't hardware-accelerated". Not terrible in itself, but
  **input breaks while frames are dropped**: turning a knob makes it jump to min or max (whichever
  way the drag went), and sometimes the mouse release doesn't register at all — the knob keeps
  turning until you click somewhere.
- **macOS:** ~10–15 **seconds** per frame. The first frame also took a while, and there were pink
  squares where the icons should be (not looked at closely). Between frames the window doesn't
  respond.

Three problems, in Flemming's words: (1) neither platform seems hardware-accelerated, (2) macOS
draws wrongly, (3) we handle slow frames badly — input gets mangled by dropped frames.

## What the code says (static analysis, 2026-11-26)

### Problem 3 is ours, and it is understood

iced_baseview queues baseview's events and hands a frame **the whole batch** that piled up since
the last one (`iced_baseview-0.6.0/src/shell/mod.rs:476-493`). Two consequences:

1. **The pointer lock multiplies the drag.** `shell/pointer_lock.rs` assumes each `CursorMoved` is
   processed right after it happens: it measures the travel, warps the pointer back to the grab
   point and sets `last_y = anchor_y`. Replayed from a batch, every queued move is measured against
   the anchor instead of against the previous event, so each contributes the *whole* travel. N
   queued moves move the value ≈N× too far — the knob slams into min or max in the direction of the
   drag. Exactly the reported symptom, and invisible on Linux because frames are paced at 15 ms.
2. **A press or release can be mis-targeted.** The batch is evaluated with a *single* cursor
   position, the newest one (`state.update_cursor_position` runs when an event arrives, line 813;
   `ui.update(…, window.state.cursor())` at line 493). iced's button events carry no position, so a
   release that arrives in a batch is tested against wherever the pointer is *now*
   (e.g. `knob.rs:384`). Move far enough during a stall and the release is swallowed: the knob's
   `state.drag` is never cleared. `hide_cursor` has a window-wide release hook, but it only un-hides
   the cursor; it doesn't end a widget's drag.

### Problem 1 is probably not the GPU

- Adapter choice looks right: `Backends::all()` + `PowerPreference::HighPerformance`
  (`vendor/iced_wgpu/src/window/compositor.rs`), and a software adapter is only taken if it is the
  only one (`force_fallback_adapter: false`).
- vsync is already off: iced_baseview forces `vsync = false` ("plugins should never use vsync as it
  blocks the host's main thread"), so `present` isn't blocking the host's message pump.
- What is expensive: `always_redraw(true)` (a full relayout and redraw on every 15 ms frame timer),
  MSAA ×4 (iced's default; the shell is almost all canvases), and — for an animated patch surface —
  a resvg rasterisation plus a **new** `image::Handle` per changed layer per frame
  (`shell/stage.rs`; `Handle::from_rgba` mints a fresh atlas id on every call).

Measured here with `cargo run -p ui-components --example render` (debug, this box, the patch
surface only — no iced layout/draw/upload):

| patch | first frame @1× | drag frame @1× | first frame @2× | drag frame @2× |
|---|---|---|---|---|
| tape-glow | 32 ms | 7.7 ms | 90 ms | 16.4 ms |
| neon-saturator | 115 ms (Background 89 ms) | 6.6 ms | 416 ms (Background 359 ms) | 17.2 ms |

So at Retina 2× there is ~16–17 ms of CPU rasterisation per animated frame before iced does
anything. That is a stutter budget gone on its own, and it is what feeds problem 3.

### Problem 2 is not diagnosable from here

macOS paces frames with a `CADisplayLink` (`vendor/baseview/src/platform/macos/view.rs:197`), so
pacing isn't obviously broken, and 10–15 s per frame is ~100× worse than anything the rasteriser
numbers explain. The pink squares are most likely the **image path**, not the icons themselves:
icons are rasterised with resvg and handed to iced as images (`shell/icons.rs`), sharing the texture
atlas with the patch's layers; garbage texels where an image should be is what a failed or evicted
atlas allocation looks like. The atlas is capped at 2048², grows by recreating the whole texture and
copying every layer, and never shrinks (`vendor/iced_wgpu/src/image/atlas.rs`).

Candidates, in order: a failing/misaligned texture upload on Metal (would now show as a logged wgpu
error); atlas growth per frame from the per-frame handles; the patch surface's CPU rasterisation at
2×; the post-process pass; something in the swap/present path.

**This is why the diagnostics below exist rather than a guess at a fix.**

## The instrumentation (2026-11-26)

`crates/plugin/src/diag.rs` plus the `iced_wgpu` patch (`vendor/iced_wgpu/PATCHES.md` §1). Always
compiled (logged through `nice_log!`, so a plain standalone run shows it on stderr too), with the
file log in diagnostics builds.

**A line per frame**, split so that the parts add up to the frame time:

```
frame #42 · 10420.3 ms (0.1 fps) · view+iced 12.1 · acquire 0.1 · encode 58.0 · swap 10350.2
  · idle 0.0 · gpu 1.20 ms · images 3/7.50 MiB · atlas 4 layers/9 allocs, 1 grows
  · surface 9.80 ms/2 rasters/1.88 Mpx · icon rasters 0 · events 37 (35 moves, 35 warps) · postfx
```

- `view+iced` — our `view` plus iced's layout and draw
- `acquire` — waiting for the swapchain image
- `encode` — `Renderer::present` (prepare, encode, submit)
- `swap` — `frame.present()`: handing the image to the window system
- `idle` — the rest of the interval: waiting for the next frame's trigger
- `gpu` — the GPU's own time for the frame (timestamp queries, a few frames late)
- `images` / `atlas` — what went through the texture atlas, and whether it grew (a grow copies all
  of it)
- `surface` — the patch's components: resvg time, layers redrawn, pixels rasterised
- `events` — the input events this frame's batch carried (see problem 3: this number *is* the
  input lag), and the pointer warps the knob drag asked for

Logged for the first 20 frames, for every frame slower than `SCRIPTFX_SLOW_FRAME_MS` (rate-limited),
and as percentiles every 5 s. `SCRIPTFX_DIAG_FRAMES=all|off` overrides.

**A stall watchdog.** `frame_stats::phase` marks what the UI thread is doing; a frame unfinished
after `SCRIPTFX_STALL_MS` (default 1 s) is logged *while it hangs*:

```
STALL · in "swap buffers" for 476 ms · no frame for 476 ms · editor: 0 fps, …
STALL over · frame #2 finished after 801 ms stuck in "swap buffers" · frame #2 · 1323.0 ms …
```

That turns "it freezes for 10 seconds" into "it spends 10 seconds in \<step\>" without a debugger.

**The GPU report**, logged with the first frame — the answer to "is this hardware-accelerated":

```
gpu: llvmpipe (LLVM 19.1.7, 256 bits) (Cpu)
gpu: backend Vulkan · vendor 0x10005 device 0x0000 · driver llvmpipe Mesa 25.0.7
gpu: software adapter: true (Cpu = no hardware acceleration)
gpu: surface Bgra8Unorm · alpha PreMultiplied · present AutoNoVsync · antialiasing Some(MSAAx4)
gpu: timestamp queries: true · max texture 2d 16384 · max buffer 2048 MiB
gpu: available: … (one line per adapter we could have taken)
```

**Uncaptured wgpu errors** are logged and counted (`device.on_uncaptured_error`) — a failing texture
upload is exactly what "pink squares on one platform" looks like.

**Bisect switches**, so a tester without a build can take one suspect out at a time:
`SCRIPTFX_DIAG_STILL`, `_NO_PATCH_LAYERS`, `_NO_ICONS`, `_NO_POSTFX`, `_NO_MSAA`, plus wgpu's own
`WGPU_BACKEND` / `ICED_PRESENT_MODE`. `SCRIPTFX_DIAG_EXIT_AFTER` makes the standalone end itself, so
the shipped `render-check` script (`.github/package/<platform>/`) runs the whole matrix unattended,
~15 s per run, and logs each.

### First numbers from the harness (Linux, Xvfb + lavapipe, tape-glow, debug build)

Software rendering, so the absolute numbers mean nothing — but the harness discriminates:

| run | fps | surface raster p50 | images/frame | gpu p50 |
|---|---|---|---|---|
| baseline | 5.2 | 4.96 ms | 3.0 | 40.7 ms |
| `SCRIPTFX_DIAG_STILL=1` | 16.3 | 0.10 ms | 0.4 | 38.8 ms |
| `SCRIPTFX_DIAG_NO_PATCH_LAYERS=1` | 6.0 | 4.93 ms | 0.5 | 37.6 ms |

The first frame uploads 29 images / 2.41 MiB (18 of them icons) and costs 55 ms to encode; steady
state re-uploads tape-glow's 2 animated layers every frame. Icons are rasterised once, as intended.

## Problem 3: fixed (2026-11-26)

`shell/pointer_lock.rs`:

- the travel is measured **between consecutive events**, never against the grab point;
- the pointer is only put back when it has wandered more than `PIN_AT` (120 px) from the grab
  point — rare, because every warp costs an event that has to be recognised again — and `Drag`'s
  `Drop` puts the cursor back where it was grabbed, so the Serum-like feel survives;
- the motion event a warp produces is recognised within `PIN_EPS` and swallowed;
- one event carries at most `MAX_STEP` (300 px) of travel, as a backstop;
- left-button releases are counted window-wide (`hide_cursor` sees them all), so `Drag::ended()`
  tells a control that the button went up even when its own release got swallowed. The knob cells,
  the meta knobs and the patch surface check it on the next pointer move and end the drag **and the
  host gesture** (which the lost release had left open).

**Reproduced and verified here**, with the new `SCRIPTFX_DIAG_SLOW_MS=200` (sleep 200 ms per frame
— a stuttering machine on demand) driving the standalone under Xvfb with xdotool: the same 40 px
drag on Clean Gain's M1 macro, read off the patch's own dB display.

| | old code | fixed |
|---|---|---|
| 60 fps | 7.2 dB | 7.2 dB |
| 200 ms per frame | **12.0 dB (slammed to the rail)** | 7.2 dB |

Unit tests in `pointer_lock.rs` cover the batch, the warp's own event, the cap and the missed
release; four of the six fail against the old algorithm (checked by reverting it).

### Still open in the input path

iced evaluates a whole batch with a **single cursor position** — the newest one — and its button
events carry no position of their own. So a press or release that arrives in a batch is tested
against wherever the pointer is *now*, and can land on the wrong widget or be dropped. That is
upstream behaviour (iced_winit does the same) and the fix is to vendor iced_baseview and feed
`ui.update` sub-batches with the cursor each event actually had. Before doing that, the frame log's
`events N (M moves)` field will say how often batches on Windows even contain a button event —
measure first.

## Minimum hardware, and the levers (2026-11-26)

**The floor is the graphics API, not speed.** wgpu needs DX12 or Vulkan 1.0 (Windows), Metal
(macOS), Vulkan (Linux) — GPUs from roughly 2013 (Intel HD 4400 / Kepler / GCN), Macs from 2012.
Without a working driver wgpu takes a *software* adapter, which the GPU report now names
(`software adapter: true`). That case is measurable: on llvmpipe here, an animated patch ran at
8.5 fps with multisampling and 21 fps without.

**The audio engine is not the constraint**: 0.7 % of one core for an 11-block chain (release,
`findings.md`), so twenty instances are ~15 % of a core. **The editor is**, and most of it is our
own CPU rasterisation of the patch's components — 17.6 ms per frame at 2× for an animated
full-size background, before iced and the upload.

### What was built

- **Frame rate limiter** (60 / 30 / 20) and **Graphics** levels (High / Medium / Low) in the
  settings sheet, per user (`user_settings`, `shell/quality.rs`).
- **The editor's clock is now its only redraw trigger.** iced_baseview's `always_redraw` is off, so
  it no longer redraws on every 15 ms baseview frame regardless of change — the limiter limits.
- **Idle throttle**: when nothing that drives the picture has changed for 900 ms and no message has
  arrived, the clock drops to 10 fps; coming back out of idle unparks the clock, so there is no
  added latency. A case we fail to notice runs at 10 fps rather than stopping.
- **The patch surface is only updated while it is the surface on screen** — on the chain or the
  Matrix its components were still being rasterised and uploaded every frame, invisibly.

Measured here (debug build, llvmpipe, tape-glow, 836×714 — the absolute numbers are software
rendering, the differences are the levers):

| | fps | GPU p50 | patch raster p50 | images/frame |
|---|---|---|---|---|
| High @60 | 8.6 | 41.5 ms | 4.90 ms | 2.0 |
| Medium @60 (no MSAA) | 22.3 | 13.1 ms | 4.97 ms | 2.0 |
| Low @30 (no MSAA/blur, still) | 28.6 | 13.8 ms | 0.18 ms | 0.0 |
| nothing moving (idle throttle) | **10.0** | – | – | – |

### What idling does and doesn't cover

The idle check watches the values a frame draws from (meters, modulators, params, the patch's
re-renders, the sheet slides, Listen's progress) plus every message that isn't the frame tick.
**While the host calls `process`, the modulators keep running**, so the mod dock's displays really
are animating and the editor stays at the full rate — which is correct, but it means idling mostly
helps when the transport is stopped or the plugin is suspended. If that turns out to matter, the
refinement is to ask what is actually *visible* rather than what changed.

## Next

1. Run the `render-check` matrix on Windows and on the Mac; read the `gpu:` block, the frame lines
   and the stalls. That decides problems 1 and 2 instead of guessing.
2. Then decide whether the batched-cursor problem above is worth vendoring iced_baseview for.
3. Automatic fallback: the frame reporting and the quality levels are both there now, so "this
   machine can't hold the budget → drop a level and say so" is a small step (STATUS backlog).
