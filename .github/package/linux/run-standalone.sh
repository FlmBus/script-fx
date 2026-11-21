#!/bin/sh
# Starts the standalone (ALSA; JACK on PipeWire doesn't call process) with all output logged.
cd "$(dirname "$0")" || exit 1
mkdir -p ~/.local/share/ScriptFX/logs
RUST_BACKTRACE=full ./scriptfx -b alsa "$@" 2>&1 | tee ~/.local/share/ScriptFX/logs/standalone-console.txt
