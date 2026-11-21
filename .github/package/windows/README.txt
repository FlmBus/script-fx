ScriptFX (Windows x86_64), diagnostics build
=============================================

An optimized build with debug symbols that writes a detailed log
(%LOCALAPPDATA%\ScriptFX\logs\): system info, GPU, audio setup, patch loads
and errors, panics with backtraces, and every 10 s the audio callback timing
(dropouts) and the editor's frame rate.

1. install.bat: installs ScriptFX.vst3 / .clap (asks for admin rights) and the
   test patches. The build isn't code-signed: SmartScreen may warn
   ("More info" -> "Run anyway").
2. Try it: run-standalone.bat first, then in the DAW (Fender Studio): rescan
   plug-ins, add "ScriptFX PoC" to a track.
3. Afterwards (also if all is fine): right-click collect-report.ps1 ->
   "Run with PowerShell". It puts ScriptFX-report-<time>.zip on the Desktop.
   Bring it back, with a few words on what you did and what happened.

symbols\ holds the .pdb files for decoding crash dumps (not needed to run).
