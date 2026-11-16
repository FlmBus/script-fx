ScriptFX PoC (macOS universal: Apple Silicon + Intel)

Install:
  cp -R ScriptFX.vst3 ~/Library/Audio/Plug-Ins/VST3/
  cp -R ScriptFX.clap ~/Library/Audio/Plug-Ins/CLAP/

The bundles are only ad-hoc signed (not notarized). Remove the download quarantine first:
  xattr -dr com.apple.quarantine ~/Library/Audio/Plug-Ins/VST3/ScriptFX.vst3 ~/Library/Audio/Plug-Ins/CLAP/ScriptFX.clap

Factory patches are installed on first run to ~/Library/Application Support/ScriptFX/patches
(override with SCRIPTFX_PATCHES=/path/to/patches).
