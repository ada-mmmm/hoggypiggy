# The lightweight HogSpasm installer. Downloads the latest release from
# GitHub (plugin + pictures + sounds + machine, everything built in) and
# installs it. Nothing else needed.

$ErrorActionPreference = 'Stop'
$Repo = 'ada-mmmm/hoggypiggy'

[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$url = "https://github.com/$Repo/releases/latest/download/HogSpasm.vst3.zip"
$tmp = Join-Path $env:TEMP "HogSpasm-install-$(Get-Random)"
New-Item -ItemType Directory -Force -Path $tmp | Out-Null

Write-Host "Downloading the hog from GitHub..."
$zip = Join-Path $tmp 'HogSpasm.vst3.zip'
Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing

Write-Host "Unpacking..."
Expand-Archive -LiteralPath $zip -DestinationPath $tmp -Force
$bundle = Get-ChildItem $tmp -Recurse -Directory -Filter 'HogSpasm.vst3' | Select-Object -First 1
if (-not $bundle) {
    Write-Error "Bundle not found in the downloaded release."
    exit 1
}

$systemTarget = 'C:\Program Files\Common Files\VST3\HogSpasm.vst3'
$userTarget = Join-Path $env:LOCALAPPDATA 'Programs\Common\VST3\HogSpasm.vst3'

$dest = $null
try {
    Copy-Item $bundle.FullName $systemTarget -Recurse -Force -ErrorAction Stop
    $dest = $systemTarget
    Write-Host "Installed to $systemTarget" -ForegroundColor Green
} catch {
    Write-Host "No admin rights for the system folder - installing for this user instead."
    New-Item -ItemType Directory -Force -Path (Split-Path $userTarget) | Out-Null
    Copy-Item $bundle.FullName $userTarget -Recurse -Force
    $dest = $userTarget
    Write-Host "Installed to $userTarget" -ForegroundColor Green
}

Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue

Write-Host ""
Write-Host "HogSpasm is ready. In FL Studio:"
Write-Host "  1. Options > Manage plugins > Find installed plugins > Start scan"
Write-Host "  2. Enable HogSpasm in the plugin list"
Write-Host "  3. Drop it on any mixer track. Oink."
