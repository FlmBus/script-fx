ScriptFX (Linux x86_64), diagnostics build
==========================================

An optimized build with debug symbols that writes a detailed log
(~/.local/share/ScriptFX/logs/). Install: ./install.sh. Standalone:
./run-standalone.sh. Rendering slow or wrong: ./render-check.sh (ten short
runs, one suspect switched off each time, logged). Afterwards:
./collect-report.sh (makes ~/ScriptFX-report-<time>.tar.gz). Hosts: Reaper, Carla (VST3; CLAP from Carla 2.6).
