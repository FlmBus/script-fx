ScriptFX PoC (Linux x86_64)

Install:
  cp -r ScriptFX.vst3 ~/.vst3/
  cp ScriptFX.clap ~/.clap/

Presets are installed on first run to ~/.local/share/ScriptFX/presets
(override with SCRIPTFX_PRESETS=/path/to/presets).

NVIDIA proprietary driver: if the editor crashes (GLXBadContext), start the host with
  __GLX_VENDOR_LIBRARY_NAME=mesa LIBGL_ALWAYS_SOFTWARE=1 <host>
