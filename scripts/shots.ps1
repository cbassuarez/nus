<#
.SYNOPSIS
  Screenshots of the running app, for nus.dev - taken by the app itself.

  Launches composite.exe once per face (paper, ink) with NUS_SHOT pointing at
  docs/media/shots.txt. The app runs that script on its own event loop and
  writes each `shot` as a PNG from its own render target; this script never
  sends input, never takes focus, and never reads the screen. It then writes
  docs/media/manifest.json: the commit, the scale factor and every file's size.

.EXAMPLE
  .\scripts\shots.ps1                 # both faces
  .\scripts\shots.ps1 -Face ink
  .\scripts\shots.ps1 -Exe spikes\composite\target\release\composite.exe

.NOTES
  Runs against a scratch profile under $env:TEMP\nus-shots, rebuilt each run:
  window pinned to a known size, sidebar pinned, splash and sounds off.
  The atlas shot wants a previous session, so a seed pass runs first unless
  -NoSeed. Shots that open a page want the site on :8000 (`npm run dev` in
  ../nus.dev); the driver starts it if nothing is listening.
#>
param(
  [string]$Exe = '',
  [ValidateSet('paper', 'ink', 'both')][string]$Face = 'both',
  [string]$Out = '',
  [string]$Site = '',
  [string]$Script = '',
  [string]$SeedScript = '',
  # A script for windows the first one opens (newwindow), else they run nothing
  [string]$Script2 = '',
  [switch]$NoSeed,
  [int]$TimeoutSec = 240,
  # The window, in logical px
  [int[]]$Size = @(1280, 800),
  # Behavior keys to set over the scratch defaults, e.g. @{ splash = "Draw"; then = "Prompt"; home_look = "Plate" }
  [hashtable]$Behavior = @{}
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if (-not $Out) { $Out = Join-Path $root 'docs\media' }
if (-not $Site) { $Site = Join-Path (Split-Path -Parent $root) 'nus.dev' }
$script = if ($Script) { (Resolve-Path $Script).Path } else { Join-Path $root 'docs\media\shots.txt' }
$seedScript = if ($SeedScript) { (Resolve-Path $SeedScript).Path } else { $script }

# --- the exe -------------------------------------------------------------------
if (-not $Exe) {
  $cands = @('release', 'debug') | ForEach-Object { Join-Path $root "spikes\composite\target\$_\composite.exe" } | Where-Object { Test-Path $_ }
  if (-not $cands) { throw "No composite.exe under spikes/composite/target - build it first (cargo build in spikes/composite)." }
  $Exe = @($cands | Sort-Object { (Get-Item $_).LastWriteTime } -Descending)[0]
}
$Exe = (Resolve-Path $Exe).Path
Write-Host "exe   $Exe  ($((Get-Item $Exe).LastWriteTime))"

# --- the site on :8000 ---------------------------------------------------------
$siteProc = $null
if (-not (Get-NetTCPConnection -LocalPort 8000 -State Listen -ErrorAction SilentlyContinue)) {
  if (Test-Path (Join-Path $Site 'scripts\build.mjs')) {
    Write-Host "site  node scripts/build.mjs --serve in $Site"
    $siteProc = Start-Process node -ArgumentList 'scripts/build.mjs', '--serve' -WorkingDirectory $Site -PassThru -WindowStyle Hidden
    Start-Sleep -Seconds 2
  } else {
    Write-Warning "nothing on :8000 and no site checkout at $Site - page shots will show an error page"
  }
}

# --- scale and window ------------------------------------------------------------
$dpi = (Get-ItemProperty 'HKCU:\Control Panel\Desktop\WindowMetrics' -ErrorAction SilentlyContinue).AppliedDPI
if (-not $dpi) { $dpi = 96 }
$scale = $dpi / 96
$logical = @($Size[0], $Size[1])
$phys = @([int]($logical[0] * $scale), [int]($logical[1] * $scale))
Write-Host "scale $scale  window $($logical -join 'x') logical = $($phys -join 'x') px"

# --- scratch profile ---------------------------------------------------------------
$scratch = Join-Path $env:TEMP 'nus-shots'
$profile = Join-Path $scratch 'profile'
for ($try = 0; $try -lt 6 -and (Test-Path $scratch); $try++) {
  try { Remove-Item $scratch -Recurse -Force -ErrorAction Stop } catch { Start-Sleep -Milliseconds 500 }
}
New-Item -ItemType Directory -Force $profile | Out-Null
$s = Get-Content (Join-Path $root 'spikes\composite\profile\settings.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$s.sidebar_pinned = $true
$s.window_rect = @(0, 0, $phys[0], $phys[1])
$s.behavior.splash = 'None'
$s.behavior.startup_sound = $false
$s.behavior.window_start = 'Last'
$s.behavior.follow_os_theme = $true
$s.behavior.start_on_launch = $false
$s.behavior | Add-Member -NotePropertyName ports_show_connections -NotePropertyValue $false -Force   # the board lists real peers otherwise
foreach ($k in $Behavior.Keys) { $s.behavior | Add-Member -NotePropertyName $k -NotePropertyValue $Behavior[$k] -Force }
# No BOM: serde_json refuses one, and PowerShell 5.1's -Encoding UTF8 writes it.
[IO.File]::WriteAllText((Join-Path $profile 'settings.json'), ($s | ConvertTo-Json -Depth 20), (New-Object Text.UTF8Encoding $false))
Set-Content (Join-Path $profile 'onboarded') 'skip' -Encoding ASCII
$resolved = Join-Path $scratch 'shots.txt'
[IO.File]::WriteAllText($resolved, ((Get-Content $script -Raw -Encoding UTF8) -replace '\{repo\}', $root), (New-Object Text.UTF8Encoding $false))
$resolvedSeed = Join-Path $scratch 'seed.txt'
[IO.File]::WriteAllText($resolvedSeed, ((Get-Content $seedScript -Raw -Encoding UTF8) -replace '\{repo\}', $root), (New-Object Text.UTF8Encoding $false))

# --- one launch per face -----------------------------------------------------------
function Invoke-Face([string]$face, [string]$out, [string]$which = $resolved) {
  $env:NUS_MODE = $face
  $env:NUS_SHOT = $which
  $env:NUS_SHOT_OUT = $out
  $env:NUS_SHELL = 'pwsh'
  if ($Script2) { $env:NUS_SHOT2 = (Resolve-Path $Script2).Path }
  $env:PATH = "$(Split-Path -Parent $Exe);$env:PATH"
  $p = Start-Process $Exe -WorkingDirectory $scratch -PassThru
  if (-not $p.WaitForExit($TimeoutSec * 1000)) {
    Write-Warning "$face`: still running after $TimeoutSec s - stopping it"
    & cmd /c "taskkill /PID $($p.Id) /T /F >nul 2>&1"
  }
  Remove-Item Env:NUS_MODE, Env:NUS_SHOT, Env:NUS_SHOT_OUT, Env:NUS_SHELL, Env:NUS_SHOT2 -ErrorAction SilentlyContinue
}

New-Item -ItemType Directory -Force $Out | Out-Null
$faces = if ($Face -eq 'both') { @('paper', 'ink') } else { @($Face) }
if (-not $NoSeed) {
  Write-Host "seed  $($faces[0]) (leaves a session for the atlas; output discarded)"
  Invoke-Face $faces[0] (Join-Path $scratch 'seed') $resolvedSeed
}
foreach ($face in $faces) {
  Write-Host "face  $face"
  Invoke-Face $face $Out
}

# --- manifest ---------------------------------------------------------------------------
Add-Type -AssemblyName System.Drawing
$commit = (& git -C $root rev-parse --short HEAD).Trim()
$shots = Get-ChildItem $Out -Filter '*.png' | Sort-Object Name | ForEach-Object {
  $img = [System.Drawing.Image]::FromFile($_.FullName)
  $m = [regex]::Match($_.BaseName, '^(.*)-(paper|ink)$')
  $row = [ordered]@{ name = $m.Groups[1].Value; face = $m.Groups[2].Value; file = $_.Name; width = $img.Width; height = $img.Height }
  $img.Dispose()
  $row
}
$manifest = [ordered]@{
  commit = $commit
  taken = (Get-Date).ToString('yyyy-MM-dd HH:mm')
  exe = (Split-Path -Leaf (Split-Path -Parent $Exe))
  scale = $scale
  window = [ordered]@{ logical = $logical; px = $phys }
  shots = @($shots)
}
[IO.File]::WriteAllText((Join-Path $Out 'manifest.json'), ($manifest | ConvertTo-Json -Depth 6), (New-Object Text.UTF8Encoding $false))

if ($siteProc) { Stop-Process -Id $siteProc.Id -Force -ErrorAction SilentlyContinue }
Write-Host "`n$(@($shots).Count) file(s) in $Out at ${scale}x, commit $commit. See manifest.json."
