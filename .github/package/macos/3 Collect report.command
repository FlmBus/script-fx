#!/bin/bash
# Packs everything needed to debug ScriptFX into one zip on the Desktop.
stamp=$(date +%Y%m%d-%H%M%S)
dir=$(mktemp -d)/ScriptFX-report-$stamp
mkdir -p "$dir/crash-reports"
# Our logs (plugin + standalone).
cp -R ~/Library/Logs/ScriptFX "$dir/logs" 2>/dev/null
# macOS crash reports of the last 3 days that mention ScriptFX or come from the hosts we use.
find ~/Library/Logs/DiagnosticReports /Library/Logs/DiagnosticReports -type f -mtime -3 2>/dev/null | while read -r f; do
  if grep -qiE 'scriptfx|Studio One|Fender|REAPER' "$f" 2>/dev/null || [[ "$f" =~ [Ss]cript[Ff][Xx]|Studio|REAPER ]]; then
    cp "$f" "$dir/crash-reports/"
  fi
done
# System facts.
{
  sw_vers
  uname -a
  sysctl -n machdep.cpu.brand_string
  system_profiler SPHardwareDataType SPDisplaysDataType SPAudioDataType 2>/dev/null
  echo "--- plug-ins"
  ls -la ~/Library/Audio/Plug-Ins/VST3 ~/Library/Audio/Plug-Ins/CLAP
  echo "--- patches"
  ls -la ~/Library/Application\ Support/ScriptFX/patches
  echo "--- quarantine flags"
  xattr -lr ~/Library/Audio/Plug-Ins/VST3/ScriptFX.vst3 2>/dev/null | head -20
} > "$dir/system.txt" 2>&1
# Recent system log lines about our plug-in (code signing, loading).
log show --last 3h --style compact --predicate 'eventMessage CONTAINS[c] "scriptfx"' > "$dir/system-log.txt" 2>/dev/null
cd "$(dirname "$dir")" && zip -qr ~/Desktop/ScriptFX-report-$stamp.zip "$(basename "$dir")"
echo "Report: ~/Desktop/ScriptFX-report-$stamp.zip"
open -R ~/Desktop/ScriptFX-report-$stamp.zip
read -n 1 -s -r -p "Press any key to close."
