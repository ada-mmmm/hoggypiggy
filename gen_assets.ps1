# One-time asset generator: draws the hog wallpaper and the hog .ico
# (same cartoon style as the popup hogs).

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$outDir = Join-Path $PSScriptRoot 'assets'
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

function Draw-PigFace([System.Drawing.Graphics]$g, [double]$cx, [double]$cy, [double]$rx, [double]$ry) {
    $pink = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(244, 167, 185))
    $darkPink = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(214, 120, 140))
    $snout = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 205, 215))
    $nostril = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(100, 40, 50))
    $white = [System.Drawing.Brushes]::White
    $black = [System.Drawing.Brushes]::Black
    $outline = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(214, 120, 140)), ($rx * 0.06)

    # Ears
    foreach ($side in @(-1, 1)) {
        $pts = @(
            (New-Object System.Drawing.PointF ($cx + $side * $rx * 0.90), ($cy - $ry * 0.30)),
            (New-Object System.Drawing.PointF ($cx + $side * $rx * 0.50), ($cy - $ry * 1.00)),
            (New-Object System.Drawing.PointF ($cx + $side * $rx * 0.12), ($cy - $ry * 0.52))
        )
        $g.FillPolygon($pink, $pts)
        $g.DrawPolygon($outline, $pts)
    }

    # Head
    $g.FillEllipse($pink, [single]($cx - $rx), [single]($cy - $ry), [single]($rx * 2), [single]($ry * 2))
    $g.DrawEllipse($outline, [single]($cx - $rx), [single]($cy - $ry), [single]($rx * 2), [single]($ry * 2))

    # Snout + nostrils
    $snoutY = $cy + $ry * 0.38
    $g.FillEllipse($snout, [single]($cx - $rx * 0.42), [single]($snoutY - $ry * 0.30), [single]($rx * 0.84), [single]($ry * 0.60))
    $g.DrawEllipse($outline, [single]($cx - $rx * 0.42), [single]($snoutY - $ry * 0.30), [single]($rx * 0.84), [single]($ry * 0.60))
    foreach ($side in @(-1, 1)) {
        $nr = $rx * 0.055
        $g.FillEllipse($nostril, [single]($cx + $side * $rx * 0.16 - $nr), [single]($snoutY - $nr), [single]($nr * 2), [single]($nr * 2))
    }

    # Eyes
    $eyeY = $cy - $ry * 0.18
    $er = $rx * 0.13
    foreach ($side in @(-1, 1)) {
        $ex = $cx + $side * $rx * 0.42
        $g.FillEllipse($white, [single]($ex - $er), [single]($eyeY - $er), [single]($er * 2), [single]($er * 2))
        $pr = $er * 0.5
        $g.FillEllipse($black, [single]($ex - $pr), [single]($eyeY - $pr), [single]($pr * 2), [single]($pr * 2))
    }

    # Rosy cheeks
    foreach ($side in @(-1, 1)) {
        $g.FillEllipse($darkPink, [single]($cx + $side * $rx * 0.58 - $rx * 0.11), [single]($cy + $ry * 0.10 - $ry * 0.08), [single]($rx * 0.22), [single]($ry * 0.16))
    }

    $pink.Dispose(); $darkPink.Dispose(); $snout.Dispose(); $nostril.Dispose(); $outline.Dispose()
}

# ---- Wallpaper: 1920x1080, soft pink field of hogs ----
$bmp = New-Object System.Drawing.Bitmap 1920, 1080
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias

$bg = New-Object System.Drawing.Drawing2D.LinearGradientBrush (
    (New-Object System.Drawing.Point 0, 0), (New-Object System.Drawing.Point 0, 1080),
    [System.Drawing.Color]::FromArgb(255, 232, 239), [System.Drawing.Color]::FromArgb(252, 231, 240)
)
$g.FillRectangle($bg, 0, 0, 1920, 1080)

$rand = New-Object System.Random 42
# Scattered small hogs
for ($i = 0; $i -lt 26; $i++) {
    $size = 60 + $rand.Next(120)
    $x = $rand.Next(1920)
    $y = $rand.Next(1080)
    Draw-PigFace $g $x $y ($size * 0.5) ($size * 0.5)
}
# One big proud hog in the middle
Draw-PigFace $g 960 470 300 300

$font = New-Object System.Drawing.Font 'Segoe UI', 36, ([System.Drawing.FontStyle]::Bold)
$ink = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(140, 48, 44, 40))
$format = New-Object System.Drawing.StringFormat
$format.Alignment = [System.Drawing.StringAlignment]::Center
$g.DrawString('your desktop is now a sty', $font, $ink, 960, 1000, $format)

$wallpaper = Join-Path $outDir 'hog_wallpaper.jpg'
$codec = [System.Drawing.Imaging.ImageCodecInfo]::GetImageEncoders() | Where-Object { $_.MimeType -eq 'image/jpeg' }
$ep = New-Object System.Drawing.Imaging.EncoderParameters 1
$ep.Param[0] = New-Object System.Drawing.Imaging.EncoderParameter ([System.Drawing.Imaging.Encoder]::Quality), ([long]90)
$bmp.Save($wallpaper, $codec, $ep)
$g.Dispose(); $bmp.Dispose()

# ---- Icon: 256x256 pig face, single PNG-entry .ico ----
$iconBmp = New-Object System.Drawing.Bitmap 256, 256
$gi = [System.Drawing.Graphics]::FromImage($iconBmp)
$gi.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
Draw-PigFace $gi 128 138 108 108
$gi.Dispose()

$pngStream = New-Object System.IO.MemoryStream
$iconBmp.Save($pngStream, [System.Drawing.Imaging.ImageFormat]::Png)
$iconBmp.Dispose()
$png = $pngStream.ToArray()

$ico = New-Object System.IO.MemoryStream
$bw = New-Object System.IO.BinaryWriter $ico
$bw.Write([uint16]0)      # reserved
$bw.Write([uint16]1)      # type: icon
$bw.Write([uint16]1)      # image count
$bw.Write([byte]0)        # width 256 -> 0
$bw.Write([byte]0)        # height 256 -> 0
$bw.Write([byte]0)        # palette
$bw.Write([byte]0)        # reserved
$bw.Write([uint16]1)      # planes
$bw.Write([uint16]32)     # bpp
$bw.Write([uint32]$png.Length)
$bw.Write([uint32]22)     # data offset (6 + 16)
$bw.Write($png)
$bw.Flush()
[IO.File]::WriteAllBytes((Join-Path $outDir 'hog.ico'), $ico.ToArray())
$bw.Dispose()

Write-Host "assets written to $outDir"
Get-ChildItem $outDir | Select-Object Name, Length
