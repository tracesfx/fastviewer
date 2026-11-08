# Generates test images for FastViewer's self test. Usage: make-testdata.ps1 -Out <dir>
param([Parameter(Mandatory)][string]$Out)
Add-Type -AssemblyName System.Drawing
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force $Out | Out-Null
$DI = [System.Drawing.Imaging.ImageFormat]

function New-Prop($id, $type, [byte[]]$bytes) {
    $pi = [System.Runtime.Serialization.FormatterServices]::GetUninitializedObject([System.Drawing.Imaging.PropertyItem])
    $pi.Id = $id; $pi.Type = $type; $pi.Value = $bytes; $pi.Len = $bytes.Length; $pi
}
function Ascii($s) { [System.Text.Encoding]::ASCII.GetBytes($s + "`0") }
function Rat($n, $d) { [BitConverter]::GetBytes([uint32]$n) + [BitConverter]::GetBytes([uint32]$d) }
function U16($v) { [BitConverter]::GetBytes([uint16]$v) }

function Gradient($w, $h, $label) {
    $b = New-Object System.Drawing.Bitmap $w, $h
    $g = [System.Drawing.Graphics]::FromImage($b)
    $br = New-Object System.Drawing.Drawing2D.LinearGradientBrush((New-Object System.Drawing.Point 0, 0), (New-Object System.Drawing.Point $w, $h), [System.Drawing.Color]::OrangeRed, [System.Drawing.Color]::RoyalBlue)
    $g.FillRectangle($br, 0, 0, $w, $h)
    $f = New-Object System.Drawing.Font("Arial", [int]($h / 6))
    $g.DrawString($label, $f, [System.Drawing.Brushes]::White, 6, 6)
    $g.Dispose(); $b
}

# plain formats
foreach ($t in @(@('a1.png', 1600, 1000, $DI::Png), @('a2.jpg', 4000, 3000, $DI::Jpeg), @('a10.bmp', 800, 600, $DI::Bmp), @('a4.tif', 1200, 800, $DI::Tiff), @('a3.gif', 500, 500, $DI::Gif))) {
    $b = Gradient $t[1] $t[2] $t[0]; $b.Save("$Out\$($t[0])", $t[3]); $b.Dispose()
}

# EXIF jpeg: stored 300x200 with Orientation=6 (must display as 200x300), camera data and GPS
$b = Gradient 300 200 "EXIF"
$b.SetPropertyItem((New-Prop 0x010F 2 (Ascii "TestCam")))
$b.SetPropertyItem((New-Prop 0x0110 2 (Ascii "TestCam X100")))
$b.SetPropertyItem((New-Prop 0x0112 3 (U16 6)))
$b.SetPropertyItem((New-Prop 0x829A 5 (Rat 1 250)))
$b.SetPropertyItem((New-Prop 0x829D 5 (Rat 28 10)))
$b.SetPropertyItem((New-Prop 0x8827 3 (U16 400)))
$b.SetPropertyItem((New-Prop 0x9003 2 (Ascii "2024:05:17 14:30:05")))
$b.SetPropertyItem((New-Prop 0x920A 5 (Rat 35 1)))
$b.SetPropertyItem((New-Prop 0x0001 2 (Ascii "N")))
$b.SetPropertyItem((New-Prop 0x0002 5 ((Rat 37 1) + (Rat 46 1) + (Rat 30 1))))
$b.SetPropertyItem((New-Prop 0x0003 2 (Ascii "W")))
$b.SetPropertyItem((New-Prop 0x0004 5 ((Rat 122 1) + (Rat 25 1) + (Rat 9 1))))
$b.Save("$Out\exif.jpg", $DI::Jpeg); $b.Dispose()

# PNG with tEXt chunk (AI-style "parameters")
$b = Gradient 320 240 "TEXT"; $b.Save("$Out\text_plain.png", $DI::Png); $b.Dispose()
$png = [IO.File]::ReadAllBytes("$Out\text_plain.png")
$crcTable = for ($n = 0; $n -lt 256; $n++) { $c = [uint32]$n; for ($k = 0; $k -lt 8; $k++) { if ($c -band 1) { $c = 0xEDB88320 -bxor ($c -shr 1) } else { $c = $c -shr 1 } }; $c }
function Crc($bytes) { $c = [uint32]4294967295; foreach ($x in $bytes) { $c = $crcTable[($c -bxor $x) -band 255] -bxor ($c -shr 8) }; $c -bxor 4294967295 }
$data = [Text.Encoding]::ASCII.GetBytes("parameters`0a lighthouse at dusk, 35mm, Steps: 20, Sampler: Euler a, CFG scale: 7")
$type = [Text.Encoding]::ASCII.GetBytes("tEXt")
$len = [BitConverter]::GetBytes([uint32]$data.Length); [Array]::Reverse($len)
$crc = [BitConverter]::GetBytes([uint32](Crc ($type + $data))); [Array]::Reverse($crc)
$chunk = $len + $type + $data + $crc
$newpng = $png[0..32] + $chunk + $png[33..($png.Length - 1)]   # after signature(8)+IHDR(25)
[IO.File]::WriteAllBytes("$Out\text.png", [byte[]]$newpng)
Remove-Item "$Out\text_plain.png"

# transparent PNG (checkerboard-visible): red disc on transparent background
$b = New-Object System.Drawing.Bitmap 400, 300, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
$g = [System.Drawing.Graphics]::FromImage($b); $g.Clear([System.Drawing.Color]::Transparent)
$g.FillEllipse((New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 220, 30, 30))), 50, 30, 300, 240)
$g.FillRectangle((New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(128, 0, 200, 0))), 0, 0, 120, 80)
$g.Dispose(); $b.Save("$Out\alpha.png", $DI::Png); $b.Dispose()

# 1x1 WebP (lossy) - tests the OS WebP codec path
[IO.File]::WriteAllBytes("$Out\tiny.webp", [Convert]::FromBase64String("UklGRiIAAABXRUJQVlA4IBYAAAAwAQCdASoBAAEADsD+JaQAA3AAAAAA"))

# animated WebP: 3 identical 1x1 lossy frames, 120 ms each (hand-assembled RIFF: VP8X + ANIM + 3x ANMF)
function U24($v) { [byte[]]@(($v -band 255), (($v -shr 8) -band 255), (($v -shr 16) -band 255)) }
function U32($v) { [BitConverter]::GetBytes([uint32]$v) }
function Chunk($tag, $data) { $c = [Text.Encoding]::ASCII.GetBytes($tag) + (U32 $data.Length) + $data; if ($data.Length % 2) { $c += [byte]0 }; , $c }
$vp8 = [Convert]::FromBase64String("UklGRiIAAABXRUJQVlA4IBYAAAAwAQCdASoBAAEADsD+JaQAA3AAAAAA")[12..41]
$wb = [Text.Encoding]::ASCII.GetBytes('WEBP') + (Chunk 'VP8X' ([byte[]](0x12, 0, 0, 0) + (U24 0) + (U24 0))) + (Chunk 'ANIM' ((U32 4279312947) + [byte[]](0, 0)))
foreach ($i in 1..3) { $wb += (Chunk 'ANMF' ((U24 0) + (U24 0) + (U24 0) + (U24 0) + (U24 120) + [byte[]](0) + $vp8)) }
New-Item -ItemType Directory -Force "$Out\webp" | Out-Null
[IO.File]::WriteAllBytes("$Out\webp\moving.webp", [Text.Encoding]::ASCII.GetBytes('RIFF') + (U32 $wb.Length) + $wb)

# animated GIF: 5 frames, 200 ms each, a bar that moves left to right
$cols = 'Red', 'Orange', 'Yellow', 'Green', 'Blue'
$frames = for ($i = 0; $i -lt 5; $i++) {
    $f = New-Object System.Drawing.Bitmap 120, 120
    $g = [System.Drawing.Graphics]::FromImage($f); $g.Clear([System.Drawing.Color]::Black)
    $g.FillRectangle((New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromName($cols[$i]))), $i * 24, 30, 24, 60)
    $g.DrawString("$($i+1)", (New-Object System.Drawing.Font("Arial", 18)), [System.Drawing.Brushes]::White, 4, 4)
    $g.Dispose(); $f
}
$delays = New-Object byte[] 20; for ($i = 0; $i -lt 5; $i++) { [BitConverter]::GetBytes([int]20).CopyTo($delays, $i * 4) }
$frames[0].SetPropertyItem((New-Prop 0x5100 4 $delays))
$frames[0].SetPropertyItem((New-Prop 0x5101 3 (U16 0)))
$codec = [System.Drawing.Imaging.ImageCodecInfo]::GetImageEncoders() | Where-Object { $_.MimeType -eq 'image/gif' }
$ep = New-Object System.Drawing.Imaging.EncoderParameters 1
$ep.Param[0] = New-Object System.Drawing.Imaging.EncoderParameter([System.Drawing.Imaging.Encoder]::SaveFlag, [long][System.Drawing.Imaging.EncoderValue]::MultiFrame)
$frames[0].Save("$Out\anim.gif", $codec, $ep)
$ep.Param[0] = New-Object System.Drawing.Imaging.EncoderParameter([System.Drawing.Imaging.Encoder]::SaveFlag, [long][System.Drawing.Imaging.EncoderValue]::FrameDimensionTime)
for ($i = 1; $i -lt 5; $i++) { $frames[0].SaveAdd($frames[$i], $ep) }
$ep.Param[0] = New-Object System.Drawing.Imaging.EncoderParameter([System.Drawing.Imaging.Encoder]::SaveFlag, [long][System.Drawing.Imaging.EncoderValue]::Flush)
$frames[0].SaveAdd($ep)
$frames | ForEach-Object { $_.Dispose() }

# many small files across two folders (large drag & drop list)
$tiny = Gradient 48 32 "x"; $tiny.Save("$Out\tiny_src.png", $DI::Png); $tiny.Dispose()
New-Item -ItemType Directory -Force "$Out\many1", "$Out\many2" | Out-Null
$bytes = [IO.File]::ReadAllBytes("$Out\tiny_src.png")
1..2500 | ForEach-Object { [IO.File]::WriteAllBytes("$Out\many1\img$_.png", $bytes); [IO.File]::WriteAllBytes("$Out\many2\pic$_.png", $bytes) }
Remove-Item "$Out\tiny_src.png"
"done: $((Get-ChildItem $Out -File).Count) files + 5000 in many1/many2"
