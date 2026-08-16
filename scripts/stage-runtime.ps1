#Requires -Version 7.0
<#
.SYNOPSIS
Deterministically stage the bundled Node/CLI runtime for the pimp-my-dsh desktop app.

.DESCRIPTION
Produces apps/desktop/src-tauri/runtime (a build artifact, gitignored) with the
official Node.js runtime, the built distribution CLI, the exact production
dependency closure, the profile patches, the distribution patch, schema, and
licenses, plus a compatibility manifest (manifest.json) that records pinned
versions, the node.exe SHA-256, and a deterministic payload tree hash.

The packaged supervisor never consults PATH; it resolves node.exe and the CLI
entry strictly from this payload. Staging aborts on any hash or version
mismatch.
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$RepoRoot = Split-Path -Parent $PSScriptRoot
$Runtime = Join-Path $RepoRoot 'apps\desktop\src-tauri\runtime'
$ManifestName = 'manifest.json'

# --- Pinned inputs (single source of truth for the compatibility manifest) ---
$ControllerVersion = '0.1.0'
$DistributionVersion = '0.1.0'
$DshVersion = '0.1.0-rc.6'
$Target = 'x86_64-pc-windows-msvc'
$NodeVersion = '24.19.0'
$PnpmVersion = '11.7.0'
$NodeZipUrl = "https://nodejs.org/dist/v$NodeVersion/node-v$NodeVersion-win-x64.zip"
$NodeZipRoot = "node-v$NodeVersion-win-x64"
# SHA-256 of the *zip archive* (download integrity only). The manifest records
# the SHA-256 of the extracted node.exe, which differs and is computed below.
$NodeZipSha256 = '57f71ab3652e797d84acddc79c81cc9ff1c6ddb2a1974cdb83f00fee9bff4c73'

function Assert-Version {
    param(
        [string]$Name,
        [scriptblock]$Probe,
        [string]$Expected
    )
    try {
        $actual = (& $Probe).ToString().Trim()
    }
    catch {
        throw "$Name check failed: $($_.Exception.Message)"
    }
    if ($actual -ne $Expected) {
        throw "$Name version mismatch: expected '$Expected', got '$actual'. Staging aborted."
    }
}

function Write-Utf8NoBom {
    param([string]$Path, [string]$Content)
    [System.IO.File]::WriteAllText($Path, $Content, [System.Text.UTF8Encoding]::new($false))
}

function Get-TreeHash {
    param([string]$Root)
    $map = @{}
    Get-ChildItem -LiteralPath $Root -Recurse -File -Force | ForEach-Object {
        $rel = $_.FullName.Substring($Root.Length + 1).Replace('\', '/')
        $map[$rel] = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    $keys = @($map.Keys)
    [Array]::Sort($keys, [System.StringComparer]::Ordinal)

    $stream = [System.IO.MemoryStream]::new()
    try {
        foreach ($rel in $keys) {
            $digest = [Convert]::FromHexString($map[$rel])
            $stream.Write($digest, 0, $digest.Length)
            $pathBytes = [System.Text.Encoding]::UTF8.GetBytes($rel)
            $stream.Write($pathBytes, 0, $pathBytes.Length)
            $stream.WriteByte(0)
        }
        return [Convert]::ToHexString([System.Security.Cryptography.SHA256]::HashData($stream.ToArray())).ToLowerInvariant()
    }
    finally {
        $stream.Dispose()
    }
}

# 1. Toolchain pin: the exact pnpm that manages the deployment.
Assert-Version 'pnpm' { pnpm --version } $PnpmVersion

# 2. Build the distribution CLI (dist/cli.js + dist/plugin.js + …).
Push-Location $RepoRoot
try {
    & pnpm build
    if ($LASTEXITCODE -ne 0) { throw 'pnpm build failed.' }
    if (-not (Test-Path -LiteralPath (Join-Path $RepoRoot 'dist\cli.js'))) {
        throw 'CLI build did not produce dist/cli.js.'
    }
}
finally {
    Pop-Location
}

# 3. Deploy the package files + exact production dependency closure.
$Staging = Join-Path (Split-Path -Parent $Runtime) ('.runtime-staging-' + $PID)
if (Test-Path -LiteralPath $Staging) { Remove-Item -LiteralPath $Staging -Recurse -Force }

Push-Location $RepoRoot
try {
    & pnpm '--config.node-linker=hoisted' --filter pimp-my-dsh --prod deploy --legacy --ignore-scripts $Staging
    if ($LASTEXITCODE -ne 0) { throw 'pnpm deploy failed.' }
}
finally {
    # `deploy --prod` records a production-only workspace modules state when
    # used with the hoisted linker. Restore the pinned development toolchain so
    # the following Tauri build (and standalone staging) remains runnable.
    & pnpm install --frozen-lockfile --ignore-scripts
    $RestoreExitCode = $LASTEXITCODE
    Pop-Location
    if ($RestoreExitCode -ne 0) { throw 'pnpm workspace restore failed.' }
}

# 4. Normalize the CLI directory (dist -> cli) and point package.json at it.
$StagingDist = Join-Path $Staging 'dist'
$StagingCli = Join-Path $Staging 'cli'
if (-not (Test-Path -LiteralPath $StagingDist)) {
    throw "pnpm deploy omitted dist/ under $Staging"
}
Move-Item -LiteralPath $StagingDist -Destination $StagingCli

$PkgPath = Join-Path $Staging 'package.json'
$Pkg = [System.IO.File]::ReadAllText($PkgPath)
$Pkg = $Pkg.Replace('"dist/', '"cli/')
[System.IO.File]::WriteAllText($PkgPath, $Pkg, [System.Text.UTF8Encoding]::new($false))

# Runtime JavaScript never loads TypeScript declarations or source maps. Prune
# them before hashing/bundling: NSIS still uses Win32 path handling and cannot
# package some pnpm dependency paths once these suffixes push them past MAX_PATH.
Get-ChildItem -LiteralPath $Staging -Recurse -File |
    Where-Object { $_.Name.EndsWith('.d.ts') -or $_.Name.EndsWith('.d.ts.map') -or $_.Name.EndsWith('.js.map') } |
    Remove-Item -Force

# 5-7. Download the official Node zip, verify its archive SHA-256, extract
# node.exe, and verify the reported node version.
$Temp = Join-Path ([System.IO.Path]::GetTempPath()) ('pimp-my-dsh-node-' + $PID)
New-Item -ItemType Directory -Path $Temp -Force | Out-Null
$Zip = Join-Path $Temp 'node.zip'
$NodeExe = Join-Path $Staging 'node\node.exe'
try {
    Invoke-WebRequest -Uri $NodeZipUrl -OutFile $Zip -UseBasicParsing
    $zipSha = (Get-FileHash -LiteralPath $Zip -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($zipSha -ne $NodeZipSha256) {
        throw "Node zip SHA-256 mismatch: expected $NodeZipSha256, got $zipSha. Staging aborted."
    }

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    New-Item -ItemType Directory -Path (Split-Path -Parent $NodeExe) -Force | Out-Null
    $archive = [System.IO.Compression.ZipFile]::OpenRead($Zip)
    try {
        $entry = $archive.GetEntry("$NodeZipRoot/node.exe")
        if ($null -eq $entry) { throw "node.exe not found under $NodeZipRoot in the archive" }
        [System.IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $NodeExe, $true)
    }
    finally {
        $archive.Dispose()
    }

    Assert-Version 'node' { & $NodeExe --version } "v$NodeVersion"
}
finally {
    Remove-Item -LiteralPath $Temp -Recurse -Force -ErrorAction SilentlyContinue
}

# 8. Compute the manifest values.
$NodeSha256 = (Get-FileHash -LiteralPath $NodeExe -Algorithm SHA256).Hash.ToLowerInvariant()
$PayloadSha256 = Get-TreeHash $Staging

$Manifest = @"
{
  "schemaVersion": 1,
  "protocolVersion": 1,
  "controllerVersion": "$ControllerVersion",
  "node": {
    "version": "$NodeVersion",
    "sha256": "$NodeSha256"
  },
  "pnpmVersion": "$PnpmVersion",
  "distributionVersion": "$DistributionVersion",
  "dshVersion": "$DshVersion",
  "target": "$Target",
  "payloadSha256": "$PayloadSha256"
}
"@
Write-Utf8NoBom (Join-Path $Staging $ManifestName) $Manifest

# 9. Atomic swap into the runtime directory.
$Backup = Join-Path (Split-Path -Parent $Runtime) ('.runtime-backup-' + $PID)
if (Test-Path -LiteralPath $Runtime) {
    Move-Item -LiteralPath $Runtime -Destination $Backup
}
try {
    Move-Item -LiteralPath $Staging -Destination $Runtime
}
catch {
    if ((Test-Path -LiteralPath $Backup) -and -not (Test-Path -LiteralPath $Runtime)) {
        Move-Item -LiteralPath $Backup -Destination $Runtime
    }
    throw
}
if (Test-Path -LiteralPath $Backup) { Remove-Item -LiteralPath $Backup -Recurse -Force }

Write-Host "Staged runtime at $Runtime"
