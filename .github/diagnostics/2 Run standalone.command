#!/bin/bash
# Starts the ScriptFX standalone (CoreAudio) with all output logged.
cd "$(dirname "$0")" || exit 1
xattr -cr scriptfx test-patches 2>/dev/null
mkdir -p ~/Library/Logs/ScriptFX
out=~/Library/Logs/ScriptFX/standalone-$(date +%Y%m%d-%H%M%S).txt
echo "Standalone output: $out"
echo "Available audio devices / options:" | tee -a "$out"
./scriptfx --help 2>&1 | tee -a "$out" | head -40
echo
echo "Starting (close the window to stop). If it doesn't start, try: ./scriptfx -b dummy" | tee -a "$out"
RUST_BACKTRACE=full ./scriptfx -b core-audio 2>&1 | tee -a "$out"
code=${PIPESTATUS[0]}
echo "exit code: $code" | tee -a "$out"
if [ "$code" != "0" ]; then
  echo "Retrying without audio (-b dummy) to see whether the UI works:" | tee -a "$out"
  RUST_BACKTRACE=full ./scriptfx -b dummy 2>&1 | tee -a "$out"
  echo "exit code (dummy): ${PIPESTATUS[0]}" | tee -a "$out"
fi
read -n 1 -s -r -p "Press any key to close."
