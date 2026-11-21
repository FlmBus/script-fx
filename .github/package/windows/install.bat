@echo off
rem Installs ScriptFX (VST3 + CLAP) and the test patches. Asks for admin rights:
rem the plug-in folders under Program Files need them.
net session >nul 2>&1
if %errorlevel% neq 0 (
  powershell -Command "Start-Process -Verb RunAs -FilePath '%~f0'"
  exit /b
)
cd /d "%~dp0"
set "VST3=%CommonProgramFiles%\VST3"
set "CLAP=%CommonProgramFiles%\CLAP"
if not exist "%VST3%" mkdir "%VST3%"
if not exist "%CLAP%" mkdir "%CLAP%"
rmdir /s /q "%VST3%\ScriptFX.vst3" 2>nul
del /q "%CLAP%\ScriptFX.clap" 2>nul
xcopy /e /i /y "ScriptFX.vst3" "%VST3%\ScriptFX.vst3" >nul
copy /y "ScriptFX.clap" "%CLAP%\" >nul
if not exist "%APPDATA%\ScriptFX\patches" mkdir "%APPDATA%\ScriptFX\patches"
xcopy /e /i /y "test-patches" "%APPDATA%\ScriptFX\patches" >nul
echo.
echo Installed to %VST3% and %CLAP%.
echo Rescan plug-ins in the DAW, then add "ScriptFX PoC" to a track.
echo Logs go to %LOCALAPPDATA%\ScriptFX\logs
pause
