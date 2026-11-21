@echo off
rem Starts the ScriptFX standalone (WASAPI) and keeps its output in the logs folder.
cd /d "%~dp0"
if not exist "%LOCALAPPDATA%\ScriptFX\logs" mkdir "%LOCALAPPDATA%\ScriptFX\logs"
set "OUT=%LOCALAPPDATA%\ScriptFX\logs\standalone-console.txt"
set RUST_BACKTRACE=full
echo Output: %OUT%
echo Close the ScriptFX window to stop.
scriptfx.exe -b wasapi > "%OUT%" 2>&1
set CODE=%errorlevel%
echo exit code: %CODE% >> "%OUT%"
if not "%CODE%"=="0" (
  echo Audio start failed, retrying without audio ^(-b dummy^) >> "%OUT%"
  scriptfx.exe -b dummy >> "%OUT%" 2>&1
)
type "%OUT%"
pause
