# nus shell integration for PowerShell (pwsh and Windows PowerShell).
# Emits OSC 133 prompt marks, OSC 7 for the working directory, and keeps
# any prompt you already had. Sourced by nus; harmless elsewhere.
if ($env:NUS_SHELL_INTEGRATION -eq 'off') { return }
if ($Global:__nus_integrated) { return }
$Global:__nus_integrated = $true
$Global:__nus_last_history = -1
$esc = [char]27; $bel = [char]7
if (-not (Test-Path Function:\Global:__nus_prompt_orig)) {
    if (Test-Path Function:\prompt) { Copy-Item Function:\prompt Function:\Global:__nus_prompt_orig } else { function Global:__nus_prompt_orig { "PS $($executionContext.SessionState.Path.CurrentLocation)$('>' * ($nestedPromptLevel + 1)) " } }
}
function Global:prompt {
  try {
    $err = if ($?) { 0 } else { 1 }
    if ($null -ne $LASTEXITCODE -and $LASTEXITCODE -ne 0) { $err = $LASTEXITCODE }
    $last = Get-History -Count 1
    $out = ""
    if ($Global:__nus_last_history -ne -1) {
        if ($null -ne $last -and $last.Id -eq $Global:__nus_last_history) { $out += "$esc]133;D$bel" }
        else { $out += "$esc]133;D;$err$bel" }
    }
    $loc = $executionContext.SessionState.Path.CurrentLocation.ProviderPath
    $out += "$esc]7;file://$env:COMPUTERNAME/$($loc.Replace([char]92, '/'))$bel"
    $out += "$esc]133;A$bel"
    $out += (& Global:__nus_prompt_orig)
    $out += "$esc]133;B$bel"
    if ($null -ne $last) { $Global:__nus_last_history = $last.Id }
    return $out
  } catch {
    # Never leave the user without a prompt.
    return "PS $($executionContext.SessionState.Path.CurrentLocation)> "
  }
}
if (Get-Module -Name PSReadLine) {
    Set-PSReadLineKeyHandler -Key Enter -BriefDescription 'nus: accept line' -ScriptBlock {
        [Console]::Write("$([char]27)]133;C$([char]7)")
        [Microsoft.PowerShell.PSConsoleReadLine]::AcceptLine()
    }
}
