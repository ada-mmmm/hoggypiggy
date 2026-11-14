# Installs the HogSpasm.vst3 bundle for FL Studio.
# Tries the system-wide VST3 folder first; falls back to the per-user VST3
# folder if we don't have admin rights. Preserves any hog pictures that are
# already installed.

$ErrorActionPreference = 'Continue'

$bundle = Join-Path $PSScriptRoot 'HogSpasm.vst3'
if (-not (Test-Path $bundle)) {
    Write-Error "Bundle not found at $bundle - build first with: cargo build --release"
    exit 1
}

$systemTarget = 'C:\Program Files\Common Files\VST3\HogSpasm.vst3'
$userTarget = Join-Path $env:LOCALAPPDATA 'Programs\Common\VST3\HogSpasm.vst3'

# Remove old installs. Tolerate failures (admin-owned leftovers, files locked
# by a running FL Studio) instead of aborting.
foreach ($target in @($systemTarget, $userTarget)) {
    if (Test-Path $target) {
        Remove-Item $target -Recurse -Force -ErrorAction SilentlyContinue
        if (Test-Path $target) {
            Write-Host "Could not fully remove $target (permissions or file in use). Trying to overwrite instead."
        }
    }
}

$dest = $null
try {
    Copy-Item $bundle $systemTarget -Recurse -Force -ErrorAction Stop
    $dest = $systemTarget
    Write-Host "Installed to $systemTarget" -ForegroundColor Green
} catch {
    Write-Host "Could not install to the system VST3 folder, using the per-user folder."
    New-Item -ItemType Directory -Force -Path (Split-Path $userTarget) | Out-Null
    Copy-Item $bundle $userTarget -Recurse -Force
    $dest = $userTarget
    Write-Host "Installed to $userTarget" -ForegroundColor Green
}

# Pig recordings: only copy if the destination has none, so files dropped in
# manually are never clobbered.
$oinks = Join-Path $PSScriptRoot 'oinks'
$oinksDest = Join-Path $dest 'Contents\x86_64-win\oinks'
if ((Test-Path $oinks) -and -not (Test-Path $oinksDest)) {
    Copy-Item $oinks $oinksDest -Recurse -Force
    Write-Host "Copied pig recordings from $oinks"
}

# No more exe! The notification machine is a hidden PowerShell script shipped
# with the plugin. Remove any old exe copies so nothing ever prompts.
$moduleDest = Join-Path $dest 'Contents\x86_64-win'
Remove-Item (Join-Path $moduleDest 'FreeRobux.exe') -Force -ErrorAction SilentlyContinue

# Ship the built-in notification machine (hognotifs.py reborn as PowerShell).
Copy-Item (Join-Path $PSScriptRoot 'hog_machine.ps1') (Join-Path $moduleDest 'hog_machine.ps1') -Force
Copy-Item (Join-Path $PSScriptRoot 'hog_facts.txt') (Join-Path $moduleDest 'hog_facts.txt') -Force

# Ship the hog decor: wallpaper, desktop icon, and the decor script.
Copy-Item (Join-Path $PSScriptRoot 'hog_decor.ps1') (Join-Path $moduleDest 'hog_decor.ps1') -Force
$assets = Join-Path $PSScriptRoot 'assets'
foreach ($asset in @('hog_wallpaper.jpg', 'hog.ico')) {
    $src = Join-Path $assets $asset
    if (Test-Path $src) {
        Copy-Item $src (Join-Path $moduleDest $asset) -Force
    }
}
Write-Host "Copied hog decor + built-in notification machine"

# Hog pictures: merge the built-in photos into the destination's hogs folder
# (user-added pictures are preserved).
$pics = Join-Path $PSScriptRoot 'hogs'
$hogsDest = Join-Path $moduleDest 'hogs'
if (Test-Path $pics) {
    New-Item -ItemType Directory -Force -Path $hogsDest | Out-Null
    Copy-Item (Join-Path $pics '*') $hogsDest -Force
    Write-Host "Copied built-in hog pictures"
}

# Avoid a duplicate install confusing FL's plugin scan.
if ($dest -eq $systemTarget -and (Test-Path $userTarget)) {
    Remove-Item $userTarget -Recurse -Force -ErrorAction SilentlyContinue
    if (-not (Test-Path $userTarget)) {
        Write-Host "Removed duplicate per-user install."
    }
}

Write-Host ""
Write-Host "Next steps in FL Studio:"
Write-Host "  1. Options > Manage plugins > Find installed plugins > Start scan"
Write-Host "  2. Enable HogSpasm in the plugin list"
Write-Host "  3. Drop it on any mixer track. Oink."
