#Requires -Version 7.0
<#
.SYNOPSIS
Build the desktop app with the local Tauri updater signing key.

.DESCRIPTION
tauri-cli 2.11.4 requires TAURI_SIGNING_PRIVATE_KEY for every NSIS build once
plugins.updater is configured (source-verified: sign_updaters hard-errors with
"A public key has been found, but no private key"). This wrapper reads the
gitignored keys/ material staged by scripts/release-setup.sh and passes any
extra arguments (e.g. --config "...") straight through to `tauri build`.
Authenticode is NOT applied locally — local bundles stay unsigned dev artifacts.
#>
[CmdletBinding()]
param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$TauriArgs
)

$ErrorActionPreference = 'Stop'

$RepoRoot = Split-Path -Parent $PSScriptRoot
$KeyFile = Join-Path $RepoRoot 'keys\tauri-updater.key'
$PwFile = Join-Path $RepoRoot 'keys\tauri-updater.password.txt'

if (-not (Test-Path $KeyFile)) {
    throw "Updater signing key not found at $KeyFile. Run scripts/release-setup.sh (key custody) or set TAURI_SIGNING_PRIVATE_KEY yourself."
}
$env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content $KeyFile -Raw).Trim()
if (Test-Path $PwFile) {
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = (Get-Content $PwFile -Raw).Trim()
}

Write-Host "Signing updater artifacts with $KeyFile"
pnpm --filter @pimp-my-dsh/desktop tauri build @TauriArgs
