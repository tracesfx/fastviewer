# Removes FastViewer's registration and (optionally) its install folder.
param([string]$Dest = (Join-Path $env:LOCALAPPDATA "Programs\FastViewer"), [switch]$Quiet)
$exe = Join-Path $Dest "fastviewer.exe"
if (Test-Path $exe) { & $exe --unregister --quiet; Start-Sleep -Milliseconds 300; Remove-Item $Dest -Recurse -Force -ErrorAction SilentlyContinue }
if (-not $Quiet) { "FastViewer removed." }
