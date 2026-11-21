#!/bin/sh
# Packs ScriptFX's logs and system info into ~/ScriptFX-report-<time>.tar.gz.
stamp=$(date +%Y%m%d-%H%M%S); dir=$(mktemp -d)/ScriptFX-report-$stamp; mkdir -p "$dir"
cp -r ~/.local/share/ScriptFX/logs "$dir/logs" 2>/dev/null
{ uname -a; cat /etc/os-release; lspci 2>/dev/null | grep -iE "vga|3d|audio"; } > "$dir/system.txt" 2>&1
coredumpctl list 2>/dev/null | grep -iE "scriptfx|reaper|carla|bitwig" > "$dir/coredumps.txt"
tar czf ~/ScriptFX-report-$stamp.tar.gz -C "$(dirname "$dir")" "$(basename "$dir")"
echo "Report: ~/ScriptFX-report-$stamp.tar.gz"
