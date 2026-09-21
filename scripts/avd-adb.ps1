# scripts/avd-adb.ps1 — PowerShell wrapper for adb on AVD (avoids MSYS path mangling).
param([Parameter(ValueFromRemainingArguments=$true)][string[]]$Args)

$adb = Join-Path $env:LOCALAPPDATA "Android\Sdk\platform-tools\adb.exe"
& $adb @Args
exit $LASTEXITCODE