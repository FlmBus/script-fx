#!/bin/bash
# Renders the editor a few times in a row, each run with one suspect switched off, and writes a
# log per run. Nothing to click: every run ends itself. Afterwards run "3 Collect report".
#
# What the logs answer (see crates/plugin/src/diag.rs):
#   gpu:     which adapter and backend, and whether it is a software one
#   frame #  where a frame's time went: view+iced / acquire / encode / swap / idle, and the GPU's
#   STALL    the step a frame was stuck in, logged while it hangs
#   frames · percentiles per 5 s, images uploaded per frame, atlas growth, wgpu errors
cd "$(dirname "$0")" || exit 1
xattr -cr scriptfx test-patches 2>/dev/null
mkdir -p ~/Library/Logs/ScriptFX

SECONDS_PER_RUN=${SECONDS_PER_RUN:-15}
out=~/Library/Logs/ScriptFX/render-check-$(date +%Y%m%d-%H%M%S).txt
echo "Render check: $SECONDS_PER_RUN s per run. Output: $out"

run() {
  name=$1
  shift
  echo "--- run: $name ($*)" | tee -a "$out"
  env SCRIPTFX_DIAG_RUN="$name" \
      SCRIPTFX_DIAG_EXIT_AFTER="$SECONDS_PER_RUN" \
      SCRIPTFX_PATCHES="$PWD/test-patches" \
      RUST_BACKTRACE=full \
      "$@" ./scriptfx -b dummy >>"$out" 2>&1
  echo "    exit code: $?" | tee -a "$out"
}

# 1. As it is, every frame logged: the baseline to compare against.
run baseline SCRIPTFX_DIAG_FRAMES=all
# 2. Animation frozen: the patch's layers stop being re-rasterised and re-uploaded every frame.
run still SCRIPTFX_DIAG_STILL=1
# 3. No patch art at all: nothing big goes through the texture atlas.
run no-patch-layers SCRIPTFX_DIAG_NO_PATCH_LAYERS=1
# 4. No icons: the small images that share that atlas (the "pink squares").
run no-icons SCRIPTFX_DIAG_NO_ICONS=1
# 5. No post-process pass (the modal's blur).
run no-postfx SCRIPTFX_DIAG_NO_POSTFX=1
# 6. No multisampling (iced defaults to MSAA x4 and the shell is nearly all canvases).
run no-msaa SCRIPTFX_DIAG_NO_MSAA=1
# 7. Everything off at once: if this is still slow, the cost is not in our drawing.
run bare SCRIPTFX_DIAG_STILL=1 SCRIPTFX_DIAG_NO_PATCH_LAYERS=1 SCRIPTFX_DIAG_NO_ICONS=1 \
    SCRIPTFX_DIAG_NO_POSTFX=1 SCRIPTFX_DIAG_NO_MSAA=1
# 8. Present without waiting for the display, in case the swap is what blocks.
run present-immediate ICED_PRESENT_MODE=immediate

echo
echo "Done. The logs are in ~/Library/Logs/ScriptFX/ — now run \"3 Collect report\"."
grep -h "frames ·" ~/Library/Logs/ScriptFX/*.log 2>/dev/null | tail -20
read -n 1 -s -r -p "Press any key to close."
