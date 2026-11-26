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
3. If the window is slow, stutters or input jumps (a knob flying to min/max):
   right-click render-check.ps1 -> "Run with PowerShell". It renders the
   editor ten times in a row, ~15 s each, every time with one suspect
   switched off (animation, the patch's art, the icons, the blur pass,
   multisampling, the graphics backend, the present mode), and logs each run.
   Nothing to click: every run closes itself.
4. Afterwards (also if all is fine): right-click collect-report.ps1 ->
   "Run with PowerShell". It puts ScriptFX-report-<time>.zip on the Desktop.
   Bring it back, with a few words on what you did and what happened.

symbols\ holds the .pdb files for decoding crash dumps (not needed to run).
