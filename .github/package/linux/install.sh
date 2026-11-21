#!/bin/sh
# Installs ScriptFX (VST3 + CLAP) for this user, and the test patches.
cd "$(dirname "$0")" || exit 1
mkdir -p ~/.vst3 ~/.clap ~/.local/share/ScriptFX/patches
rm -rf ~/.vst3/ScriptFX.vst3 ~/.clap/ScriptFX.clap
cp -r ScriptFX.vst3 ~/.vst3/ && cp ScriptFX.clap ~/.clap/
cp -r test-patches/* ~/.local/share/ScriptFX/patches/
echo "Installed to ~/.vst3 and ~/.clap. Logs: ~/.local/share/ScriptFX/logs/"
