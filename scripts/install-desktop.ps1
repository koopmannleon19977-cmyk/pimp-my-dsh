$ErrorActionPreference = 'Stop'
$setup = Join-Path $PSScriptRoot '..\apps\desktop\src-tauri\target\release\bundle\nsis\Pimp my DSH_0.1.0_x64-setup.exe'
$setup = (Resolve-Path $setup).Path
Write-Host "Installing $setup"
$p = Start-Process -FilePath $setup -ArgumentList '/S' -Wait -PassThru
Write-Host ("Installer exit code: " + $p.ExitCode)
# Common per-user install locations
$candidates = @(
    "$env:LOCALAPPDATA\Programs\Pimp my DSH\Pimp my DSH.exe",
    "$env:LOCALAPPDATA\Programs\Pimp my DSH\pimp-my-dsh.exe",
    "$env:LOCALAPPDATA\Pimp my DSH\Pimp my DSH.exe"
)
foreach ($c in $candidates) {
    if (Test-Path $c) { Write-Host ("FOUND: " + $c) }
}
Write-Host "Install done"