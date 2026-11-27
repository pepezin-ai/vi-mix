<#
.SYNOPSIS
vi-mix: starts your own copy of the title through AnyXboxX with the creator's own settings
(`anyxboxx run`, the same as the 0.2.x graphical launcher: Windows + NVIDIA profile, worker pools sized
by the CPU, 1920x1080 output). See README.md for what this mix adds and what it measured.

.PARAMETER GameDir
Folder of your own unpacked copy (the one that holds game_scarlett_master.exe). Asked once and remembered
in %LOCALAPPDATA%\vi-mix\game-dir.txt.

.PARAMETER SeriesS
Report a Series S instead of a Series X, through tools\lancer_victory.ps1 (measured slower here).

.PARAMETER NativeOutput
Keep the title's own output size (4K on Series X) instead of 1920x1080 (measured slower here).
1920x1080 also makes the HUD look larger, because the title lays it out for 1080p.

.PARAMETER Warm
Translate every shader into the cache before the game (first start of a new build: tens of minutes).
#>
param(
    [string]$GameDir,
    [switch]$SeriesS,
    [switch]$NativeOutput,
    [switch]$Warm
)
$ErrorActionPreference = 'Stop'
$state = Join-Path $env:LOCALAPPDATA 'vi-mix'
New-Item -ItemType Directory -Force $state | Out-Null
$remember = Join-Path $state 'game-dir.txt'
if (-not $GameDir -and (Test-Path -LiteralPath $remember)) { $GameDir = (Get-Content -LiteralPath $remember -Raw).Trim() }
if (-not $GameDir) { $GameDir = Read-Host 'Folder of your own unpacked copy (holds game_scarlett_master.exe)' }
if (-not (Test-Path -LiteralPath (Join-Path $GameDir 'game_scarlett_master.exe'))) {
    throw "game_scarlett_master.exe not found in '$GameDir'."
}
Set-Content -LiteralPath $remember -Value $GameDir -Encoding UTF8

# Saves, caches and logs stay out of the game folder.
$env:VICTORY_DATA_DIR = Join-Path $state 'data'

if ($SeriesS) {
    $override = @{}
    if (-not $NativeOutput) { $override['VICTORY_OUTPUT'] = '1920x1080' }
    $args2 = @{ GameDir = $GameDir; Override = $override; Console = 'series-s' }
    if ($Warm) { $args2['Warm'] = $true }
    & (Join-Path $PSScriptRoot 'tools\lancer_victory.ps1') @args2
    exit $LASTEXITCODE
}

# The creator's path: build the anyxboxx command once, then let it build VIctory and start the game.
$cli = Join-Path $PSScriptRoot 'target\release\anyxboxx.exe'
if (-not (Test-Path -LiteralPath $cli)) {
    Write-Host 'Building the anyxboxx command (first start only)...'
    & cargo build --release -p anyxboxx --manifest-path (Join-Path $PSScriptRoot 'Cargo.toml')
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed (exit code $LASTEXITCODE)." }
}
$argv = @('run', '--game-dir', $GameDir)
if (-not $NativeOutput) { $argv += @('--override', 'VICTORY_OUTPUT=1920x1080') }
if ($Warm) { $argv += '--warm' }
& $cli @argv
exit $LASTEXITCODE
