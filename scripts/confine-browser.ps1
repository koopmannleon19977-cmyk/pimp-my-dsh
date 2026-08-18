#Requires -Version 7.0
<#
.SYNOPSIS
Confine the pinned Playwright Chromium's network egress on Windows.

.DESCRIPTION
The browser automation Chrome runs unrestricted by default. This script pins
Windows Firewall outbound BLOCK rules on the exact Playwright Chromium
executable so the agent-driven browser can never reach loopback, RFC1918
private, link-local, or multicast destinations — the SSRF-class vector that
motivates keeping browser automation opt-in. Public internet stays reachable.

Rule group: pimp-dsh-browser-confinement. Idempotent: the group is removed
and re-created on every run, so a Playwright pin upgrade (new chromium-XXXX
directory) is handled by re-running this script.

Elevation: required for -Apply and -Cleanup (mutating firewall rules), never
for -Verify (read-only status; doctor checks can run unelevated).

Known ceiling (ponytail: per-account rules if a second browser profile ever
needs different scopes): the rules bind to ONE chromium path. Machines using
a loopback DNS resolver (rare VPN/client setups) lose Chrome DNS while
confined — browsing to the local harness web UI at 127.0.0.1 is intentionally
impossible while confined; that is the protection, not a bug.

.EXAMPLE
pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/confine-browser.ps1 -Apply
pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/confine-browser.ps1 -Verify
pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/confine-browser.ps1 -Cleanup
#>
[CmdletBinding()]
param(
    [Parameter(ParameterSetName = 'Apply')]
    [switch]$Apply,

    [Parameter(ParameterSetName = 'Verify')]
    [switch]$Verify,

    [Parameter(ParameterSetName = 'Cleanup')]
    [switch]$Cleanup
)

$ErrorActionPreference = 'Stop'
$Group = 'pimp-dsh-browser-confinement'

$BlockedV4 = @(
    '0.0.0.0/8',      # this network (local-ish)
    '10.0.0.0/8',     # RFC1918
    '127.0.0.0/8',    # loopback
    '169.254.0.0/16', # link-local
    '172.16.0.0/12',  # RFC1918
    '192.168.0.0/16', # RFC1918
    '224.0.0.0/4'     # multicast + reserved
)
$BlockedV6 = @(
    '::1/128',  # loopback
    'fc00::/7', # unique local (RFC4193)
    'fe80::/10' # link-local
)

function Get-ChromiumPath {
    $root = Join-Path $env:LOCALAPPDATA 'ms-playwright'
    if (-not (Test-Path $root)) {
        throw "No Playwright browser cache at $root (browser automation never ran, or cache relocated)."
    }
    $exe = Get-ChildItem -Path $root -Filter 'chrome.exe' -Recurse -File |
        Sort-Object FullName |
        Select-Object -Last 1
    if (-not $exe) { throw 'No chrome.exe under the Playwright cache.' }
    return $exe.FullName
}

function Assert-Elevated {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'This mode requires an elevated PowerShell. Re-run: powershell -Command "Start-Process pwsh -Verb RunAs -ArgumentList ''-NoProfile -File scripts/confine-browser.ps1 <mode>''"'
    }
}

function Remove-ConfineRules {
    Get-NetFirewallRule -Group $Group -ErrorAction SilentlyContinue |
        Remove-NetFirewallRule
}

function Test-Confined {
    param([string]$Program)
    $rules = @(Get-NetFirewallRule -Group $Group -ErrorAction SilentlyContinue)
    if ($rules.Count -eq 0) {
        Write-Warning "No rules in group '$Group' — the browser is NOT confined."
        exit 1
    }
    $wrong = @($rules | Where-Object {
        $_.Enabled -ne 'True' -or
        $_.Direction -ne 'Outbound' -or
        $_.Action -ne 'Block' -or
        ($_.GetAddressFilter().Program -ne $Program)
    })
    $scopes = @($rules | ForEach-Object { $_.GetAddressFilter().RemoteAddress })
    $missing = @()
    foreach ($cidr in ($BlockedV4 + $BlockedV6)) {
        if (-not ($scopes -contains $cidr)) { $missing += $cidr }
    }
    if ($wrong.Count -gt 0 -or $missing.Count -gt 0) {
        Write-Warning "Confinement incomplete: $($wrong.Count) mismatched rule(s), missing scopes: $($missing -join ', ')."
        exit 1
    }
    Write-Host "Browser confinement active: $($rules.Count) rules on $(Split-Path $Program -Leaf) ($Program)."
}

switch ($PSCmdlet.ParameterSetName) {
    'Verify' {
        Test-Confined -Program (Get-ChromiumPath)
    }
    'Cleanup' {
        Assert-Elevated
        Remove-ConfineRules
        Write-Host "Removed rule group '$Group'."
    }
    'Apply' {
        Assert-Elevated
        $chrome = Get-ChromiumPath
        Remove-ConfineRules
        foreach ($cidr in $BlockedV4) {
            New-NetFirewallRule -DisplayName "pimp-dsh: block outbound $cidr" -Group $Group `
                -Direction Outbound -Action Block -Program $chrome -RemoteAddress $cidr |
                Out-Null
        }
        foreach ($cidr in $BlockedV6) {
            New-NetFirewallRule -DisplayName "pimp-dsh: block outbound $cidr" -Group $Group `
                -Direction Outbound -Action Block -Program $chrome -RemoteAddress $cidr |
                Out-Null
        }
        Write-Host "Confined $(Split-Path $chrome -Leaf) ($chrome) — $($BlockedV4.Count + $BlockedV6.Count) block rules in group '$Group'. Public internet remains reachable."
        Write-Host 'Re-run after any Playwright pin upgrade (new chromium-XXXX directory).'
    }
}
