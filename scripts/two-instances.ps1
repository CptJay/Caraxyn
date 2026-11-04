# Runs two Caraxyn instances side by side, "a" and "b", each with its own data
# folder, to try invites and shares on one machine. Ctrl-C stops both.
#
# "a" runs the dev server (the UI reloads on changes); "b" uses the same server.
# Their data is kept in target\two-instances\ between runs; delete it to start fresh.
#
# Run from anywhere:  powershell -ExecutionPolicy Bypass -File scripts\two-instances.ps1
$ErrorActionPreference = 'Stop'

$root = (Resolve-Path "$PSScriptRoot\..").Path
$app = Join-Path $root 'crates\caraxyn'
$data = Join-Path $root 'target\two-instances'
$devUrl = 'http://localhost:5173' # build.devUrl in tauri.conf.json
New-Item -ItemType Directory -Force "$data\a", "$data\b" | Out-Null

if (-not (Test-Path "$app\ui\node_modules")) {
    Push-Location "$app\ui"
    try { npm ci } finally { Pop-Location }
}

$started = @()
try {
    # Child processes copy the environment at start, so each gets its own folder.
    $env:CARAXYN_DATA_DIR = "$data\a"
    $a = Start-Process -PassThru -NoNewWindow -WorkingDirectory $app `
        -FilePath "$app\ui\node_modules\.bin\tauri.cmd" -ArgumentList 'dev'
    $started += $a

    Write-Host "waiting for the dev server at $devUrl ..."
    while ($true) {
        if ($a.HasExited) { throw 'instance a stopped before the dev server came up' }
        try {
            Invoke-WebRequest -UseBasicParsing -TimeoutSec 2 $devUrl | Out-Null
            break
        } catch {
            Start-Sleep -Seconds 1
        }
    }

    # Windows locks a running .exe, so "b" runs a copy: a's rebuilds on Rust changes can
    # then replace caraxyn.exe. "b" keeps the Rust code it started with until the next run.
    cargo build --manifest-path (Join-Path $root 'Cargo.toml') -p caraxyn
    if ($LASTEXITCODE -ne 0) { throw 'building the app failed' }
    $copy = Join-Path $data 'caraxyn-b.exe'
    Copy-Item -Force (Join-Path $root 'target\debug\caraxyn.exe') $copy

    $env:CARAXYN_DATA_DIR = "$data\b"
    $started += Start-Process -PassThru -NoNewWindow -WorkingDirectory $root -FilePath $copy

    Wait-Process -Id ($started | ForEach-Object Id)
} finally {
    Remove-Item Env:CARAXYN_DATA_DIR -ErrorAction SilentlyContinue
    # /T also stops what they started (node, vite, the app itself).
    foreach ($p in $started) {
        if (-not $p.HasExited) { taskkill /T /F /PID $p.Id | Out-Null }
    }
}
