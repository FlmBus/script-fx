# Packs ScriptFX's logs, Windows crash reports of the last 3 days and system info into one zip
# on the Desktop. Run: right-click -> "Run with PowerShell".
$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$dir = Join-Path $env:TEMP "ScriptFX-report-$stamp"
New-Item -ItemType Directory -Force -Path "$dir\crash-reports" | Out-Null

# Our logs (plug-in + standalone).
Copy-Item -Recurse -ErrorAction SilentlyContinue "$env:LOCALAPPDATA\ScriptFX\logs" "$dir\logs"

# Crash dumps / Windows Error Reporting of the last 3 days from the hosts we use.
Get-ChildItem -Recurse -ErrorAction SilentlyContinue "$env:LOCALAPPDATA\CrashDumps", "$env:ProgramData\Microsoft\Windows\WER\ReportArchive" |
  Where-Object { -not $_.PSIsContainer -and $_.LastWriteTime -gt (Get-Date).AddDays(-3) -and $_.FullName -match 'scriptfx|Studio|Fender|REAPER' } |
  ForEach-Object { Copy-Item -ErrorAction SilentlyContinue $_.FullName "$dir\crash-reports\" }

# Application errors that mention us.
Get-WinEvent -ErrorAction SilentlyContinue -FilterHashtable @{ LogName = 'Application'; Level = 1, 2; StartTime = (Get-Date).AddDays(-3) } |
  Where-Object { $_.Message -match 'scriptfx' } |
  Format-List TimeCreated, ProviderName, Message |
  Out-File "$dir\event-log.txt"

# System facts.
& {
  Get-ComputerInfo | Select-Object OsName, OsVersion, CsProcessors, CsTotalPhysicalMemory
  Get-CimInstance Win32_VideoController | Select-Object Name, DriverVersion
  Get-ChildItem "$env:CommonProgramFiles\VST3", "$env:CommonProgramFiles\CLAP" -ErrorAction SilentlyContinue
} | Out-File "$dir\system.txt"

$zip = Join-Path ([Environment]::GetFolderPath("Desktop")) "ScriptFX-report-$stamp.zip"
Compress-Archive -Path $dir -DestinationPath $zip
Write-Host "Report: $zip"
Read-Host "Press Enter to close"
