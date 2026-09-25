param([Parameter(Mandatory=$true)][string]$Directory, [string]$Installer)
$ErrorActionPreference='Stop'
# Signing happens in CI through Azure Artifact Signing; this only proves the
# result. Every nus-owned executable, and the installer when given, must carry
# a valid, timestamped signature (Artifact Signing certificates last days, so an
# untimestamped one expires) from the same signer.
$files=@('nus.exe','nus-hold.exe','bin/nus.exe' | ForEach-Object { Join-Path $Directory $_ })
if ($Installer) { $files+=$Installer }
$signer=$null
foreach ($path in $files) {
  if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing executable: $path" }
  $signature=Get-AuthenticodeSignature -LiteralPath $path
  if ($signature.Status -ne 'Valid') { throw "Invalid Authenticode signature: $path ($($signature.Status): $($signature.StatusMessage))" }
  if (-not $signature.TimeStamperCertificate) { throw "Signature is not timestamped: $path" }
  $subject=$signature.SignerCertificate.Subject
  if ($signer -and $subject -ne $signer) { throw "Signer differs: $path is signed by $subject, not $signer" }
  $signer=$subject
  Write-Host "$(Split-Path -Leaf $path)  signed by $subject"
  Write-Host "$(Split-Path -Leaf $path)  timestamped by $($signature.TimeStamperCertificate.Subject)"
}
