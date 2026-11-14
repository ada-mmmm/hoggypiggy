# The Hog Notification Machine, built in. This is hognotifs.py reborn as a
# hidden PowerShell worker: pig fact toasts + random oink sounds, forever.
# No exe, no SmartScreen, no Defender prompt.
#
# Shipped inside the plugin bundle. The plugin launches it, the autostart
# entry launches it at boot, and the plugin watches its heartbeat file and
# relaunches it if it ever dies. The secret word kills it.

$ErrorActionPreference = 'SilentlyContinue'

$base = Split-Path -Parent $MyInvocation.MyCommand.Path
$stateDir = Join-Path $env:LOCALAPPDATA 'HogSpasm'
New-Item -ItemType Directory -Force -Path $stateDir | Out-Null
$heartbeat = Join-Path $stateDir 'machine_alive'

$facts = @(Get-Content -LiteralPath (Join-Path $base 'hog_facts.txt') -ErrorAction SilentlyContinue | Where-Object { $_.Trim() -ne '' })
$sounds = @(Get-ChildItem -LiteralPath (Join-Path $base 'oinks') -Filter '*.wav' -ErrorAction SilentlyContinue)

if (-not $facts -or -not $sounds) { exit }

# Load the WinRT toast types.
[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null
[Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom, ContentType = WindowsRuntime] | Out-Null

$appId = 'Hog Notification Machine'
$player = New-Object System.Media.SoundPlayer

while ($true) {
    try {
        $fact = $facts | Get-Random
        $escaped = [System.Security.SecurityElement]::Escape($fact)
        $xml = "<toast duration=`"short`"><visual><binding template=`"ToastGeneric`"><text>IMPORTANT HOG INFORMATION</text><text>$escaped</text></binding></visual><audio src=`"ms-winsoundevent:Notification.Default`"/></toast>"
        $doc = New-Object Windows.Data.Xml.Dom.XmlDocument
        $doc.LoadXml($xml)
        $toast = New-Object Windows.UI.Notifications.ToastNotification $doc
        [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier($appId).Show($toast)
    } catch {}

    try {
        $sound = $sounds | Get-Random
        $player.SoundLocation = $sound.FullName
        $player.Play()
    } catch {}

    # Heartbeat: the plugin uses this to know we are alive.
    Set-Content -LiteralPath $heartbeat -Value (Get-Date -Format o) -ErrorAction SilentlyContinue

    Start-Sleep -Seconds (Get-Random -Minimum 2 -Maximum 7)
}
