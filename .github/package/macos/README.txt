ScriptFX (macOS universal: Apple Silicon + Intel, 11+), diagnostics build
===================================================================

An optimized build with debug symbols that writes a detailed log
(~/Library/Logs/ScriptFX/): system info, GPU, audio setup, patch loads and
errors, panics with backtraces, and every 10 s the audio callback timing
(dropouts) and the editor's frame rate.

1. Double-click "1 Install plugins.command"
   (installs ScriptFX.vst3 / .clap and the test patches, removes quarantine).
   If macOS refuses to open it: right-click → Open, or in Terminal:
     bash "1 Install plugins.command"
2. Try it:
   - standalone first: "2 Run standalone.command" (CoreAudio; the Terminal
     window shows all output; falls back to no-audio mode if audio fails),
   - then in Studio One: rescan plug-ins, add "ScriptFX PoC" to a track.
   Things to try: the library (Patches), Surface / Chain views, click a block,
   the Macros button, knob drags (cursor should hide and stay put), meta knobs
   on block pages (right-click), hot reload, close/reopen the plug-in window,
   two instances. Test patches: "Long Chain", "Drum Bus", "... (no panel)".
3. Whatever happens (also if all is fine): double-click
   "3 Collect report.command". It puts ScriptFX-report-<time>.zip on the
   Desktop (our logs, macOS crash reports from the last 3 days, system info).
   Bring that zip back.
4. If the window was slow, stuttered or drew wrongly (pink squares, missing
   icons), also double-click "4 Render check.command" before collecting the
   report. It renders the editor ten times in a row, ~15 s each, every time
   with one suspect switched off (animation, the patch's art, the icons, the
   blur pass, multisampling, the graphics backend, the present mode), and logs
   each run. Nothing to click: every run closes itself. The logs then say
   where each frame's time went and which switch made the difference. Run
   "3 Collect report.command" again afterwards to pick those logs up.

Notes in your own words help a lot: what you clicked, what happened, when
(the log has timestamps in UTC).

symbols/ holds the debug symbols (dSYMs) to turn crash-report addresses into source lines;
it stays here (not needed on the Mac).
