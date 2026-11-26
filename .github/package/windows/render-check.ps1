# Renders the editor a few times in a row, each run with one suspect switched off, and writes a
# log per run. Nothing to click: every run ends itself. Afterwards run collect-report.ps1.
# Run: right-click -> "Run with PowerShell".
#
# What the logs answer (see crates/plugin/src/diag.rs):
#   gpu:     which adapter and backend, and whether it is a software one
#   frame #  where a frame's time went: view+iced / acquire / encode / swap / idle, and the GPU's
#   STALL    the step a frame was stuck in, logged while it hangs
#   frames . percentiles per 5 s, images uploaded per frame, atlas growth, wgpu errors
$ErrorActionPreference = "Continue"
Set-Location $PSScriptRoot

$seconds = if ($env:SECONDS_PER_RUN) { $env:SECONDS_PER_RUN } else { "15" }
$logs = "$env:LOCALAPPDATA\ScriptFX\logs"
New-Item -ItemType Directory -Force -Path $logs | Out-Null
$out = Join-Path $logs "render-check-$(Get-Date -Format 'yyyyMMdd-HHmmss').txt"
Write-Host "Render check: $seconds s per run. Output: $out"

$env:SCRIPTFX_DIAG_EXIT_AFTER = $seconds
$env:SCRIPTFX_PATCHES = Join-Path $PSScriptRoot "test-patches"
$env:RUST_BACKTRACE = "full"

# Each run: a name and the environment it adds. One suspect at a time, then all of them, then the
# other backends and present modes.
$runs = @(
  @{ name = "baseline";          env = @{ SCRIPTFX_DIAG_FRAMES = "all" } },
  @{ name = "still";             env = @{ SCRIPTFX_DIAG_STILL = "1" } },
  @{ name = "no-patch-layers";   env = @{ SCRIPTFX_DIAG_NO_PATCH_LAYERS = "1" } },
  @{ name = "no-icons";          env = @{ SCRIPTFX_DIAG_NO_ICONS = "1" } },
  @{ name = "no-postfx";         env = @{ SCRIPTFX_DIAG_NO_POSTFX = "1" } },
  @{ name = "no-msaa";           env = @{ SCRIPTFX_DIAG_NO_MSAA = "1" } },
  @{ name = "bare";              env = @{ SCRIPTFX_DIAG_STILL = "1"; SCRIPTFX_DIAG_NO_PATCH_LAYERS = "1";
                                          SCRIPTFX_DIAG_NO_ICONS = "1"; SCRIPTFX_DIAG_NO_POSTFX = "1";
                                          SCRIPTFX_DIAG_NO_MSAA = "1" } },
  @{ name = "backend-dx12";      env = @{ WGPU_BACKEND = "dx12" } },
  @{ name = "backend-vulkan";    env = @{ WGPU_BACKEND = "vulkan" } },
  @{ name = "present-immediate"; env = @{ ICED_PRESENT_MODE = "immediate" } }
)

foreach ($run in $runs) {
  $added = $run.env.Keys
  Write-Host "--- run: $($run.name) ($($added -join ', '))"
  "--- run: $($run.name) ($($added -join ', '))" | Out-File -Append $out
  $env:SCRIPTFX_DIAG_RUN = $run.name
  foreach ($k in $added) { Set-Item -Path "env:$k" -Value $run.env[$k] }
  & .\scriptfx.exe -b dummy *>> $out
  "    exit code: $LASTEXITCODE" | Out-File -Append $out
  # Leave the next run a clean environment.
  foreach ($k in $added) { Remove-Item -Path "env:$k" -ErrorAction SilentlyContinue }
}

Write-Host ""
Write-Host "Done. The logs are in $logs - now run collect-report.ps1."
Select-String -Path "$logs\*.log" -Pattern "frames " | Select-Object -Last 20 | ForEach-Object { $_.Line }
Read-Host "Press Enter to close"
