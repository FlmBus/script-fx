#!/bin/bash
# Installs the ScriptFX diagnostics build (VST3 + CLAP) and the test patches.
cd "$(dirname "$0")" || exit 1
set -x
xattr -cr ScriptFX.vst3 ScriptFX.clap scriptfx test-patches 2>/dev/null
mkdir -p ~/Library/Audio/Plug-Ins/VST3 ~/Library/Audio/Plug-Ins/CLAP
rm -rf ~/Library/Audio/Plug-Ins/VST3/ScriptFX.vst3 ~/Library/Audio/Plug-Ins/CLAP/ScriptFX.clap
cp -R ScriptFX.vst3 ~/Library/Audio/Plug-Ins/VST3/
cp -R ScriptFX.clap ~/Library/Audio/Plug-Ins/CLAP/
# Test patches next to the factory patches (those are installed on the first start).
mkdir -p ~/Library/Application\ Support/ScriptFX/patches
cp -R test-patches/* ~/Library/Application\ Support/ScriptFX/patches/
set +x
echo
echo "Installed. Restart Studio One (or rescan plug-ins), then add ScriptFX PoC to a track."
echo "Logs go to ~/Library/Logs/ScriptFX/"
read -n 1 -s -r -p "Press any key to close."
