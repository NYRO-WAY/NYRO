$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$src = Join-Path $root "src"
$dst = Join-Path (Split-Path $root -Parent) "src\\web\\assets"

New-Item -ItemType Directory -Force -Path $dst | Out-Null
Copy-Item (Join-Path $src "admin.html") (Join-Path $dst "admin.html") -Force
Copy-Item (Join-Path $src "admin.css") (Join-Path $dst "admin.css") -Force
Copy-Item (Join-Path $src "admin.js") (Join-Path $dst "admin.js") -Force

Write-Output "synced admin assets to $dst"

