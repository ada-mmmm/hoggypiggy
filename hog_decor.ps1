# Hogifies the desktop (apply) and puts everything back (revert).
#
# Wallpaper: uses the modern IDesktopWallpaper COM interface (the legacy
# SystemParametersInfo route silently fails on some systems, e.g. when the
# current wallpaper is a .HEIC set through the Photos app). The original
# wallpaper path and a copy of the live transcoded wallpaper are saved as a
# fallback for the revert.
#
# Icons: every .lnk shortcut, .url internet shortcut, and folder on the
# desktop(s) gets a hog icon. Originals are recorded first.
#
# State: %LOCALAPPDATA%\HogSpasm
#   wallpaper_orig.txt  - original wallpaper path (from the registry)
#   wallpaper_orig.jpg  - copy of the live transcoded wallpaper (fallback)
#   wallpaper_hog.*     - the hog wallpaper we applied
#   icons_state.txt     - original icon assignments
#   decor_log.txt       - what happened, for debugging

param(
    [Parameter(Mandatory = $true)][string]$Mode,
    [string]$Ico,
    [string]$Wallpaper
)

$ErrorActionPreference = 'SilentlyContinue'

$stateDir = Join-Path $env:LOCALAPPDATA 'HogSpasm'
New-Item -ItemType Directory -Force -Path $stateDir | Out-Null
$log = Join-Path $stateDir 'decor_log.txt'

function Log($msg) {
    "$(Get-Date -Format s)  $msg" | Out-File -LiteralPath $log -Append -Encoding UTF8
}

Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

namespace Hog {
    public class Wallpaper {
        [ComImport]
        [Guid("B92B56A9-8B55-4E14-9A89-0199BBB6F93B")]
        [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
        private interface IDesktopWallpaper {
            void SetWallpaper([MarshalAs(UnmanagedType.LPWStr)] string monitorID, [MarshalAs(UnmanagedType.LPWStr)] string wallpaper);
            [return: MarshalAs(UnmanagedType.LPWStr)]
            string GetWallpaper([MarshalAs(UnmanagedType.LPWStr)] string monitorID);
        }

        [ComImport]
        [Guid("C2CF3110-460E-4FC1-B9D0-8A1C0C9CC4BD")]
        private class DesktopWallpaperClass {}

        public static string Set(string path) {
            var obj = (IDesktopWallpaper)new DesktopWallpaperClass();
            obj.SetWallpaper(null, path);
            return obj.GetWallpaper(null);
        }
        public static string Get() {
            var obj = (IDesktopWallpaper)new DesktopWallpaperClass();
            return obj.GetWallpaper(null);
        }
    }
}
'@
Add-Type -Namespace Hog -Name Shell -MemberDefinition '[DllImport("shell32.dll")] public static extern void SHChangeNotify(int wEventId, int uFlags, IntPtr dwItem1, IntPtr dwItem2);'

function Refresh-Desktop {
    # SHCNE_ASSOCCHANGED - makes Explorer re-read icons.
    [Hog.Shell]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)
    ie4uinit.exe -show 2>$null | Out-Null
}

function Get-RegistryWallpaper {
    (Get-ItemProperty 'HKCU:\Control Panel\Desktop' -Name Wallpaper -ErrorAction SilentlyContinue).Wallpaper
}

# ---------------------------------------------------------------------------
if ($Mode -eq 'apply') {
    $wallpaperState = Join-Path $stateDir 'wallpaper_orig.txt'

    if (Test-Path $wallpaperState) {
        Log "apply: already hogged, skipping wallpaper"
    } elseif ($Wallpaper -and (Test-Path $Wallpaper)) {
        # Save the original: registry path + a copy of the live transcoded
        # wallpaper (works even if the original is a weird format like HEIC).
        $orig = Get-RegistryWallpaper
        $orig | Out-File -LiteralPath $wallpaperState -Encoding UTF8
        $transcoded = Join-Path $env:APPDATA 'Microsoft\Windows\Themes\TranscodedWallpaper'
        if (Test-Path $transcoded) {
            Copy-Item $transcoded (Join-Path $stateDir 'wallpaper_orig.jpg') -Force
        }

        $got = [Hog.Wallpaper]::Set($Wallpaper)
        Log "apply: set wallpaper '$Wallpaper' -> now '$got'"
    } else {
        Log "apply: no wallpaper file provided, skipping wallpaper"
    }

    if (-not $Ico -or -not (Test-Path $Ico)) {
        Log "apply: no icon file provided, skipping icons"
        exit 0
    }

    $iconsState = Join-Path $stateDir 'icons_state.txt'
    if (Test-Path $iconsState) {
        Log "apply: icons already hogged, skipping"
        exit 0
    }

    $lines = New-Object System.Collections.Generic.List[string]
    $changed = 0
    $failed = 0
    $ws = New-Object -ComObject WScript.Shell

    $desktops = @([Environment]::GetFolderPath('Desktop'), [Environment]::GetFolderPath('CommonDesktopDirectory')) | Select-Object -Unique

    foreach ($desktop in $desktops) {
        if (-not (Test-Path $desktop)) { continue }

        Get-ChildItem -LiteralPath $desktop -Force | ForEach-Object {
            if ($_.PSIsContainer) {
                $ini = Join-Path $_.FullName 'desktop.ini'
                if (Test-Path $ini) {
                    $lines.Add("DIRINI|$($_.FullName)|EXISTS|$([Convert]::ToBase64String([IO.File]::ReadAllBytes($ini)))")
                } else {
                    $lines.Add("DIRINI|$($_.FullName)|NONE|")
                }
            } elseif ($_.Extension -ieq '.lnk') {
                $lnk = $ws.CreateShortcut($_.FullName)
                if ($lnk) {
                    $lines.Add("LNK|$($_.FullName)|$($lnk.IconLocation)|")
                    $lnk.IconLocation = "$Ico,0"
                    $saved = $false
                    for ($try = 0; $try -lt 2 -and -not $saved; $try++) {
                        $lnk.Save()
                        $saved = $?
                        if (-not $saved) { Start-Sleep -Milliseconds 150 }
                    }
                    if ($saved) {
                        $changed++
                        $_.LastWriteTime = Get-Date   # nudge Explorer to re-read
                    } else {
                        $failed++
                        Log "apply: failed to save shortcut $($_.FullName)"
                    }
                }
            } elseif ($_.Extension -ieq '.url') {
                $raw = [IO.File]::ReadAllText($_.FullName)
                $lines.Add("URL|$($_.FullName)|EXISTS|$([Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($raw)))")
                if ($raw -match '(?m)^IconFile=') {
                    $raw = [regex]::Replace($raw, '(?m)^IconFile=.*$', "IconFile=$Ico")
                    $raw = [regex]::Replace($raw, '(?m)^IconIndex=.*$', 'IconIndex=0')
                } else {
                    $raw = $raw.TrimEnd() + "`r`nIconFile=$Ico`r`nIconIndex=0`r`n"
                }
                if ([IO.File]::WriteAllText($_.FullName, $raw, [Text.Encoding]::UTF8)) { }
                $changed++
            }
        }
    }

    foreach ($desktop in $desktops) {
        if (-not (Test-Path $desktop)) { continue }

        Get-ChildItem -LiteralPath $desktop -Directory -Force | ForEach-Object {
            $ini = Join-Path $_.FullName 'desktop.ini'
            "[.ShellClassInfo]`r`nIconResource=$Ico,0" | Out-File -LiteralPath $ini -Encoding Unicode
            attrib +s +h "$ini" | Out-Null
        }
    }

    $lines | Out-File -LiteralPath $iconsState -Encoding UTF8
    Log "apply: icons hogged ($changed changed, $failed failed)"
    Refresh-Desktop
}
# ---------------------------------------------------------------------------
elseif ($Mode -eq 'revert') {
    # Wallpaper back.
    $wallpaperState = Join-Path $stateDir 'wallpaper_orig.txt'
    if (Test-Path $wallpaperState) {
        $orig = (Get-Content -LiteralPath $wallpaperState -ErrorAction SilentlyContinue | Select-Object -First 1)
        $orig = "$orig".Trim()
        $restored = $false

        if ($orig -ne '') {
            try {
                $got = [Hog.Wallpaper]::Set($orig)
                $restored = ($got -eq $orig)
            } catch { $restored = $false }
        }

        if (-not $restored) {
            $fallback = Join-Path $stateDir 'wallpaper_orig.jpg'
            if (Test-Path $fallback) {
                try {
                    $got = [Hog.Wallpaper]::Set($fallback)
                    Log "revert: original format refused, restored transcoded copy instead -> '$got'"
                    $restored = $true
                } catch { Log "revert: fallback wallpaper also failed: $($_.Exception.Message)" }
            }
        }

        Remove-Item -LiteralPath $wallpaperState -Force
        Log "revert: wallpaper restored=$restored"
    } else {
        Log "revert: no wallpaper state, nothing to do"
    }

    # Icons back.
    $iconsState = Join-Path $stateDir 'icons_state.txt'
    if (Test-Path $iconsState) {
        $ws = New-Object -ComObject WScript.Shell

        Get-Content -LiteralPath $iconsState | ForEach-Object {
            $parts = $_ -split '\|', 4
            if ($parts.Count -lt 3) { return }

            if ($parts[0] -eq 'LNK') {
                $lnk = $ws.CreateShortcut($parts[1])
                if ($lnk) {
                    if ($parts[2] -and $parts[2].Trim() -ne '') {
                        $lnk.IconLocation = $parts[2]
                    } else {
                        # Empty original icon location = target's own icon.
                        $lnk.IconLocation = ',0'
                    }
                    $lnk.Save()
                }
            } elseif ($parts[0] -eq 'URL') {
                if ($parts[2] -eq 'EXISTS' -and $parts[3]) {
                    [IO.File]::WriteAllBytes($parts[1], [Convert]::FromBase64String($parts[3]))
                }
            } elseif ($parts[0] -eq 'DIRINI') {
                $ini = Join-Path $parts[1] 'desktop.ini'
                if ($parts[2] -eq 'EXISTS' -and $parts[3]) {
                    [IO.File]::WriteAllBytes($ini, [Convert]::FromBase64String($parts[3]))
                } else {
                    attrib -s -h "$ini" | Out-Null
                    Remove-Item -LiteralPath $ini -Force
                }
            }
        }

        Remove-Item -LiteralPath $iconsState -Force
        Log "revert: icons restored"
    } else {
        Log "revert: no icons state, nothing to do"
    }

    Refresh-Desktop
}
