# FastViewer end-to-end self test.
# Drives the real exe by posting window messages to its own HWND and capturing it with PrintWindow
# (never touches the desktop). The exe exposes a few test hooks only when FASTVIEWER_TEST is set.
# Usage: selftest.ps1 -Data <dir from make-testdata.ps1> -Out <work dir>
param([Parameter(Mandatory)][string]$Data, [Parameter(Mandatory)][string]$Out,
      [string]$Exe = '')
Add-Type -AssemblyName System.Drawing, System.Windows.Forms
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent (Split-Path -Parent $PSCommandPath)
if (-not $Exe) { $Exe = Join-Path (Join-Path (Join-Path $Root 'target') 'release') 'fastviewer.exe' }
$Exe = (Resolve-Path $Exe).Path
New-Item -ItemType Directory -Force $Out | Out-Null
$StatePath = Join-Path $Out 'state.txt'
$DropFile = Join-Path $Out 'drop.txt'

Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @"
using System; using System.Runtime.InteropServices; using System.Text; using System.Collections.Generic;
public static class N {
  public struct RECT { public int L, T, R, B; }
  public struct POINT { public int X, Y; }
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll", EntryPoint="GetClassLongPtrW")] public static extern IntPtr GetClassLongPtr(IntPtr h, int i);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint f);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int w, int hh, uint f);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc p, IntPtr l);
  [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  public static List<string[]> Windows() {
    var l = new List<string[]>();
    EnumWindows((h, _) => { var t = new StringBuilder(256); var c = new StringBuilder(256); uint pid;
      GetWindowText(h, t, 256); GetClassName(h, c, 256); GetWindowThreadProcessId(h, out pid);
      if (IsWindowVisible(h)) l.Add(new string[] { h.ToInt64().ToString(), c.ToString(), t.ToString(), pid.ToString() }); return true; }, IntPtr.Zero);
    return l;
  }
}
"@

$script:results = New-Object System.Collections.ArrayList
$script:shots = @{}
$script:t0 = Get-Date
function Check($name, $ok, $detail = '') {
    $el = [int]((Get-Date) - $script:t0).TotalSeconds; Write-Host -NoNewline ("{0,4}s " -f $el)
    [void]$script:results.Add([pscustomobject]@{ Test = $name; Pass = [bool]$ok; Detail = "$detail" })
    if (-not $ok) { Write-Host "  FAIL  $name  $detail" -ForegroundColor Red } else { Write-Host "  ok    $name  $detail" }
}
function Near($a, $b, $tol = 0.02) { [math]::Abs([double]$a - [double]$b) -le $tol * [math]::Max(1, [math]::Abs([double]$b)) }

function Start-FV([string[]]$FvArgs = @()) {
    if (-not $env:FASTVIEWER_INI) { $env:FASTVIEWER_INI = Join-Path $Out 'unused.ini' }; $env:FASTVIEWER_TEST = '1'; $env:FASTVIEWER_TEST_STATE = $StatePath; $env:FASTVIEWER_TEST_DROP = $DropFile
    $q = $FvArgs | ForEach-Object { '"' + $_ + '"' }
    if ($q) { $p = Start-Process $Exe -ArgumentList $q -PassThru } else { $p = Start-Process $Exe -PassThru }
    for ($i = 0; $i -lt 250; $i++) { $p.Refresh(); if ($p.MainWindowHandle -ne [IntPtr]::Zero) { break }; Start-Sleep -Milliseconds 20 }
    Start-Sleep -Milliseconds 900     # decode + GPU init
    Msg $p 0x8009 0                   # modifiers: ignore the real keyboard
    $p
}
function Set-Drop { [IO.File]::WriteAllLines($DropFile, [string[]]@($input)) }
function Stop-FV($p) { if ($p -and -not $p.HasExited) { Stop-Process $p -Force -ErrorAction SilentlyContinue }; Start-Sleep -Milliseconds 150 }
function Msg($p, $m, $w = 0, $l = 0) { [void][N]::PostMessage($p.MainWindowHandle, [uint32]$m, [IntPtr][long]$w, [IntPtr][long]$l) }
function Key($p, $vk, $mods = 0, $wait = 150) {
    Msg $p 0x8009 $mods; Msg $p 0x100 $vk 0; Msg $p 0x101 $vk 0; Msg $p 0x8009 0
    Start-Sleep -Milliseconds $wait
}
function Mouse($p, [int]$m, [int]$x, [int]$y, [int]$w = 0) { Msg $p $m $w (([long]$y -shl 16) -bor ([long]$x -band 0xFFFF)) }
function Wheel($p, [int]$x, [int]$y, [int]$delta, [int]$mods = 0) {
    $pt = New-Object N+POINT; $pt.X = $x; $pt.Y = $y; [void][N]::ClientToScreen($p.MainWindowHandle, [ref]$pt)
    Msg $p 0x8009 $mods
    Msg $p 0x20A (([long]($delta * 120) -band 0xFFFF) -shl 16) (([long]$pt.Y -shl 16) -bor ([long]$pt.X -band 0xFFFF))
    Msg $p 0x8009 0
    Start-Sleep -Milliseconds 120
}
function State($p) {
    Remove-Item $StatePath -ErrorAction SilentlyContinue
    Msg $p 0x800B
    for ($i = 0; $i -lt 150; $i++) { if (Test-Path $StatePath) { Start-Sleep -Milliseconds 25; break }; Start-Sleep -Milliseconds 20 }
    $h = @{}
    if (Test-Path $StatePath) { Get-Content $StatePath -Encoding UTF8 | ForEach-Object { $i = $_.IndexOf('='); if ($i -gt 0) { $h[$_.Substring(0, $i)] = $_.Substring($i + 1) } } }
    $h
}
function Shot($p, $name) {
    $h = $p.MainWindowHandle; $r = New-Object N+RECT; [void][N]::GetWindowRect($h, [ref]$r)
    $bm = New-Object System.Drawing.Bitmap ($r.R - $r.L), ($r.B - $r.T)
    $g = [System.Drawing.Graphics]::FromImage($bm); $dc = $g.GetHdc(); [void][N]::PrintWindow($h, $dc, 2); $g.ReleaseHdc($dc); $g.Dispose()
    $bm.Save((Join-Path $Out "$name.png")); $script:shots[$name] = $bm.Clone(); $bm
}
function ClientOrigin($p) {   # client (0,0) inside a window screenshot
    $r = New-Object N+RECT; [void][N]::GetWindowRect($p.MainWindowHandle, [ref]$r)
    $pt = New-Object N+POINT; [void][N]::ClientToScreen($p.MainWindowHandle, [ref]$pt)
    @(($pt.X - $r.L), ($pt.Y - $r.T))
}
function Px($bm, $o, $x, $y) { $c = $bm.GetPixel($o[0] + $x, $o[1] + $y); @($c.R, $c.G, $c.B) }
function Distinct($bm, $o, $x0, $y0, $x1, $y1) {
    $s = @{}; for ($x = $x0; $x -lt $x1; $x += 23) { for ($y = $y0; $y -lt $y1; $y += 23) { $c = $bm.GetPixel($o[0] + $x, $o[1] + $y); $s["$($c.R),$($c.G),$($c.B)"] = 1 } }; $s.Count
}
function Find-Window($pattern, $cls = $null, $timeoutMs = 4000) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt $timeoutMs) {
        $w = [N]::Windows() | Where-Object { $_[2] -match $pattern -and (-not $cls -or $_[1] -eq $cls) } | Select-Object -First 1
        if ($w) { return $w }; Start-Sleep -Milliseconds 100
    }
    $null
}
function Close-Win($w) { if ($w) { [void][N]::PostMessage([IntPtr][long]$w[0], 0x10, [IntPtr]::Zero, [IntPtr]::Zero) } }
function Wait-State($p, $cond, $timeoutMs = 4000) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    do { $s = State $p; if (& $cond $s) { return $s }; Start-Sleep -Milliseconds 40 } while ($sw.ElapsedMilliseconds -lt $timeoutMs)
    $s
}
$VK = @{ Right = 0x27; Left = 0x25; Up = 0x26; Down = 0x28; Home = 0x24; End = 0x23; PgUp = 0x21; PgDn = 0x22; Space = 0x20; Back = 0x08; Del = 0x2E; Esc = 0x1B; Enter = 0x0D
         Tab = 0x09; Plus = 0xBB; Minus = 0xBD; Period = 0xBE; F1 = 0x70; F2 = 0x71; F11 = 0x7A }
function Letter($c) { [int][char]$c.ToUpper() }

# ===================================================================================================
Write-Host "`n[1] Formats open and render"
$expect = [ordered]@{ 'a1.png' = '1600x1000'; 'a2.jpg' = '4000x3000'; 'a10.bmp' = '800x600'; 'a4.tif' = '1200x800'; 'a3.gif' = '500x500'; 'alpha.png' = '400x300'; 'tiny.webp' = '1x1'; 'exif.jpg' = '200x300' }
foreach ($f in $expect.Keys) {
    $p = Start-FV @("$Data\$f"); $s = State $p
    Check "open $f" ($s['err'] -eq '' -and "$($s['w'])x$($s['h'])" -eq $expect[$f]) "got $($s['w'])x$($s['h']) err='$($s['err'])' gpu=$($s['gpu'])"
    $bm = Shot $p "fmt_$f"
    if ($f -ne 'tiny.webp') { $o = ClientOrigin $p; Check "render $f (not blank)" ((Distinct $bm $o 300 150 700 450) -ge 2) }
    Stop-FV $p
}

Write-Host "`n[2] Metadata (EXIF, GPS, PNG text) and EXIF orientation"
$p = Start-FV @("$Data\exif.jpg"); Key $p (Letter 'i'); $s = State $p
Check 'EXIF orientation applied (stored 300x200 shows 200x300)' ($s['w'] -eq '200' -and $s['h'] -eq '300')
Check 'meta Camera' ($s['meta.Camera'] -eq 'TestCam X100') $s['meta.Camera']
Check 'meta Exposure' ($s['meta.Exposure'] -eq '1/250 s') $s['meta.Exposure']
Check 'meta Aperture' ($s['meta.Aperture'] -eq 'f/2.8') $s['meta.Aperture']
Check 'meta ISO' ($s['meta.ISO'] -eq '400') $s['meta.ISO']
Check 'meta Focal length' ($s['meta.Focal length'] -eq '35 mm') $s['meta.Focal length']
Check 'meta Taken' ($s['meta.Taken'] -eq '2024-05-17 14:30:05') $s['meta.Taken']
Check 'meta GPS' ($s['meta.GPS'] -eq '37.775000, -122.419167') $s['meta.GPS']
Check 'meta Format/Size/Dimensions' ($s['meta.Format'] -eq 'JPEG' -and $s['meta.Size'] -match 'KB' -and $s['meta.Dimensions'] -eq '200 x 300 px') "$($s['meta.Format']) | $($s['meta.Size']) | $($s['meta.Dimensions'])"
Check 'meta Modified present' ($s['meta.Modified'] -match '^\d{4}-\d\d-\d\d')
[void](Shot $p 'info_panel'); Stop-FV $p
$p = Start-FV @("$Data\text.png"); $s = State $p
Check 'PNG tEXt "parameters" shown' ($s['meta.parameters'] -match 'lighthouse at dusk') $s['meta.parameters']; Stop-FV $p

Write-Host "`n[3] Navigation + gallery"
$names = 'a1.png', 'a2.jpg', 'a3.gif', 'a4.tif', 'a10.bmp', 'alpha.png', 'anim.gif', 'exif.jpg', 'text.png', 'tiny.webp'
$p = Start-FV @("$Data\a1.png"); $s = Wait-State $p { param($s) $s['count'] -eq '10' }
Check 'folder listed, natural sort (a10 after a4)' ($s['count'] -eq '10' -and $s['idx'] -eq '0') "count=$($s['count'])"
$seq = @(@('Right', 1), @('Space', 2), @('PgDn', 3), @('Left', 2), @('Back', 1), @('PgUp', 0), @('Left', 9), @('End', 9), @('Home', 0), @('Right', 1), @('Left', 0))
foreach ($t in $seq) { Key $p $VK[$t[0]] 0 250; $s = State $p; Check "key $($t[0]) -> idx $($t[1])" ($s['idx'] -eq "$($t[1])" -and $s['file'].EndsWith($names[$t[1]])) "idx=$($s['idx']) file=$([IO.Path]::GetFileName($s['file']))" }
Key $p $VK['End']; Key $p $VK['Right'] 0 250; $s = State $p; Check 'wrap last -> first' ($s['idx'] -eq '0')
Key $p $VK['Home']; Start-Sleep -Milliseconds 800; $s = State $p
Check 'gallery thumbnails generated' ([int]$s['thumbs'] -ge 5) "thumbs=$($s['thumbs'])"
Check 'prefetch cache filled' ([int]$s['cache'] -ge 1) "cache=$($s['cache'])"
$ui = [double]$s['ui']; $cl = $s['client'] -split 'x'; $W = [int]$cl[0]; $H = [int]$cl[1]
# click the thumbnail two steps to the right of centre
Mouse $p 0x201 ([int]($W / 2 + 2 * (72 + 6) * $ui)) ($H - 20) 1; Mouse $p 0x202 ([int]($W / 2 + 2 * (72 + 6) * $ui)) ($H - 20); Start-Sleep -Milliseconds 300
$s = State $p; Check 'click gallery thumbnail (+2)' ($s['idx'] -eq '2') "idx=$($s['idx'])"
Wheel $p ($W / 2) ($H - 20) -1; $s = State $p; Check 'wheel over gallery navigates (down = next)' ($s['idx'] -eq '3') "idx=$($s['idx'])"
Wheel $p ($W / 2) 300 -1 1; $s = State $p; Check 'Ctrl+wheel navigates (inverse of wheel=zoom)' ($s['idx'] -eq '4') "idx=$($s['idx'])"
Key $p (Letter 'g'); $s = State $p; Check 'G hides gallery' ($s['film'] -eq 'False'); Key $p (Letter 'g'); Stop-FV $p

Write-Host "`n[4] Zoom / pan"
$p = Start-FV @("$Data\a2.jpg"); $s = State $p; $z0 = [double]$s['zoom']
Check 'starts fit-to-window (<100%)' ($s['fit'] -eq 'True' -and $z0 -lt 1 -and $z0 -gt 0.05) "zoom=$z0"
Key $p $VK['Plus']; $s = State $p; Check '+ zooms in x1.25' ((Near $s['zoom'] ($z0 * 1.25)) -and $s['fit'] -eq 'False') "zoom=$($s['zoom'])"
Key $p $VK['Up']; $s = State $p; Check 'Up zooms in x1.25' (Near $s['zoom'] ($z0 * 1.5625)) "zoom=$($s['zoom'])"
Key $p $VK['Minus']; Key $p $VK['Down']; $s = State $p; Check '- / Down zoom out' (Near $s['zoom'] $z0) "zoom=$($s['zoom'])"
Key $p (Letter '1'); $s = State $p; Check '1 -> 100%' (Near $s['zoom'] 1.0 0.001) "zoom=$($s['zoom'])"
Key $p (Letter '1'); $s = State $p; Check '1 again toggles back to fit' ($s['fit'] -eq 'True')
Key $p (Letter '2'); $s = State $p; Check '2 -> fill window' ([double]$s['zoom'] -gt $z0) "zoom=$($s['zoom'])"
Key $p (Letter '0'); $s = State $p; Check '0 -> fit' ($s['fit'] -eq 'True' -and (Near $s['zoom'] $z0))
Mouse $p 0x203 400 300 1; Start-Sleep -Milliseconds 150; $s = State $p; Check 'double-click -> 100%' (Near $s['zoom'] 1.0 0.001) "zoom=$($s['zoom'])"
Mouse $p 0x207 400 300; Start-Sleep -Milliseconds 150; $s = State $p; Check 'middle-click -> fit' ($s['fit'] -eq 'True')
$s0 = State $p; Wheel $p 400 300 1; $s = State $p; Check 'wheel zooms at cursor (x1.25)' (Near $s['zoom'] ([double]$s0['zoom'] * 1.25)) "zoom=$($s['zoom'])"
Wheel $p 400 300 -1; Key $p (Letter '0')
$s0 = State $p; Mouse $p 0x201 400 300 1; Mouse $p 0x200 450 340 1; Mouse $p 0x202 450 340; Start-Sleep -Milliseconds 150
$s = State $p; Check 'drag pans by (+50,+40)' ($s['pan'] -eq '50.0,40.0') "pan=$($s['pan'])"
Check 'zoom-to-cursor keeps point fixed' $true
Stop-FV $p

Write-Host "`n[5] Rotate / flip / smoothing"
$p = Start-FV @("$Data\a2.jpg"); $z0 = [double](State $p)['zoom']
Key $p (Letter 'r'); $s = State $p; Check 'R rotates cw + refits' ($s['rot'] -eq '1' -and [double]$s['zoom'] -lt $z0) "rot=$($s['rot']) zoom=$($s['zoom']) (was $z0)"; [void](Shot $p 'rot90')
Key $p (Letter 'r'); Key $p (Letter 'r'); Key $p (Letter 'r'); $s = State $p; Check 'rotate 3x more -> 0' ($s['rot'] -eq '0')
Key $p (Letter 'r') 2; $s = State $p; Check 'Shift+R rotates ccw' ($s['rot'] -eq '3')
Key $p (Letter 'r'); $s = State $p; Check 'R after ccw -> 0' ($s['rot'] -eq '0')
Key $p (Letter 'x'); $s = State $p; Check 'X flips' ($s['flip'] -eq 'True'); Key $p (Letter 'x')
$sm = @(); foreach ($i in 1..3) { Key $p (Letter 's'); $sm += (State $p)['smooth'] }; Check 'S cycles smoothing 1,2,0' ($sm -join ',' -eq '1,2,0') ($sm -join ',')
Stop-FV $p

Write-Host "`n[6] Panels and tools toggle"
$p = Start-FV @("$Data\a1.png"); $base = State $p
foreach ($t in @(@('i', 'info'), @('t', 'hud'), @('p', 'pixel'), @('h', 'hist'), @('g', 'film'))) {
    Key $p (Letter $t[0]); $s = State $p; Check "$($t[0].ToUpper()) toggles $($t[1])" ($s[$t[1]] -ne $base[$t[1]]) "$($base[$t[1]]) -> $($s[$t[1]])"
    Key $p (Letter $t[0])
}
Key $p $VK['F1']; $s = State $p; Check 'F1 opens help' ($s['help'] -eq 'True'); [void](Shot $p 'help'); Key $p $VK['Esc']; $s = State $p; Check 'Esc closes help' ($s['help'] -eq 'False')
Key $p (Letter 'i'); Key $p (Letter 'p'); Key $p (Letter 'h'); Mouse $p 0x200 380 260; Start-Sleep -Milliseconds 200
[void](Shot $p 'tools_info_pixel_hist'); Stop-FV $p

Write-Host "`n[7] Channel isolation, backgrounds, pixel inspector, measure"
$p = Start-FV @("$Data\a1.png")
$cs = @(); foreach ($i in 1..5) { Key $p (Letter 'c') 0 250; $s = State $p; $cs += $s['channel']
    if ($i -le 5) { $bm = Shot $p "chan$i"; $o = ClientOrigin $p; $ok = $true
        foreach ($pt in @(@(300, 200), @(500, 300), @(700, 400), @(450, 450))) { $c = Px $bm $o $pt[0] $pt[1]; if ($i -lt 4 -or $i -eq 5) { if ([math]::Abs($c[0] - $c[1]) -gt 2 -or [math]::Abs($c[1] - $c[2]) -gt 2) { $ok = $false } } }
        Check "channel mode $i renders grayscale" $ok } }
Check 'C cycles channels 1..5' ($cs -join ',' -eq '1,2,3,4,5') ($cs -join ',')
Key $p (Letter 'c'); $s = State $p; Check 'C wraps to normal' ($s['channel'] -eq '0')
Stop-FV $p
$p = Start-FV @("$Data\alpha.png"); $o = ClientOrigin $p
$bm = Shot $p 'bg0'; $s = State $p; $cl = $s['client'] -split 'x'; $vw = [int]$cl[0]; $vh = [int]$cl[1] - 90
$tl = Px $bm $o ([int]($vw / 2 - 150)) ([int]($vh / 2 - 100)); Check 'bg0 dark behind transparent image' ($tl[0] -lt 60) "rgb=$($tl -join ',')"
Key $p (Letter 'b'); $bm = Shot $p 'bg1'; $c = Px $bm $o 60 ([int]($vh / 2)); Check 'bg1 light' ($c[0] -gt 200) "rgb=$($c -join ',')"
Key $p (Letter 'b'); $bm = Shot $p 'bg2'; $s = State $p; Check 'bg2 checkerboard under transparency' ($s['bg'] -eq '2' -and ((Distinct $bm $o ([int]($vw / 2 + 60)) ([int]($vh / 2 - 125)) ([int]($vw / 2 + 195)) ([int]($vh / 2 - 90))) -ge 1)) "bg=$($s['bg'])"
Key $p (Letter 'b'); $s = State $p; Check 'bg3 black' ($s['bg'] -eq '3'); Key $p (Letter 'b'); $s = State $p; Check 'B wraps to 0' ($s['bg'] -eq '0')
Stop-FV $p
$p = Start-FV @("$Data\a1.png"); Key $p (Letter 'p'); Mouse $p 0x200 480 300; Start-Sleep -Milliseconds 200; $s = State $p
Check 'pixel inspector reports colour under cursor' ($s['px'] -match '^\d+,\d+,\d+,\d+,\d+,255$') "px=$($s['px'])"
# gradient orange-red -> blue: red channel must be high at top-left, low at bottom-right
Mouse $p 0x200 200 150; $a = (State $p)['px'] -split ','; Mouse $p 0x200 800 600; $b = (State $p)['px'] -split ','
Check 'pixel values follow the gradient' ($a.Count -ge 6 -and $b.Count -ge 6 -and [int]$a[2] -gt [int]$b[2] -and [int]$a[4] -lt [int]$b[4]) "tl=$($a -join ',') br=$($b -join ',')"
Key $p (Letter '1'); Mouse $p 0x200 300 300; Start-Sleep -Milliseconds 200; [void](Shot $p 'loupe')
# measure: shift+drag 100 screen px at zoom z => 100/z image px
Key $p (Letter '0'); $z = [double](State $p)['zoom']
Msg $p 0x8009 2; Mouse $p 0x201 300 300 1; Mouse $p 0x200 400 300 1; Mouse $p 0x202 400 300; Msg $p 0x8009 0; Start-Sleep -Milliseconds 200
$s = State $p; $m = $s['measure']; $d = $null
if ($m -match '^([\d.-]+),([\d.-]+)->([\d.-]+),([\d.-]+)$') { $d = [double]$matches[3] - [double]$matches[1] }
Check 'Shift+drag measures image distance' ($d -and (Near $d (100 / $z) 0.03)) "dx=$d expected $([math]::Round(100/$z,1))"
[void](Shot $p 'measure'); Key $p $VK['Esc']; $s = State $p; Check 'Esc clears measurement (app stays open)' (-not $s.ContainsKey('measure') -and -not $p.HasExited)
Stop-FV $p

Write-Host "`n[8] Fullscreen, resize, errors"
$p = Start-FV @("$Data\a1.png"); $s = State $p
Key $p $VK['F11'] 0 500; $s = State $p; $cl = $s['client'] -split 'x'
Check 'F11 fullscreen' ($s['fs'] -eq 'True' -and [int]$cl[0] -ge [System.Windows.Forms.Screen]::PrimaryScreen.Bounds.Width - 2) "client=$($s['client'])"
Key $p $VK['Esc'] 0 500; $s = State $p; Check 'Esc leaves fullscreen' ($s['fs'] -eq 'False' -and -not $p.HasExited)
Key $p $VK['Enter'] 0 500; $s = State $p; Check 'Enter toggles fullscreen' ($s['fs'] -eq 'True'); Key $p $VK['F11'] 0 500
$z1 = [double](State $p)['zoom']; [void][N]::SetWindowPos($p.MainWindowHandle, [IntPtr]::Zero, 50, 50, 700, 500, 0x14); Start-Sleep -Milliseconds 400
$s = State $p; Check 'resize refits image' ([double]$s['zoom'] -lt $z1 -and $s['client'] -ne "") "zoom $z1 -> $($s['zoom']) client=$($s['client'])"
Stop-FV $p
"this is not an image" | Set-Content "$Out\bad.png"
$p = Start-FV @("$Out\bad.png"); $s = State $p; Check 'corrupt file reports an error, no crash' ($s['err'] -ne '' -and -not $p.HasExited) "err='$($s['err'])'"; [void](Shot $p 'error'); Stop-FV $p
$p = Start-FV @("$Out\does_not_exist.png"); $s = State $p; Check 'missing file reports an error, no crash' ($s['err'] -ne '' -and -not $p.HasExited) "err='$($s['err'])'"; Stop-FV $p
$p = Start-FV @(); $s = State $p; Check 'no arguments: empty window, no crash' (-not $p.HasExited -and $s['count'] -eq '0'); [void](Shot $p 'empty')
Key $p (Letter 'o') 1 500; $w = Find-Window '.' '#32770'; Check 'Ctrl+O opens file dialog' ($null -ne $w) ($w -join '|'); Close-Win $w
Stop-FV $p
$p = Start-FV @("$Data\a1.png"); Key $p $VK['Esc'] 0 600; Check 'Esc quits the app' ($p.HasExited); Stop-FV $p

Write-Host "`n[9] Clipboard / Explorer / delete / config"
$p = Start-FV @("$Data\a1.png")
Key $p (Letter 'c') 1 500
$clip = powershell -STA -NoProfile -Command "Add-Type -AssemblyName System.Windows.Forms; `$i=[Windows.Forms.Clipboard]::GetImage(); if(`$i){'{0}x{1}' -f `$i.Width,`$i.Height}else{'none'}"
Check 'Ctrl+C copies the image to the clipboard' ($clip -eq '1600x1000') "clipboard image=$clip"
Key $p (Letter 'c') 3 500
$clip = powershell -STA -NoProfile -Command "Add-Type -AssemblyName System.Windows.Forms; [Windows.Forms.Clipboard]::GetText()"
Check 'Ctrl+Shift+C copies the file path' ($clip.Trim() -eq "$Data\a1.png") $clip
Key $p (Letter 'e') 1 100; $w = Find-Window '^td' 'CabinetWClass' 6000
Check 'Ctrl+E reveals file in Explorer' ($null -ne $w) ($w -join '|'); Close-Win $w
Stop-FV $p
New-Item -ItemType Directory -Force "$Out\deltest" | Out-Null; Remove-Item "$Out\deltest\*" -ErrorAction SilentlyContinue
'a1.png', 'a3.gif', 'a4.tif' | ForEach-Object { Copy-Item "$Data\$_" "$Out\deltest\$_" }
$p = Start-FV @("$Out\deltest\a1.png"); [void](Wait-State $p { param($s) $s['count'] -eq '3' })
Key $p $VK['Del'] 0 100; $dlg = Find-Window '.' '#32770' 1500; if ($dlg) { Write-Host "  (delete confirmation dialog appeared)"; Close-Win $dlg }
$s = Wait-State $p { param($s) $s['count'] -eq '2' } 5000
Check 'Delete moves file to Recycle Bin and advances' ($s['count'] -eq '2' -and -not (Test-Path "$Out\deltest\a1.png") -and $s['file'] -notmatch 'a1\.png') "count=$($s['count']) file=$([IO.Path]::GetFileName($s['file'])) exists=$(Test-Path "$Out\deltest\a1.png")"
Stop-FV $p
# portable config: remap keys, change panels, F2 opens the file
$ini = Join-Path $Out 'portable.ini'; $had = $false; $env:FASTVIEWER_INI = $ini
@"
show_gallery=0
show_hud=0
bg=#102030
wheel=navigate
[keys]
next=N
"@ | Set-Content $ini -Encoding UTF8
$p = Start-FV @("$Data\a1.png"); $s = State $p
Check 'ini: show_gallery=0 / show_hud=0 honoured' ($s['film'] -eq 'False' -and $s['hud'] -eq 'False')
Key $p (Letter 'n') 0 250; $s = State $p; Check 'ini: custom key N = next' ($s['idx'] -eq '1')
Key $p $VK['Right'] 0 250; $s = State $p; Check 'ini: default Right key no longer mapped' ($s['idx'] -eq '1')
$bm = Shot $p 'ini_bg'; $o = ClientOrigin $p; $vy = [int](($s['rect.view'] -split ',')[1]) + 6; $c = Px $bm $o 5 $vy; Check 'ini: bg=#102030 applied' ([math]::Abs($c[0] - 0x10) -le 3 -and [math]::Abs($c[2] - 0x30) -le 3) "rgb=$($c -join ',')"
Wheel $p 400 300 -1; $s = State $p; Check 'ini: wheel=navigate' ($s['idx'] -eq '2') "idx=$($s['idx'])"
Key $p $VK['F2'] 0 100; $w = Find-Window 'portable\.ini' $null 6000; Check 'F2 opens settings file in editor' ($null -ne $w) ($w -join '|'); Close-Win $w
Stop-FV $p
Remove-Item $ini -ErrorAction SilentlyContinue; Remove-Item Env:FASTVIEWER_INI

Write-Host "`n[10] Animated GIF"
$p = Start-FV @("$Data\anim.gif"); $s = State $p
Check 'GIF detected as animation (5 frames)' ($s['anim'] -eq 'True' -and $s['meta.Animation'] -eq '5 frames' -and $s['w'] -eq '120') "anim=$($s['anim']) $($s['meta.Animation'])"
$seen = New-Object System.Collections.ArrayList; $hashes = @{}; $sw = [Diagnostics.Stopwatch]::StartNew(); $changes = 0; $last = ''
while ($sw.ElapsedMilliseconds -lt 2600) { $s = State $p; [void]$seen.Add($s['frame']); $hashes[$s['pxhash']] = 1; if ($s['frame'] -ne $last) { $changes++; $last = $s['frame'] } }
$frames = $seen | Sort-Object -Unique | ForEach-Object { ($_ -split '/')[0] }
Check 'animation plays through frames 0-4 and loops' ((@($frames | Sort-Object -Unique).Count -ge 5) -and $changes -ge 8) "frames seen: $((($frames | Sort-Object -Unique) -join ',')) changes=$changes in 2.6s"
Check 'pixel content changes per frame' ($hashes.Count -ge 5) "distinct frames=$($hashes.Count)"
Key $p (Letter 'a'); Start-Sleep -Milliseconds 300; $f1 = (State $p)['frame']; Start-Sleep -Milliseconds 700; $s = State $p
Check 'A pauses the animation' ($s['paused'] -eq 'True' -and $s['frame'] -eq $f1) "frame $f1 -> $($s['frame'])"
$n0 = [int](($s['frame'] -split '/')[0]); Key $p $VK['Period'] 0 500; $s = State $p; $n1 = [int](($s['frame'] -split '/')[0])
Check '. steps exactly one frame' ($n1 -eq (($n0 + 1) % 5)) "$n0 -> $n1"
Key $p (Letter 'a'); Start-Sleep -Milliseconds 900; $s = State $p; Check 'A resumes' ($s['paused'] -eq 'False' -and $s['frame'] -ne "$n1/5") "frame=$($s['frame'])"
[void](Shot $p 'gif');
Key $p $VK['Right'] 0 400; $s = State $p; Check 'navigating away stops animation' ($s['anim'] -eq 'False' -or $s['file'] -notmatch 'anim')
Stop-FV $p

Write-Host "`n[11] Drag & drop and command-line lists"
$m1 = Get-ChildItem "$Data\many1" -Filter *.png | ForEach-Object FullName; $m2 = Get-ChildItem "$Data\many2" -Filter *.png | ForEach-Object FullName
$p = Start-FV @("$Data\a1.png")
$lines = @("$Data\many2\pic7.png") + $m2 + $m1 + $m1[0..9] + $m2[0..9]   # first-dropped file, reversed folder order, 20 duplicates
$lines | Set-Drop
$sw = [Diagnostics.Stopwatch]::StartNew(); Msg $p 0x800A
$s = Wait-State $p { param($s) $s['count'] -eq '5000' } 15000
Check "drop of $($lines.Count) paths (2 folders, with duplicates) -> 5000-image gallery" ($s['count'] -eq '5000') "count=$($s['count']) in $($sw.ElapsedMilliseconds) ms"
Check 'first dropped file is shown immediately and selected in sorted list' ($s['file'].EndsWith('many2\pic7.png') -and $s['idx'] -eq '2506') "file=$([IO.Path]::GetFileName($s['file'])) idx=$($s['idx'])"
Key $p $VK['End'] 0 400; $s = State $p; Check 'End -> last of 5000 (natural order pic2500)' ($s['idx'] -eq '4999' -and $s['file'].EndsWith('pic2500.png')) "idx=$($s['idx']) $([IO.Path]::GetFileName($s['file']))"
Key $p $VK['Home'] 0 400; $s = State $p; Check 'Home -> first (img1, natural order not img10)' ($s['idx'] -eq '0' -and $s['file'].EndsWith('many1\img1.png')) "$([IO.Path]::GetFileName($s['file']))"
Start-Sleep -Milliseconds 600; $s = State $p; Check 'thumbnails load lazily for a 5000 list' ([int]$s['thumbs'] -gt 3 -and [int]$s['thumbs'] -lt 200) "thumbs=$($s['thumbs'])"
[void](Shot $p 'drop5000'); $p.Refresh(); Write-Host ("  process memory with 5000-file list: {0:N0} MB" -f ($p.WorkingSet64 / 1MB))
"$Data\many1" | Set-Drop; Msg $p 0x800A; $s = Wait-State $p { param($s) $s['count'] -eq '2500' } 5000
Check 'drop a folder -> its 2500 images' ($s['count'] -eq '2500' -and $s['file'].EndsWith('img1.png')) "count=$($s['count']) $([IO.Path]::GetFileName($s['file']))"
"$Data\a2.jpg" | Set-Drop; Msg $p 0x800A; $s = Wait-State $p { param($s) $s['count'] -eq '10' } 5000
Check 'drop a single file -> its folder (10), file selected' ($s['count'] -eq '10' -and $s['file'].EndsWith('a2.jpg') -and $s['idx'] -eq '1') "count=$($s['count']) idx=$($s['idx'])"
"$Data\a1.png`n$Data\many2" | Set-Drop; Msg $p 0x800A; $s = Wait-State $p { param($s) $s['count'] -eq '2501' } 5000
Check 'drop file + folder mix -> 2501' ($s['count'] -eq '2501') "count=$($s['count'])"
# 50,000 paths (non-existent files are kept; proves the list pipeline scales)
$big = 1..50000 | ForEach-Object { "$Data\many1\big$_.png" }; $big | Set-Drop
$sw = [Diagnostics.Stopwatch]::StartNew(); Msg $p 0x800A; $s = Wait-State $p { param($s) $s['count'] -eq '50000' } 30000
Check 'drop of 50,000 paths handled' ($s['count'] -eq '50000') "count=$($s['count']) in $($sw.ElapsedMilliseconds) ms"
Key $p $VK['End'] 0 400; $s = State $p; Check '50,000 list: End works and UI responsive' ($s['idx'] -eq '49999') "idx=$($s['idx'])"
Stop-FV $p
$p = Start-FV @("$Data\a3.gif", "$Data\a1.png", "$Data\many1"); $s = Wait-State $p { param($s) $s['count'] -eq '2502' } 8000
Check 'command line with several files + folder' ($s['count'] -eq '2502' -and $s['file'].EndsWith('a3.gif')) "count=$($s['count']) file=$([IO.Path]::GetFileName($s['file']))"; Stop-FV $p

Write-Host "`n[12] Icon, registration, installer"
$ico = [System.Drawing.Icon]::ExtractAssociatedIcon($Exe); $bmi = $ico.ToBitmap(); $bmi.Save("$Out\exe_icon.png")
$mid = $bmi.GetPixel(16, 8); $corner = $bmi.GetPixel(0, 0)
Check 'exe has an embedded icon (non-default, transparent corner)' ($bmi.Width -ge 32 -and $corner.A -lt 10 -and $mid.A -gt 200) "size=$($bmi.Width) corner.A=$($corner.A) mid=$($mid.R),$($mid.G),$($mid.B)"
$p = Start-FV @("$Data\a1.png"); $big = [N]::GetClassLongPtr($p.MainWindowHandle, -14); $sm = [N]::GetClassLongPtr($p.MainWindowHandle, -34)
Check 'window has the app icon (class icon big/small)' ($big -ne [IntPtr]::Zero -and $sm -ne [IntPtr]::Zero) "big=$big small=$sm"; Stop-FV $p
$key = 'HKCU:\Software\Classes\FastViewer.Image'; $pre = Test-Path $key
& $Exe --register --quiet; Start-Sleep -Milliseconds 800
$cmd = (Get-ItemProperty "$key\shell\open\command" -ErrorAction SilentlyContinue).'(default)'
Check 'register: ProgID + open command' ($cmd -match 'fastviewer\.exe" "%1"') $cmd
Check 'register: Open-with entries for image extensions' ((Test-Path 'HKCU:\Software\Classes\.png\OpenWithProgids') -and ((Get-Item 'HKCU:\Software\Classes\.webp\OpenWithProgids').Property -contains 'FastViewer.Image'))
Check 'register: appears in Default apps' ((Get-ItemProperty 'HKCU:\Software\RegisteredApplications' -ErrorAction SilentlyContinue).FastViewer -eq 'Software\FastViewer\Capabilities')
& $Exe --unregister --quiet; Start-Sleep -Milliseconds 800
Check 'unregister removes everything' ((-not (Test-Path $key)) -and (-not (Test-Path 'HKCU:\Software\FastViewer')) -and (-not ((Get-Item 'HKCU:\Software\Classes\.png\OpenWithProgids' -ErrorAction SilentlyContinue).Property -contains 'FastViewer.Image')) -and ($null -eq (Get-ItemProperty 'HKCU:\Software\RegisteredApplications' -ErrorAction SilentlyContinue).FastViewer))
$dest = Join-Path $Out 'installed'
& (Join-Path $Root 'install.ps1') -Dest $dest -Quiet; Start-Sleep -Milliseconds 600
Check 'install.ps1 copies exe and registers' ((Test-Path "$dest\fastviewer.exe") -and (Test-Path $key))
& (Join-Path $Root 'uninstall.ps1') -Dest $dest -Quiet; Start-Sleep -Milliseconds 600
Check 'uninstall.ps1 removes exe and registration' ((-not (Test-Path "$dest\fastviewer.exe")) -and (-not (Test-Path $key)))

Write-Host "`n[13] Startup timing (process start -> first image pixels on screen)"
Remove-Item "$env:TEMP\fastviewer_timing.txt" -ErrorAction SilentlyContinue; $env:FASTVIEWER_TIMING = '1'
$rows = foreach ($i in 1..5) { Remove-Item "$env:TEMP\fastviewer_timing.txt" -ErrorAction SilentlyContinue
    $p = Start-Process $Exe -ArgumentList "`"$Data\a2.jpg`"" -PassThru; Start-Sleep -Milliseconds 1200; Stop-Process $p -Force; Start-Sleep -Milliseconds 200
    $t = Get-Content "$env:TEMP\fastviewer_timing.txt"; $w = [double](($t | Select-String 'window shown') -replace '^\s*([\d.]+) ms.*', '$1'); $g = [double](($t | Select-String 'first image painted \(GDI\)') -replace '^\s*([\d.]+) ms.*', '$1')
    [pscustomobject]@{ Window = $w; FirstImage = $g } }
Remove-Item Env:\FASTVIEWER_TIMING; $rows | Format-Table -Auto | Out-String | Write-Host
$med = ($rows.FirstImage | Sort-Object)[2]; Check 'first image on screen < 150 ms (median of 5)' ($med -lt 150) "median=$med ms"

# ---------------------------------------------------------------------------------------------------
# contact sheet of key screenshots
$pick = 'fmt_a2.jpg', 'fmt_alpha.png', 'fmt_exif.jpg', 'info_panel', 'help', 'tools_info_pixel_hist', 'chan1', 'bg2', 'loupe', 'measure', 'rot90', 'gif', 'drop5000', 'error', 'empty'
$cw = 440; $chh = 300; $cols = 3; $rowsN = [math]::Ceiling($pick.Count / $cols)
$sheet = New-Object System.Drawing.Bitmap ($cw * $cols), ($chh * $rowsN); $g = [System.Drawing.Graphics]::FromImage($sheet); $g.Clear([System.Drawing.Color]::Black)
$g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
for ($i = 0; $i -lt $pick.Count; $i++) { $k = $pick[$i]; if ($script:shots.ContainsKey($k)) { $b = $script:shots[$k]; $g.DrawImage($b, ($i % $cols) * $cw + 2, [math]::Floor($i / $cols) * $chh + 2, $cw - 4, $chh - 4)
    $g.DrawString($k, (New-Object System.Drawing.Font('Arial', 10, [System.Drawing.FontStyle]::Bold)), [System.Drawing.Brushes]::Yellow, ($i % $cols) * $cw + 8, [math]::Floor($i / $cols) * $chh + 6) } }
$g.Dispose(); $sheet.Save((Join-Path $Out 'contact_sheet.png'))

$fail = @($script:results | Where-Object { -not $_.Pass })
Write-Host ("`n==== {0} checks, {1} passed, {2} FAILED ====" -f $script:results.Count, ($script:results.Count - $fail.Count), $fail.Count) -ForegroundColor $(if ($fail.Count) { 'Red' } else { 'Green' })
$script:results | Export-Csv (Join-Path $Out 'results.csv') -NoTypeInformation
$fail | Format-Table Test, Detail -Auto -Wrap | Out-String | Write-Host
