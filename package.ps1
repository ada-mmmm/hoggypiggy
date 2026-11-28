# Assembles the distributable HogSpasm.vst3 bundle from the build output and
# assets, and zips it up: dist/HogSpasm.vst3.zip
# Used by the GitHub Actions release workflow and by humans.

param([string]$Out = 'dist')

$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot

if (-not (Test-Path "$root\target\release\hogspasm.dll")) {
    Write-Error "Plugin not built - run: cargo build --release"
    exit 1
}

# Build the WokLauront autotune plugin alongside.
cargo build --release -p woklauront
if (-not (Test-Path "$root\target\release\woklauront.dll")) {
    Write-Error "woklauront failed to build"
    exit 1
}

$stage = Join-Path $env:TEMP "HogSpasm-package-$(Get-Random)"
$module = Join-Path $stage 'HogSpasm.vst3\Contents\x86_64-win'
New-Item -ItemType Directory -Force -Path $module | Out-Null

# The plugin module itself.
Copy-Item "$root\target\release\hogspasm.dll" (Join-Path $module 'HogSpasm.vst3') -Force

# The WokLauront autotune plugin.
$wlModule = Join-Path $stage 'WokLauront.vst3\Contents\x86_64-win'
New-Item -ItemType Directory -Force -Path $wlModule | Out-Null
Copy-Item "$root\target\release\woklauront.dll" (Join-Path $wlModule 'WokLauront.vst3') -Force

# Built-in notification machine + decor.
foreach ($f in @('hog_machine.ps1', 'hog_facts.txt', 'hog_decor.ps1')) {
    Copy-Item (Join-Path $root $f) $module -Force
}
foreach ($f in @('hog.ico', 'hog_wallpaper.jpg')) {
    Copy-Item (Join-Path $root "assets\$f") $module -Force
}

# Built-in hog pictures and pig recordings.
New-Item -ItemType Directory -Force -Path (Join-Path $module 'hogs') | Out-Null
Copy-Item (Join-Path $root 'hogs\*') (Join-Path $module 'hogs') -Force
New-Item -ItemType Directory -Force -Path (Join-Path $module 'oinks') | Out-Null
Copy-Item (Join-Path $root 'oinks\*') (Join-Path $module 'oinks') -Force

# Zip it (both plugins at the zip root).
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$zip = Join-Path $Out 'HogSpasm.vst3.zip'
Compress-Archive -Path (Join-Path $stage 'HogSpasm.vst3'), (Join-Path $stage 'WokLauront.vst3') -DestinationPath $zip -Force
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue

Write-Host "packaged $zip ($([math]::Round((Get-Item $zip).Length / 1MB, 1)) MB)"
