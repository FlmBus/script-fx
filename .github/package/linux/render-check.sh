#!/bin/bash
# Renders the editor a few times in a row, each run with one suspect switched off, and writes a
# log per run. Nothing to click: every run ends itself. Afterwards run collect-report.sh.
#
# What the logs answer (see crates/plugin/src/diag.rs):
#   gpu:     which adapter and backend, and whether it is a software one
#   frame #  where a frame's time went: view+iced / acquire / encode / swap / idle, and the GPU's
#   STALL    the step a frame was stuck in, logged while it hangs
#   frames · percentiles per 5 s, images uploaded per frame, atlas growth, wgpu errors
cd "$(dirname "$0")" || exit 1
logs=${XDG_DATA_HOME:-$HOME/.local/share}/ScriptFX/logs
mkdir -p "$logs"
seconds=${SECONDS_PER_RUN:-15}
out=$logs/render-check-$(date +%Y%m%d-%H%M%S).txt
echo "Render check: $seconds s per run. Output: $out"

run() {
  name=$1
  shift
  echo "--- run: $name ($*)" | tee -a "$out"
  env SCRIPTFX_DIAG_RUN="$name" \
      SCRIPTFX_DIAG_EXIT_AFTER="$seconds" \
      SCRIPTFX_PATCHES="$PWD/test-patches" \
      RUST_BACKTRACE=full \
      "$@" ./scriptfx -b dummy >>"$out" 2>&1
  echo "    exit code: $?" | tee -a "$out"
}

run baseline SCRIPTFX_DIAG_FRAMES=all
run still SCRIPTFX_DIAG_STILL=1
run no-patch-layers SCRIPTFX_DIAG_NO_PATCH_LAYERS=1
run no-icons SCRIPTFX_DIAG_NO_ICONS=1
run no-postfx SCRIPTFX_DIAG_NO_POSTFX=1
run no-msaa SCRIPTFX_DIAG_NO_MSAA=1
run bare SCRIPTFX_DIAG_STILL=1 SCRIPTFX_DIAG_NO_PATCH_LAYERS=1 SCRIPTFX_DIAG_NO_ICONS=1 \
    SCRIPTFX_DIAG_NO_POSTFX=1 SCRIPTFX_DIAG_NO_MSAA=1
run backend-vulkan WGPU_BACKEND=vulkan
run backend-gl WGPU_BACKEND=gl
run present-immediate ICED_PRESENT_MODE=immediate

echo
echo "Done. The logs are in $logs — now run collect-report.sh."
grep -h "frames ·" "$logs"/*.log 2>/dev/null | tail -20
