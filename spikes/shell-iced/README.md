# Shell spike (iced)

Throwaway. The v7 shell in the "ScriptFX Shell" design system (skin Matte + Paper chat), built
with iced on nice-plug, to decide the shell toolkit. Mock data only; the audio path is a
passthrough with a peak meter. The iced shell was built into the plugin; see
[`docs/HISTORY.md`](../../docs/HISTORY.md) for the decision and results.

Not part of the main workspace (own `[workspace]`), so it doesn't slow down the main build.

```sh
export CARGO_TARGET_DIR=/tmp/scriptfx-target/spike
cargo build --release

# standalone (no audio needed)
ICED_PRESENT_MODE=fifo /tmp/scriptfx-target/spike/release/shell_spike -b dummy
# start states for screenshots: SPIKE_STATE=empty | nochat | neon
# SPIKE_DEMO=1: the chat slides in 2 s after start (to check the animation without clicking)

# VST3 / CLAP for hosts
B=~/.vst3/ScriptFXShellSpike.vst3/Contents/x86_64-linux; mkdir -p $B
cp /tmp/scriptfx-target/spike/release/libshell_spike.so $B/ScriptFXShellSpike.so
cp /tmp/scriptfx-target/spike/release/libshell_spike.so ~/.clap/ScriptFXShellSpike.clap
ICED_PRESENT_MODE=fifo reaper            # or: carla-single vst3 ~/.vst3/ScriptFXShellSpike.vst3
```

`ICED_PRESENT_MODE=fifo` matters: without it the editor renders ~1200 fps under XWayland
(see the doc, "Frame pacing").

Layout: `src/shell/mod.rs` routes messages; one module per section (`library`, `notch`, `stage`,
`chat`, `signal_line`, `empty`); `theme.rs` holds the design tokens and shared styles;
`stage.wgsl` is the GPU layer; `vendor/iced_wgpu` is iced_wgpu 0.14.0 patched for frame timing
(`src/frame_stats.rs`, changes marked `PATCH(scriptfx)`); `slide.rs` is a small custom widget (slides + clips the chat sheet).
