<#
.SYNOPSIS
vi-mix: starts your own copy of the title through AnyXboxX (Vice-Tay VIctory base) with the
settings that measured best on an RTX 5070 + Ryzen 7 5700X (see README.md).

.PARAMETER GameDir
Folder of your own unpacked copy (the one that holds game_scarlett_master.exe). Asked once and remembered
in %LOCALAPPDATA%\vi-mix\game-dir.txt.

.PARAMETER SeriesS
Report a Series S to the title instead of a Series X (measured slower here: 7.1 against 10.9 fps).

.PARAMETER NativeOutput
Keep the title's own output size (4K on Series X) instead of forcing 1920x1080 (measured slower: 7.4 fps).
Forcing 1080p also makes the HUD look larger, because the title lays it out for 1080p.

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

$override = @{}
if (-not $NativeOutput) { $override['VICTORY_OUTPUT'] = '1920x1080' }
$lancer = Join-Path $PSScriptRoot 'tools\lancer_victory.ps1'
$args2 = @{ GameDir = $GameDir; Override = $override }
if ($SeriesS) { $args2['Console'] = 'series-s' }
if ($Warm) { $args2['Warm'] = $true }
& $lancer @args2
exit $LASTEXITCODE
