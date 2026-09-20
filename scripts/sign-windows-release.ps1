param([Parameter(Mandatory=$true)][string]$Directory, [string]$Channel='preview')
$ErrorActionPreference='Stop'
if (-not $env:WINDOWS_CERTIFICATE) {
  if ($Channel -eq 'stable') { throw 'Stable Windows releases require an Authenticode certificate.' }
  Write-Host 'Preview is unsigned.'
  exit 0
}
$pfx=Join-Path $env:RUNNER_TEMP 'nus-signing.pfx'
try {
  [IO.File]::WriteAllBytes($pfx,[Convert]::FromBase64String($env:WINDOWS_CERTIFICATE))
  $tool=Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" | Sort-Object FullName -Descending | Select-Object -First 1
  if (-not $tool) { throw 'Windows SDK signtool is missing.' }
  foreach ($exe in @((Join-Path $Directory 'nus.exe'),(Join-Path $Directory 'nus-hold.exe'),(Join-Path $Directory 'bin/nus.exe'))) {
    & $tool.FullName sign /fd SHA256 /td SHA256 /tr http://timestamp.digicert.com /f $pfx /p $env:WINDOWS_CERTIFICATE_PASSWORD $exe
    if ($LASTEXITCODE -ne 0) { throw "Signing failed: $exe" }
    & $tool.FullName verify /pa $exe
    if ($LASTEXITCODE -ne 0) { throw "Signature verification failed: $exe" }
  }
} finally { Remove-Item $pfx -Force -ErrorAction SilentlyContinue }
