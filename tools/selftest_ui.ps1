# FastViewer UI layout self test (toolbar, gallery strip/grid/docks/size, metadata panel docks + hidden fields).
# Reuses the helpers from selftest.ps1. Usage: selftest_ui.ps1 -Data <dir from make-testdata.ps1> -Out <work dir>
param([Parameter(Mandatory)][string]$Data, [Parameter(Mandatory)][string]$Out, [string]$Exe = '')
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$lines = Get-Content (Join-Path $here 'selftest.ps1')
$end = ($lines | Select-String '^# =====' | Select-Object -First 1).LineNumber - 1
$start = ($lines | Select-String '^Add-Type -AssemblyName' | Select-Object -First 1).LineNumber - 1
$Root = Split-Path -Parent $here
if (-not $Exe) { $Exe = Join-Path (Join-Path (Join-Path $Root 'target') 'release') 'fastviewer.exe' }
$helpers = $lines[$start..($end - 1)] | Where-Object { $_ -notmatch '^\$Root = ' -and $_ -notmatch '^if \(-not \$Exe\)' }
. ([scriptblock]::Create(($helpers -join "`n")))

$ini = Join-Path $Out 'ui.ini'
function Rc($s, $k) { if (-not $s.ContainsKey($k) -or -not $s[$k]) { return $null }; $a = $s[$k] -split ','; ,@([double]$a[0], [double]$a[1], [double]$a[2], [double]$a[3]) }
function Cen($r) { @([int]($r[0] + $r[2] / 2), [int]($r[1] + $r[3] / 2)) }
function Click($p, $x, $y, $wait = 140) { Mouse $p 0x201 $x $y 1; Mouse $p 0x202 $x $y; Start-Sleep -Milliseconds $wait }
function DblClick($p, $x, $y) { Mouse $p 0x203 $x $y 1; Mouse $p 0x202 $x $y; Start-Sleep -Milliseconds 200 }
function Btn($p, $name) { $s = State $p; $r = Rc $s "btn.$name"; if (-not $r) { return $false }; $c = Cen $r; Click $p $c[0] $c[1]; $true }
function Start-UI($file, $iniText = '') {
    $env:FASTVIEWER_INI = $ini; Remove-Item $ini -ErrorAction SilentlyContinue
    if ($iniText) { [IO.File]::WriteAllText($ini, $iniText) }
    $p = Start-FV @($file); [void](Wait-State $p { param($s) $s['count'] -ne '0' }); $p
}
function Within($r, $o, $tol = 1.5) { $r[0] -ge $o[0] - $tol -and $r[1] -ge $o[1] - $tol -and ($r[0] + $r[2]) -le ($o[0] + $o[2] + $tol) -and ($r[1] + $r[3]) -le ($o[1] + $o[3] + $tol) }
function Overlap($a, $b) { $w = [math]::Min($a[0] + $a[2], $b[0] + $b[2]) - [math]::Max($a[0], $b[0]); $h = [math]::Min($a[1] + $a[3], $b[1] + $b[3]) - [math]::Max($a[1], $b[1]); $w -gt 1.5 -and $h -gt 1.5 }
$allActions = 'next', 'prev', 'first', 'last', 'zoom_in', 'zoom_out', 'fit', 'actual', 'fill', 'rotate_cw', 'rotate_ccw', 'flip', 'info', 'gallery', 'gallery_layout', 'gallery_dock', 'thumb_bigger', 'thumb_smaller', 'browse', 'info_dock', 'fields', 'toolbar', 'toolbar_dock', 'hud', 'pixel', 'histogram', 'channel', 'animate', 'step', 'background', 'smooth', 'fullscreen', 'copy_image', 'copy_path', 'reveal', 'open', 'delete', 'config', 'help'

Write-Host "`n[A] Toolbar: a button for every function, and each button works"
$p = Start-UI "$Data\a1.png"; $s = State $p; $ui = [double]$s['ui']
Check 'toolbar shown at top by default' ($s['u.toolbar'] -eq 'True' -and $s['u.tb_dock'] -eq 'top')
$bar = Rc $s 'rect.bar'; $view = Rc $s 'rect.view'
Check 'image view sits below the toolbar' ($view[1] -ge $bar[1] + $bar[3] - 1) "bar=$($bar -join ',') view=$($view -join ',')"
$missing = @($allActions | Where-Object { -not $s.ContainsKey("btn.$_") })
Check "a button exists for every function ($($allActions.Count) functions)" ($missing.Count -eq 0) "missing: $($missing -join ',')"
$all = @($s.Keys | Where-Object { $_ -like 'btn.*' } | ForEach-Object { , (Rc $s $_) })
Check 'all buttons lie inside the toolbar (wrapped onto rows)' (@($all | Where-Object { -not (Within $_ $bar) }).Count -eq 0) "buttons=$(@($all).Count) bar=$($bar -join ',')"
[void](Shot $p 'ui_default'); $o = ClientOrigin $p; $bm = $script:shots['ui_default']
Check 'toolbar actually painted (text on panel)' ((Distinct $bm $o 40 4 600 ([int]$bar[3] - 4)) -ge 3)
$r = Rc $s 'btn.fit'; $c = Cen $r; Mouse $p 0x200 $c[0] $c[1]; Start-Sleep -Milliseconds 250; $s = State $p
Check 'hovering a button is tracked (tooltip)' ($s['u.hover'] -match 'Btn'); [void](Shot $p 'ui_tooltip'); Mouse $p 0x200 600 400

$cases = @(
    @('next', { param($b, $a) [int]$a['idx'] -eq [int]$b['idx'] + 1 }), @('prev', { param($b, $a) [int]$a['idx'] -eq [int]$b['idx'] - 1 }),
    @('last', { param($b, $a) $a['idx'] -eq '9' }), @('first', { param($b, $a) $a['idx'] -eq '0' }),
    @('zoom_in', { param($b, $a) Near $a['zoom'] ([double]$b['zoom'] * 1.25) }), @('zoom_out', { param($b, $a) Near $a['zoom'] ([double]$b['zoom'] / 1.25) }),
    @('actual', { param($b, $a) Near $a['zoom'] 1.0 0.001 }), @('fit', { param($b, $a) $a['fit'] -eq 'True' }),
    @('fill', { param($b, $a) [double]$a['zoom'] -gt [double]$b['zoom'] }), @('fit', { param($b, $a) $a['fit'] -eq 'True' }),
    @('rotate_cw', { param($b, $a) $a['rot'] -eq '1' }), @('rotate_ccw', { param($b, $a) $a['rot'] -eq '0' }),
    @('flip', { param($b, $a) $a['flip'] -eq 'True' }), @('flip', { param($b, $a) $a['flip'] -eq 'False' }),
    @('info', { param($b, $a) $a['info'] -eq 'True' -and $a.ContainsKey('rect.info') }), @('info', { param($b, $a) $a['info'] -eq 'False' }),
    @('hud', { param($b, $a) $a['hud'] -ne $b['hud'] }), @('hud', { param($b, $a) $a['hud'] -eq 'True' }),
    @('pixel', { param($b, $a) $a['pixel'] -eq 'True' }), @('pixel', { param($b, $a) $a['pixel'] -eq 'False' }),
    @('histogram', { param($b, $a) $a['hist'] -eq 'True' }), @('histogram', { param($b, $a) $a['hist'] -eq 'False' }),
    @('channel', { param($b, $a) $a['channel'] -eq '1' }),
    @('background', { param($b, $a) $a['bg'] -eq '1' }), @('smooth', { param($b, $a) $a['smooth'] -eq '1' }),
    @('gallery', { param($b, $a) $a['film'] -eq 'False' -and -not $a.ContainsKey('rect.gal') }), @('gallery', { param($b, $a) $a['film'] -eq 'True' -and $a.ContainsKey('rect.gal') }),
    @('help', { param($b, $a) $a['help'] -eq 'True' })
)
foreach ($c in $cases) {
    $b = State $p
    $clicked = Btn $p $c[0]; $a = State $p
    Check "button [$($c[0])]" ($clicked -and (& $c[1] $b $a)) "idx=$($a['idx']) zoom=$($a['zoom']) rot=$($a['rot']) bg=$($a['bg'])"
    if ($c[0] -eq 'help') { Key $p $VK['Esc'] }
}
Stop-FV $p

Write-Host "`n[A2] Buttons that open or change things outside the view"
$p = Start-UI "$Data\a1.png"
[void](Btn $p 'copy_image'); Start-Sleep -Milliseconds 400
$clip = powershell -STA -NoProfile -Command "Add-Type -AssemblyName System.Windows.Forms; `$i=[Windows.Forms.Clipboard]::GetImage(); if(`$i){'{0}x{1}' -f `$i.Width,`$i.Height}else{'none'}"
Check 'button [copy_image]' ($clip -eq '1600x1000') $clip
[void](Btn $p 'copy_path'); Start-Sleep -Milliseconds 400
$clip = powershell -STA -NoProfile -Command "Add-Type -AssemblyName System.Windows.Forms; [Windows.Forms.Clipboard]::GetText()"
Check 'button [copy_path]' ($clip.Trim() -eq "$Data\a1.png") $clip
[void](Btn $p 'reveal'); $w = Find-Window '^td' 'CabinetWClass' 6000; Check 'button [reveal]' ($null -ne $w); Close-Win $w
[void](Btn $p 'open'); $w = Find-Window '.' '#32770' 4000; Check 'button [open]' ($null -ne $w); Close-Win $w; Start-Sleep -Milliseconds 300
[void](Btn $p 'config'); $w = Find-Window 'ui\.ini' $null 6000; Check 'button [config]' ($null -ne $w); Close-Win $w
[void](Btn $p 'fullscreen'); Start-Sleep -Milliseconds 400; $s = State $p; Check 'button [fullscreen]' ($s['fs'] -eq 'True'); Key $p $VK['F11'] 0 500
Stop-FV $p
New-Item -ItemType Directory -Force "$Out\deltest2" | Out-Null; Remove-Item "$Out\deltest2\*" -ErrorAction SilentlyContinue
'a1.png', 'a3.gif', 'a4.tif' | ForEach-Object { Copy-Item "$Data\$_" "$Out\deltest2\$_" }
$p = Start-UI "$Out\deltest2\a1.png"; [void](Btn $p 'delete'); $dlg = Find-Window '.' '#32770' 1200; if ($dlg) { Close-Win $dlg }
$s = Wait-State $p { param($s) $s['count'] -eq '2' } 5000; Check 'button [delete]' ($s['count'] -eq '2' -and -not (Test-Path "$Out\deltest2\a1.png")); Stop-FV $p
$p = Start-UI "$Data\anim.gif"; Start-Sleep -Milliseconds 500
[void](Btn $p 'animate'); Start-Sleep -Milliseconds 400; $a = State $p; Check 'button [animate] pauses' ($a['paused'] -eq 'True')
$f0 = [int](($a['frame'] -split '/')[0]); [void](Btn $p 'step'); Start-Sleep -Milliseconds 400; $a = State $p; Check 'button [step] advances one frame' ([int](($a['frame'] -split '/')[0]) -eq (($f0 + 1) % 5)) "$f0 -> $($a['frame'])"
[void](Btn $p 'animate'); Start-Sleep -Milliseconds 700; $a = State $p; Check 'button [animate] resumes' ($a['paused'] -eq 'False'); Stop-FV $p

Write-Host "`n[B] Gallery size (buttons, keys, Ctrl+wheel, drag the edge) and persistence"
$p = Start-UI "$Data\a1.png"; $s = State $p; $ui = [double]$s['ui']; $g0 = Rc $s 'rect.gal'
Check 'default gallery: strip at bottom, thumb 72' ($s['u.g_dock'] -eq 'bottom' -and $s['u.g_grid'] -eq 'False' -and $s['u.thumb'] -eq '72' -and $g0[1] + $g0[3] -ge [double]($s['client'] -split 'x')[1] - 1) "gal=$($g0 -join ',')"
Check 'strip thickness = (thumb+18) * scale' (Near $g0[3] ((72 + 18) * $ui) 0.03) "h=$($g0[3])"
[void](Btn $p 'thumb_bigger'); $s = State $p; $g1 = Rc $s 'rect.gal'
Check 'T+ button enlarges thumbnails and the strip' ($s['u.thumb'] -eq '83' -and $g1[3] -gt $g0[3]) "thumb=$($s['u.thumb']) h=$($g1[3])"
Key $p $VK['Plus'] 1; $s = State $p; Check 'Ctrl+Plus enlarges' ($s['u.thumb'] -eq '95') $s['u.thumb']
Key $p $VK['Minus'] 1; $s = State $p; Check 'Ctrl+Minus shrinks' ($s['u.thumb'] -eq '83') $s['u.thumb']
$cl = (State $p)['client'] -split 'x'; $W = [int]$cl[0]; $g = Rc (State $p) 'rect.gal'
Wheel $p ($W / 2) ([int]($g[1] + 40)) 1 1; $s = State $p; Check 'Ctrl+wheel over gallery enlarges' ([int]$s['u.thumb'] -gt 83) $s['u.thumb']
Wheel $p ($W / 2) ([int]($g[1] + 40)) -1 1; Wheel $p ($W / 2) ([int]($g[1] + 40)) -1 1; $s = State $p; Check 'Ctrl+wheel down shrinks' ([int]$s['u.thumb'] -lt 95) $s['u.thumb']
foreach ($i in 1..25) { Key $p $VK['Minus'] 1 30 }; $s = State $p; Check 'size clamps at minimum 32' ($s['u.thumb'] -eq '32') $s['u.thumb']
foreach ($i in 1..25) { Key $p $VK['Plus'] 1 30 }; $s = State $p; Check 'size clamps at maximum 320' ($s['u.thumb'] -eq '320') $s['u.thumb']
Start-Sleep -Milliseconds 800; $s = State $p; Check 'thumbnails regenerated at higher resolution' ([int]$s['u.thumb_cap'] -ge 320 * $ui -and [int]$s['thumbs'] -ge 3) "cap=$($s['u.thumb_cap']) thumbs=$($s['thumbs'])"
foreach ($i in 1..40) { Key $p $VK['Minus'] 1 20 }; Key $p $VK['Plus'] 1; Key $p $VK['Plus'] 1; $s = State $p; $thumb0 = [int]$s['u.thumb']; $g = Rc $s 'rect.gal'
$cx = [int]($W / 2); Mouse $p 0x201 $cx ([int]$g[1] + 1) 1; Mouse $p 0x200 $cx ([int]$g[1] - 20) 1; Mouse $p 0x200 $cx ([int]$g[1] - 39) 1; Mouse $p 0x202 $cx ([int]$g[1] - 39); Start-Sleep -Milliseconds 250
$s = State $p; $exp = ($g[3] + 40) / $ui - 18
Check 'dragging the gallery edge resizes thumbnails' ([math]::Abs([double]$s['u.thumb'] - $exp) -le 3) "thumb $thumb0 -> $($s['u.thumb']) expected ~$([math]::Round($exp))"
$iniText = Get-Content $ini -Raw; Check 'size is saved to the ini (comments kept)' ($iniText -match "thumb_size=$($s['u.thumb'])" -and $iniText -match '; FastViewer settings') ($iniText -split "`n" | Select-String 'thumb_size')
$saved = [regex]::Match($iniText, 'thumb_size=(\d+)').Groups[1].Value
Stop-FV $p
$p = Start-FV @("$Data\a1.png"); $s = State $p; Check 'size restored after restart' ($s['u.thumb'] -eq $saved) "$($s['u.thumb']) vs $saved"; Stop-FV $p

Write-Host "`n[C] Gallery layouts: docks, grid, scrolling, browse"
$p = Start-UI "$Data\a1.png"; $s = State $p; $cl = $s['client'] -split 'x'; $W = [int]$cl[0]; $H = [int]$cl[1]; $bar = Rc $s 'rect.bar'
foreach ($e in @('left', 'top', 'right', 'bottom')) {
    [void](Btn $p 'gallery_dock'); Start-Sleep -Milliseconds 400; $s = State $p; $g = Rc $s 'rect.gal'; $v = Rc $s 'rect.view'
    $ok = $s['u.g_dock'] -eq $e
    switch ($e) { 'left' { $ok = $ok -and $g[0] -le 1 -and $g[2] -lt 200 * $ui -and $g[3] -gt 300 }; 'top' { $ok = $ok -and $g[3] -lt 200 * $ui -and $g[2] -gt 500 -and $g[1] -ge ($bar[3] - 1) }
        'right' { $ok = $ok -and ($g[0] + $g[2]) -ge $W - 1 -and $g[2] -lt 200 * $ui }; 'bottom' { $ok = $ok -and ($g[1] + $g[3]) -ge $H - 1 } }
    Check "gallery dock -> $e" $ok "gal=$($g -join ',') view=$($v -join ',')"
    if ($e -in 'left', 'right') {
        $t2 = Rc $s 'thumb.2'; $c = Cen $t2; Click $p $c[0] $c[1]; $s = State $p
        Check "vertical strip ($e): click selects image" ($s['idx'] -eq '2') "idx=$($s['idx'])"; [void](Btn $p 'first')
        Wheel $p $c[0] $c[1] -1; $s = State $p; Check "vertical strip ($e): wheel navigates" ($s['idx'] -eq '1') "idx=$($s['idx'])"; [void](Btn $p 'first')
        if ($e -eq 'left') { [void](Shot $p 'ui_gallery_left') }
    }
}
[void](Btn $p 'gallery_layout'); $s = State $p; $g = Rc $s 'rect.gal'; $cols = [int]$s['gal.cols']
$expCols = [math]::Floor(($g[2] - 16 * $ui) / ((72 + 6) * $ui))
Check 'grid layout: panel uses gallery_size, cols fit the width' ($s['u.g_grid'] -eq 'True' -and (Near $g[3] (240 * $ui) 0.03) -and $cols -eq $expCols) "h=$($g[3]) cols=$cols expected $expCols"
Start-Sleep -Milliseconds 700; $s = State $p; $t5 = Rc $s 'thumb.5'; $c = Cen $t5; Click $p $c[0] $c[1]; $s = State $p; Check 'grid: click selects thumbnail' ($s['idx'] -eq '5') "idx=$($s['idx'])"
Check 'grid: thumbnails loaded' ([int]$s['thumbs'] -ge 8) "thumbs=$($s['thumbs'])"; [void](Shot $p 'ui_grid_bottom')
Stop-FV $p

$p = Start-UI "$Data\many1\img1.png"; [void](Wait-State $p { param($s) $s['count'] -eq '2500' } 6000)
[void](Btn $p 'gallery_layout'); Start-Sleep -Milliseconds 500; $s = State $p; $g = Rc $s 'rect.gal'; $cl = $s['client'] -split 'x'; $W = [int]$cl[0]
Wheel $p ($W / 2) ([int]($g[1] + 60)) -3; $s = State $p; Check 'grid: wheel scrolls rows' ([double]$s['u.g_scroll'] -gt 0) "scroll=$($s['u.g_scroll'])"
Key $p $VK['End'] 0 500; $s = State $p; $g = Rc $s 'rect.gal'; $tc = Rc $s 'thumb.cur'
Check 'grid: End jumps to last image and scrolls it into view' ($s['idx'] -eq '2499' -and $tc[1] -ge $g[1] - 1 -and ($tc[1] + $tc[3]) -le ($g[1] + $g[3] + 1)) "scroll=$($s['u.g_scroll']) thumb=$($tc -join ',') gal=$($g -join ',')"
Key $p $VK['Home'] 0 400; $s = State $p; Check 'grid: Home scrolls back to top' ($s['idx'] -eq '0' -and [double]$s['u.g_scroll'] -le 1)
[void](Btn $p 'browse'); Start-Sleep -Milliseconds 700; $s = State $p; $g = Rc $s 'rect.gal'; $v = Rc $s 'rect.view'; $cl = $s['client'] -split 'x'
Check 'browse: thumbnail grid fills the window below the toolbar' ($s['u.browse'] -eq 'True' -and (Near $g[2] ([double]$cl[0]) 0.01) -and $g[3] -gt 400 -and $v[2] -le 2) "gal=$($g -join ',') view=$($v -join ',')"
$cols = [int]$s['gal.cols']; [void](Shot $p 'ui_browse')
$t = Rc $s 'thumb.3'; $c = Cen $t; Click $p $c[0] $c[1]; $s = State $p; Check 'browse: click selects, stays in browse' ($s['idx'] -eq '3' -and $s['u.browse'] -eq 'True')
Key $p $VK['Down'] 0 300; $s = State $p; Check 'browse: Down moves one row' ($s['idx'] -eq "$(3 + $cols)") "idx=$($s['idx']) cols=$cols"
Key $p $VK['Up'] 0 300; $s = State $p; Check 'browse: Up moves one row back' ($s['idx'] -eq '3')
Key $p $VK['Right'] 0 300; $s = State $p; Check 'browse: Right moves one' ($s['idx'] -eq '4')
$t = Rc $s 'thumb.6'; $c = Cen $t; DblClick $p $c[0] $c[1]; $s = State $p; Check 'browse: double-click opens the image' ($s['idx'] -eq '6' -and $s['u.browse'] -eq 'False')
Key $p $VK['Tab'] 0 500; $s = State $p; Check 'Tab toggles browse on' ($s['u.browse'] -eq 'True'); Key $p $VK['Esc'] 0 300; $s = State $p; Check 'Esc leaves browse' ($s['u.browse'] -eq 'False' -and -not $p.HasExited)
Stop-FV $p

Write-Host "`n[D] Metadata panel: docks, size, hiding fields, field chooser"
$p = Start-UI "$Data\exif.jpg"; [void](Btn $p 'info'); $s = State $p; $i = Rc $s 'rect.info'; $cl = $s['client'] -split 'x'; $W = [int]$cl[0]; $H = [int]$cl[1]
Check 'info panel docked right, 330 wide, single column' ($s['u.i_dock'] -eq 'right' -and (Near $i[2] (330 * $ui) 0.03) -and ($i[0] + $i[2]) -ge $W - 1 -and $s['info.cols'] -eq '1') "info=$($i -join ',') cols=$($s['info.cols'])"
Check 'metadata rows listed' ($s['info.rows'] -match 'Camera' -and $s['info.rows'] -match 'GPS' -and $s['info.rows'] -match 'Dimensions') $s['info.rows']
foreach ($e in @('bottom', 'left', 'top', 'right')) {
    [void](Btn $p 'info_dock'); $s = State $p; $i = Rc $s 'rect.info'
    $ok = $s['u.i_dock'] -eq $e
    switch ($e) { 'bottom' { $ok = $ok -and (Near $i[3] (170 * $ui) 0.03) -and $i[2] -ge $W - 2 -and ($i[1] + $i[3]) -ge (Rc $s 'rect.gal')[1] - 1 -and [int]$s['info.cols'] -ge 2 }   # docks directly above the gallery
        'left' { $ok = $ok -and (Near $i[2] (330 * $ui) 0.03) -and $i[0] -le 1 -and $s['info.cols'] -eq '1' }
        'top' { $ok = $ok -and (Near $i[3] (170 * $ui) 0.03) -and [int]$s['info.cols'] -ge 2 }
        'right' { $ok = $ok -and (Near $i[2] (330 * $ui) 0.03) -and ($i[0] + $i[2]) -ge $W - 1 } }
    Check "info dock -> $e" $ok "info=$($i -join ',') cols=$($s['info.cols'])"
    if ($e -eq 'bottom') { [void](Shot $p 'ui_info_bottom') }
}
$i = Rc $s 'rect.info'; $cy = [int]($i[1] + 200); Mouse $p 0x201 ([int]$i[0] + 1) $cy 1; Mouse $p 0x200 ([int]$i[0] - 60) $cy 1; Mouse $p 0x202 ([int]$i[0] - 80) $cy; Start-Sleep -Milliseconds 250
$s = State $p; $i2 = Rc $s 'rect.info'; Check 'dragging the panel edge resizes it' ($i2[2] -gt $i[2] + 40) "width $($i[2]) -> $($i2[2])"
$r0 = Rc $s 'info.row0'; $first = ($s['info.rows'] -split '\|')[0]; Mouse $p 0x200 ([int]($r0[0] + 30)) ([int]($r0[1] + 5)); Start-Sleep -Milliseconds 200
Click $p ([int]($r0[0] + $r0[2] - 8)) ([int]($r0[1] + $r0[3] / 2)); $s = State $p
Check "click x on a row hides that field ($first)" ($s['u.hidden'] -match [regex]::Escape($first) -and $s['info.rows'] -notmatch ("^" + [regex]::Escape($first) + "\|")) "hidden=$($s['u.hidden'])"
Key $p (Letter 'i') 1 300; $s = State $p; Check 'Ctrl+I opens the field chooser listing every field' ($s['u.i_edit'] -eq 'True' -and ($s['info.rows'] -split '\|').Count -ge 30) "rows=$(($s['info.rows'] -split '\|').Count)"
[void](Shot $p 'ui_fields')
$r0 = Rc $s 'info.row0'; Click $p ([int]($r0[0] + 20)) ([int]($r0[1] + $r0[3] / 2)); $s = State $p; Check 'chooser: clicking a row toggles it back on' ($s['u.hidden'] -notmatch [regex]::Escape($first))
$r0 = Rc $s 'info.row0'; Click $p ([int]($r0[0] + 20)) ([int]($r0[1] + $r0[3] / 2)); Key $p $VK['Esc'] 0 200; $s = State $p
Check 'Esc closes the chooser; field hidden again' ($s['u.i_edit'] -eq 'False' -and $s['u.hidden'] -match [regex]::Escape($first))
$iniText = Get-Content $ini -Raw; Check 'hidden fields saved to ini' ($iniText -match ('info_hidden=.*' + [regex]::Escape($first))) ($iniText -split "`n" | Select-String 'info_hidden')
Stop-FV $p
$p = Start-FV @("$Data\exif.jpg"); [void](Btn $p 'info'); $s = State $p; Check 'hidden field stays hidden after restart' ($s['u.hidden'] -match [regex]::Escape($first) -and $s['info.rows'] -notmatch ("^" + [regex]::Escape($first) + "\|")); Stop-FV $p
$p = Start-UI "$Data\exif.jpg" "info_hidden=Folder,Megapixels,Aspect,Size`ninfo_dock=left`nshow_info=1`n"; $s = State $p
Check 'ini: info_hidden / info_dock / show_info honoured' ($s['u.i_dock'] -eq 'left' -and $s['info.rows'] -notmatch 'Folder|Megapixels|Aspect|Size' -and $s['info.rows'] -match 'Camera') $s['info.rows']; Stop-FV $p

Write-Host "`n[E] Toolbar position, wrapping, visibility, custom buttons"
$p = Start-UI "$Data\a1.png"; $s = State $p; $cl = $s['client'] -split 'x'; $W = [int]$cl[0]; $H = [int]$cl[1]; $bar0 = Rc $s 'rect.bar'
foreach ($e in @('right', 'bottom', 'left', 'top')) {
    [void](Btn $p 'toolbar_dock'); $s = State $p; $b = Rc $s 'rect.bar'; $v = Rc $s 'rect.view'
    $bs = @($s.Keys | Where-Object { $_ -like 'btn.*' } | ForEach-Object { , (Rc $s $_) })
    $ok = $s['u.tb_dock'] -eq $e -and (@($bs | Where-Object { -not (Within $_ $b) }).Count -eq 0)
    switch ($e) { 'right' { $ok = $ok -and ($b[0] + $b[2]) -ge $W - 1 -and $b[2] -lt $W / 3 }; 'bottom' { $ok = $ok -and ($b[1] + $b[3]) -ge $H - 1 }
        'left' { $ok = $ok -and $b[0] -le 1 -and $b[2] -lt $W / 3 }; 'top' { $ok = $ok -and $b[1] -le 1 -and $b[2] -ge $W - 1 } }
    Check "toolbar dock -> $e (all buttons inside)" $ok "bar=$($b -join ',') view=$($v -join ',')"
    if ($e -eq 'left') { [void](Shot $p 'ui_toolbar_left') }
}
[void][N]::SetWindowPos($p.MainWindowHandle, [IntPtr]::Zero, 40, 40, 560, 600, 0x14); Start-Sleep -Milliseconds 500; $s = State $p; $b = Rc $s 'rect.bar'
Check 'narrow window: toolbar wraps onto more rows' ($b[3] -gt $bar0[3] * 1.4) "bar h $($bar0[3]) -> $($b[3])"
[void][N]::SetWindowPos($p.MainWindowHandle, [IntPtr]::Zero, 40, 40, 1100, 760, 0x14); Start-Sleep -Milliseconds 400
Key $p (Letter 't') 1 300; $s = State $p; Check 'Ctrl+T hides the toolbar; view uses the space' ($s['u.toolbar'] -eq 'False' -and -not $s.ContainsKey('rect.bar') -and (Rc $s 'rect.view')[1] -le 1)
$iniText = Get-Content $ini -Raw; Check 'toolbar visibility saved' ($iniText -match 'toolbar=0')
Key $p (Letter 't') 1 300; $s = State $p; Check 'Ctrl+T shows it again' ($s['u.toolbar'] -eq 'True'); Key $p (Letter 't') 2 300; $s = State $p; Check 'Shift+T moves the toolbar' ($s['u.tb_dock'] -eq 'right'); Stop-FV $p
$p = Start-UI "$Data\a1.png" "toolbar_buttons=next,prev,|,fit,rotate_cw`ntoolbar_dock=bottom`n"; $s = State $p; $n = @($s.Keys | Where-Object { $_ -like 'btn.*' }).Count
Check 'ini: custom toolbar_buttons / toolbar_dock' ($n -eq 4 -and $s.ContainsKey('btn.rotate_cw') -and $s['u.tb_dock'] -eq 'bottom' -and -not $s.ContainsKey('btn.info')) "buttons=$n"; Stop-FV $p
$p = Start-UI "$Data\a1.png" "toolbar=0`n"; $s = State $p; Check 'ini: toolbar=0 starts without toolbar' (-not $s.ContainsKey('rect.bar')); Stop-FV $p

Write-Host "`n[F] Everything together: panels never overlap, settings persist"
$p = Start-UI "$Data\exif.jpg"
foreach ($a in 'info', 'gallery_layout', 'gallery_dock', 'info_dock', 'info_dock', 'toolbar_dock', 'thumb_bigger') { [void](Btn $p $a) }
Start-Sleep -Milliseconds 500; $s = State $p; $cl = $s['client'] -split 'x'; $W = [double]$cl[0]; $H = [double]$cl[1]
$rs = @{ bar = Rc $s 'rect.bar'; gal = Rc $s 'rect.gal'; info = Rc $s 'rect.info'; view = Rc $s 'rect.view' }
$names = 'bar', 'gal', 'info', 'view'; $ov = @()
for ($a = 0; $a -lt 4; $a++) { for ($b2 = $a + 1; $b2 -lt 4; $b2++) { if ($rs[$names[$a]] -and $rs[$names[$b2]] -and (Overlap $rs[$names[$a]] $rs[$names[$b2]])) { $ov += "$($names[$a])/$($names[$b2])" } } }
$area = 0; foreach ($n2 in $names) { if ($rs[$n2]) { $area += $rs[$n2][2] * $rs[$n2][3] } }
Check 'bar, gallery, info and view do not overlap and tile the window' ($ov.Count -eq 0 -and (Near $area ($W * $H) 0.02)) "overlaps=$($ov -join ',') area=$area window=$($W*$H) docks: bar=$($s['u.tb_dock']) gal=$($s['u.g_dock']) info=$($s['u.i_dock'])"
$z = [double]$s['zoom']; $v = $rs.view; Check 'image is fitted inside the remaining view' ($z -le 1.0 -and ($z * [double]$s['w'] -le $v[2] + 1) -and ($z * [double]$s['h'] -le $v[3] + 1)) "zoom=$z view=$($v -join ',')"
[void](Shot $p 'ui_all_panels'); $keep = @{ gd = $s['u.g_dock']; gg = $s['u.g_grid']; id = $s['u.i_dock']; td = $s['u.tb_dock']; th = $s['u.thumb'] }
Stop-FV $p
$iniText = Get-Content $ini -Raw
Check 'layout written to ini; comments and other settings preserved' ($iniText -match "gallery_dock=$($keep.gd)" -and $iniText -match 'gallery_layout=grid' -and $iniText -match "info_dock=$($keep.id)" -and $iniText -match "toolbar_dock=$($keep.td)" -and $iniText -match 'zoom_step=1.25' -and $iniText -match '\[keys\]')
$p = Start-FV @("$Data\exif.jpg"); $s = State $p
Check 'layout restored after restart' ($s['u.g_dock'] -eq $keep.gd -and $s['u.g_grid'] -eq $keep.gg -and $s['u.i_dock'] -eq $keep.id -and $s['u.tb_dock'] -eq $keep.td -and $s['u.thumb'] -eq $keep.th) "gal=$($s['u.g_dock']) grid=$($s['u.g_grid']) info=$($s['u.i_dock']) bar=$($s['u.tb_dock']) thumb=$($s['u.thumb'])"
Stop-FV $p
Remove-Item Env:\FASTVIEWER_INI

$pick = 'ui_default', 'ui_tooltip', 'ui_gallery_left', 'ui_grid_bottom', 'ui_browse', 'ui_info_bottom', 'ui_fields', 'ui_toolbar_left', 'ui_all_panels'
$cw = 560; $chh = 380; $cols = 3; $rowsN = [math]::Ceiling($pick.Count / $cols)
$sheet = New-Object System.Drawing.Bitmap ($cw * $cols), ($chh * $rowsN); $g = [System.Drawing.Graphics]::FromImage($sheet); $g.Clear([System.Drawing.Color]::Black)
$g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
for ($i = 0; $i -lt $pick.Count; $i++) { $k = $pick[$i]; if ($script:shots.ContainsKey($k)) { $bmp = $script:shots[$k]; $g.DrawImage($bmp, ($i % $cols) * $cw + 2, [math]::Floor($i / $cols) * $chh + 2, $cw - 4, $chh - 4)
    $g.DrawString($k, (New-Object System.Drawing.Font('Arial', 10, [System.Drawing.FontStyle]::Bold)), [System.Drawing.Brushes]::Yellow, ($i % $cols) * $cw + 8, [math]::Floor($i / $cols) * $chh + 6) } }
$g.Dispose(); $sheet.Save((Join-Path $Out 'ui_contact_sheet.png'))
$fail = @($script:results | Where-Object { -not $_.Pass })
Write-Host ("`n==== {0} UI checks, {1} passed, {2} FAILED ====" -f $script:results.Count, ($script:results.Count - $fail.Count), $fail.Count) -ForegroundColor $(if ($fail.Count) { 'Red' } else { 'Green' })
$script:results | Export-Csv (Join-Path $Out 'ui_results.csv') -NoTypeInformation
$fail | Format-Table Test, Detail -Auto -Wrap | Out-String | Write-Host
