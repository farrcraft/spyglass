# Drives the spyglass viewer on Windows for checks by eye: start it from known settings, click,
# scroll, screenshot, stop. Windows PowerShell 5.1. See SKILL.md beside it.
#
#   viewer.ps1 start -Out <dir> [-Seed 12345] [-Version 26.3] [-Dimension overworld]
#                    [-CentreX 0] [-CentreZ 0] [-Scale 4] [-Surface] [-Relief] [-Unconfirmed]
#                    [-Structures minecraft:ancient_city,minecraft:igloo] [-Wait 6]
#   viewer.ps1 shot  -Out <file.png>
#   viewer.ps1 click -X <px> -Y <px>        (left click, screen pixels)
#   viewer.ps1 rclick -X <px> -Y <px>       (right click: the map's locate menu)
#   viewer.ps1 move  -X <px> -Y <px>        (hover: opens submenus)
#   viewer.ps1 wheel -X <px> -Y <px> [-Notches 3]   (negative scrolls up)
#   viewer.ps1 key -Keys n                  (SendKeys text, e.g. n, {ESC}; click the map first)
#   viewer.ps1 stop
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [ValidateSet('start', 'shot', 'click', 'rclick', 'move', 'wheel', 'key', 'stop')]
    [string]$Action,
    [string]$Out,
    [string]$Seed = '12345',
    [string]$Version = '26.3',
    [ValidateSet('overworld', 'the_nether', 'the_end')]
    [string]$Dimension = 'overworld',
    [double]$CentreX = 0,
    [double]$CentreZ = 0,
    [int]$X = 0,
    [int]$Y = 0,
    [int]$Scale = 4,
    [switch]$Surface,
    [switch]$Relief,
    [switch]$Unconfirmed,
    [string[]]$Structures = @(),
    [int]$Notches = 3,
    [string]$Keys,
    [int]$Wait = 6
)

$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path
$exe = Join-Path $repo 'target\dist\spyglass-viewer.exe'

Add-Type -AssemblyName System.Drawing
if (-not ('ViewerInput' -as [type])) {
    Add-Type @"
using System; using System.Runtime.InteropServices;
public class ViewerInput {
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(int f, int x, int y, int d, int e);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int a, out RECT r, int s);
    public struct RECT { public int L, T, R, B; }
}
"@
}

function Press([int]$down, [int]$up) {
    [ViewerInput]::SetCursorPos($X, $Y) | Out-Null
    Start-Sleep -Milliseconds 150
    [ViewerInput]::mouse_event($down, 0, 0, 0, 0)
    Start-Sleep -Milliseconds 60
    [ViewerInput]::mouse_event($up, 0, 0, 0, 0)
}

switch ($Action) {
    'start' {
        if (-not $Out) { throw 'start needs -Out, a folder for the settings file' }
        if (-not (Test-Path $exe)) { throw "no ${exe}: cargo build --profile dist -p spyglass-viewer" }
        New-Item -ItemType Directory -Force $Out | Out-Null
        $bundles = (Join-Path $repo 'bundles') -replace '\\', '/'
        $ids = ($Structures | ForEach-Object { '\"' + $_ + '\"' }) -join ','
        $flag = { param($b) if ($b) { 'true' } else { 'false' } }
        # The viewer's settings in RON, as eframe stores them; fields left out take defaults.
        $viewer = "(bundles:\`"$bundles\`",version:Some(\`"$Version\`"),seed:\`"$Seed\`"," +
            "dimension:\`"minecraft:$Dimension\`",view:(centre:($CentreX,$CentreZ),scale:$Scale,y:64)," +
            "views:{},ties:false,slime_chunks:false,spawn:true,confirm:$(& $flag (-not $Unconfirmed))," +
            "relief:$(& $flag $Relief),surface:$(& $flag $Surface),structures:[$ids],highlight:[])"
        $window = '(normal_geometry:(outer_position_pixels:Some((x:0.0,y:0.0)),scale_factor:1.0,' +
            'inner_size_points:(x:1400.0,y:900.0)),fullscreen:false,maximized:false)'
        $settings = Join-Path $Out 'viewer.ron'
        # A JSON object of RON strings, which is what eframe's settings file is.
        $json = '{"window": "' + $window + '", "spyglass": "' + $viewer + '"}'
        [System.IO.File]::WriteAllText($settings, $json, (New-Object System.Text.UTF8Encoding $false))
        Get-Process spyglass-viewer -ErrorAction SilentlyContinue | Stop-Process
        $env:SPYGLASS_VIEWER_SETTINGS = $settings
        Start-Process -FilePath $exe | Out-Null
        Start-Sleep -Seconds $Wait
        "started from $settings"
    }
    'shot' {
        if (-not $Out) { throw 'shot needs -Out, a .png path' }
        $p = Get-Process spyglass-viewer | Select-Object -First 1
        [ViewerInput]::SetForegroundWindow($p.MainWindowHandle) | Out-Null
        Start-Sleep -Milliseconds 500
        $r = New-Object ViewerInput+RECT
        # DWMWA_EXTENDED_FRAME_BOUNDS: the window without its invisible resize border.
        [ViewerInput]::DwmGetWindowAttribute($p.MainWindowHandle, 9, [ref]$r, 16) | Out-Null
        $bmp = New-Object System.Drawing.Bitmap ($r.R - $r.L), ($r.B - $r.T)
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
        $bmp.Save($Out)
        "saved $Out ($($r.R - $r.L) x $($r.B - $r.T), window at $($r.L),$($r.T))"
    }
    'click' { Press 2 4 }
    'rclick' { Press 8 16 }
    'move' {
        [ViewerInput]::SetCursorPos($X, $Y) | Out-Null
        Start-Sleep -Milliseconds 400
    }
    'wheel' {
        [ViewerInput]::SetCursorPos($X, $Y) | Out-Null
        Start-Sleep -Milliseconds 200
        for ($i = 0; $i -lt [Math]::Abs($Notches); $i++) {
            [ViewerInput]::mouse_event(0x0800, 0, 0, -120 * [Math]::Sign($Notches), 0)
            Start-Sleep -Milliseconds 150
        }
    }
    'key' {
        if (-not $Keys) { throw 'key needs -Keys, in SendKeys notation' }
        Add-Type -AssemblyName System.Windows.Forms
        $p = Get-Process spyglass-viewer | Select-Object -First 1
        [ViewerInput]::SetForegroundWindow($p.MainWindowHandle) | Out-Null
        Start-Sleep -Milliseconds 200
        [System.Windows.Forms.SendKeys]::SendWait($Keys)
        Start-Sleep -Milliseconds 300
    }
    'stop' { Get-Process spyglass-viewer -ErrorAction SilentlyContinue | Stop-Process }
}
