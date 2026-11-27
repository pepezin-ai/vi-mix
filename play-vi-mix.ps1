<#
.SYNOPSIS
vi-mix: starts your own copy of the title through AnyXboxX with the creator's own settings
(`anyxboxx run`, the same as the 0.2.x graphical launcher: Windows + NVIDIA profile, worker pools sized
by the CPU, 1920x1080 output). See README.md for what this mix adds and what it measured.

.PARAMETER GameDir
Folder of your own unpacked copy (the one that holds game_scarlett_master.exe). Asked once and remembered
in %LOCALAPPDATA%\vi-mix\game-dir.txt.

.PARAMETER NativeOutput
Keep the title's own output size (4K on Series X) instead of 1920x1080 (measured slower here).
1920x1080 also makes the HUD look larger, because the title lays it out for 1080p.

.PARAMETER Warm
Translate every shader into the cache before the game (first start of a new build: tens of minutes).

.PARAMETER Stock
Leave out the vi-mix switches below and start with the creator's settings alone.

.PARAMETER Tuned
Add the v9 memory switches measured to help on our machine (preliminary): VICTORY_WINDOWS_BAR_MB=2048 and
VICTORY_SECTION_MB=2. See README.md.

.PARAMETER Override
Extra KEY=VALUE settings passed to `anyxboxx run --override` (VICTORY_* names only), for example
-Override VICTORY_GPU_BACK_MAX_KB=64 (read the warning in README.md first).

.NOTES
vi-mix adds two switches to every start (both read by VIctory, both off in the creator's settings):
  VICTORY_PIPELINE_HISTORY=0  start without the scheduling history of earlier runs. Whole-picture colour
                              banding came back on some launches with it (TaynanGT/Vice-Tay#44); four
                              launches in a row without it were clean here.
  VICTORY_WIDE_COPY=1         cut big title copies into pieces on the kept threads (LooperSalty/AnyXboxX#500).
#>
param(
    [string]$GameDir,
    [switch]$NativeOutput,
    [switch]$Warm,
    [switch]$Stock,
    [switch]$Tuned,
    [string[]]$Override
)
# The two vi-mix switches (see .NOTES), plus the v9 memory switches with -Tuned.
$mixTweaks = [ordered]@{ VICTORY_PIPELINE_HISTORY = '0'; VICTORY_WIDE_COPY = '1' }
if ($Stock) { $mixTweaks = [ordered]@{} }
if ($Tuned) { $mixTweaks['VICTORY_WINDOWS_BAR_MB'] = '2048'; $mixTweaks['VICTORY_SECTION_MB'] = '2' }
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

# The creator's path: build the anyxboxx command once, then let it build VIctory and start the game.
$cli = Join-Path $PSScriptRoot 'target\release\anyxboxx.exe'
if (-not (Test-Path -LiteralPath $cli)) {
    Write-Host 'Building the anyxboxx command (first start only)...'
    & cargo build --release -p anyxboxx --manifest-path (Join-Path $PSScriptRoot 'Cargo.toml')
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed (exit code $LASTEXITCODE)." }
}
$argv = @('run', '--game-dir', $GameDir)
if (-not $NativeOutput) { $argv += @('--override', 'VICTORY_OUTPUT=1920x1080') }
foreach ($k in $mixTweaks.Keys) { $argv += @('--override', "$k=$($mixTweaks[$k])") }
foreach ($o in $Override) { $argv += @('--override', $o) }
if ($Warm) { $argv += '--warm' }
& $cli @argv
exit $LASTEXITCODE
