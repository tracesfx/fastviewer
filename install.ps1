# Installs FastViewer for the current user (no admin needed) and registers it for "Open with" / Default apps.
# Usage: .\install.ps1 [-Dest <folder>]
param([string]$Dest = (Join-Path $env:LOCALAPPDATA "Programs\FastViewer"), [switch]$Quiet)
$src = Join-Path $PSScriptRoot "target\release\fastviewer.exe"
if (-not (Test-Path $src)) { $src = Join-Path $PSScriptRoot "fastviewer.exe" }
if (-not (Test-Path $src)) { throw "fastviewer.exe not found - run 'cargo build --release' first." }
New-Item -ItemType Directory -Force $Dest | Out-Null
Copy-Item $src $Dest -Force
& (Join-Path $Dest "fastviewer.exe") --register --quiet
if (-not $Quiet) { "Installed to $Dest.`nWindows 11: right-click an image > Open with > Choose another app > FastViewer > Always." }
