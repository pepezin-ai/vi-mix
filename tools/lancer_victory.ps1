<#
.SYNOPSIS
  Builds (cargo skips the work when current) and launches VIctory (third_party/victory, Vice-Tay base) on the user's own
  game folder.

.PARAMETER GameDir
  Folder of the game package (kept outside this repository). It must contain game_scarlett_master.exe.

.PARAMETER Warm
  Optional. Before the game, runs 'victory.exe warm' on the .rpf archives of the game folder only (never the folder
  itself: a game folder can hold other trees, which warm would walk too). It translates their shaders into the cache
  and makes their compute pipelines, with the same environment variables as the game (the cache key depends on them).
  About 7 minutes the first time; later launches reuse the cache.

.PARAMETER Precompile
  Optional. Before the game, runs 'victory.exe warm' on shaders.rpf, the game executable and its two DLLs
  (XboxBvhBuilder.dll, amd_fidelityfx_upscaler_xbox.dll), with the same environment variables as the game.
  Combine with -Warm to cover every archive as well.

.PARAMETER AutoA
  Optional. Taps A through the start screens for 150 s (VICTORY_AUTO_A=150), as Vice-Tay's play.ps1 does not (it is off here unless asked).

.PARAMETER FrametimeDump
  Optional. Writes frametime.csv into the cache folder, one row a second (VICTORY_FRAMETIME_DUMP=1).

.PARAMETER Diag
  Optional. Builds and runs the diag profile with the diagnostics feature instead of the release build the frame rates
  were measured with. -Dred implies it.

.PARAMETER SkipPipelines
  Optional. Passed unchanged to VICTORY_SKIP_PIPELINES when given.

.PARAMETER NoTunedSettings
  Optional. Does not set Vice-Tay's switch set below (only VICTORY_GAME_DIR, VICTORY_DISCORD and
  VICTORY_OCEAN_READBACK are set).

.PARAMETER Dred
  Optional. Turns on the full device-removed data (VICTORY_D3D12_DRED=1) to diagnose a device loss; needs the diagnostics
  build, so it implies -Diag. The tuned set carries VICTORY_D3D12_DRED=light (lists without markers); -Dred is applied
  after the set and after -Override, so it replaces that value. The full mode notes every command list operation and
  slows each frame.

.PARAMETER XgDll
  Optional. Path to your own copy of xg_xs.dll, the offline PC build of the GDK's texture layout library (never in this
  repository). VIctory then lays out the title's textures with it (VICTORY_XG_DLL).

.PARAMETER XgMode
  Optional, with -XgDll. 'real' (default) serves the title from xg_xs.dll; 'compare' keeps VIctory's layouts and logs
  every difference with the library (VICTORY_XG_MODE).

.PARAMETER BatchCardRehash
  Optional. Sets VICTORY_CARD_REHASH_RUNS=1 even with -NoTunedSettings (Vice-Tay's switch set already sets it).

.PARAMETER Override
  Optional. A hashtable of VICTORY_* variables set after the tuned set (and after -NoTunedSettings), for A/B runs:
  -Override @{ VICTORY_RELAY = '0'; VICTORY_PIPELINE_WAIT_MS = '12000' }. Applied last, so it wins over the set.

.PARAMETER Quality
  Optional. Applies the picture-first set on top of the tuned set (after -NoTunedSettings too, before -Override):
  windows and buffers are read fresh each frame instead of kept as card copies, with more VRAM headroom. It costs
  about half the frame rate: measured about 7 to 9 fps against 14 to 17 for the default set, with 51 to 93 texture
  refusals against 296 (run auto-ram-2326). Picture first, frame rate second.

.NOTES
  The switch set is Vice-Tay's scripts/play.ps1 -Machine 4070 (RTX 4070 / 8-core profile), not -Safe, windowed, no
  panel (our overlay replaces it), Series X. Measured 2026-11-26 on an RTX 4070 SUPER + i7-13700KF, Series X, warm
  cache: about 20 fps median in the world, against 14.2 for the Neptune set it replaces.
  VICTORY_D3D12_DRED=light is part of the measured set: it keeps the lists without markers and only matters in the
  diagnostics build; -Dred sets the full mode over it.
  Discord Rich Presence stays off (VICTORY_DISCORD=0).

.EXAMPLE
  .\tools\lancer_victory.ps1 -GameDir D:\Games\MyPackage -Warm

.EXAMPLE
  .\tools\lancer_victory.ps1 -GameDir D:\Games\MyPackage -XgDll D:\MyGdk\xg_xs.dll
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$GameDir,
    [switch]$Warm,
    [switch]$Precompile,
    [switch]$AutoA,
    [switch]$FrametimeDump,
    [switch]$Diag,
    [string]$SkipPipelines,
    [switch]$NoTunedSettings,
    [switch]$Dred,
    [string]$XgDll,
    [ValidateSet('real', 'compare')][string]$XgMode = 'real',
    [switch]$BatchCardRehash,
    [switch]$Quality,
    [hashtable]$Override = @{},
    # Copia local (OpenBot): modelo de console que o jogo ve. Vazio = padrao do victory (series-x).
    [ValidateSet('', 'series-x', 'series-s')][string]$Console = ''
)

$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot
$victory = Join-Path $repo 'third_party\victory'
$manifest = Join-Path $victory 'Cargo.toml'

if (-not (Test-Path -LiteralPath $manifest)) {
    throw "third_party/victory/Cargo.toml is missing: this copy of AnyXboxX is incomplete."
}

$gameExe = Join-Path $GameDir 'game_scarlett_master.exe'
if (-not (Test-Path -LiteralPath $gameExe)) {
    throw "game_scarlett_master.exe not found in '$GameDir'."
}

# VIctory is its own Cargo workspace: its target folder is third_party\victory\target
# unless CARGO_TARGET_DIR is set.
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $victory 'target' }
$useDiag = $Diag -or $Dred
if ($useDiag) {
    $profileArgs = @('--profile', 'diag', '-p', 'victory', '--features', 'diagnostics')
    $exe = Join-Path $target 'diag\victory.exe'
} else {
    $profileArgs = @('--release', '-p', 'victory')
    $exe = Join-Path $target 'release\victory.exe'
}

# Always ask cargo: it does nothing when victory.exe is current, and rebuilds it after a pull that changes
# third_party/victory (an old victory.exe would ignore the settings the new tree reads).
Write-Host "Building VIctory ($(if ($useDiag) { 'diag profile, diagnostics feature' } else { 'release' }))..."
# Windows PowerShell 5.1 wraps redirected native stderr (`2>`) in NativeCommandError, which
# $ErrorActionPreference = 'Stop' turns into a terminating error on benign warnings; rely on $LASTEXITCODE.
$prevErrorAction = $ErrorActionPreference
try {
    $ErrorActionPreference = 'Continue'
    & cargo build @profileArgs --manifest-path $manifest
} finally {
    $ErrorActionPreference = $prevErrorAction
}
if ($LASTEXITCODE -ne 0) { throw "cargo build failed (exit code $LASTEXITCODE)." }

# No victory.toml is needed: VIctory reads the game folder from VICTORY_GAME_DIR.
$env:VICTORY_GAME_DIR = $GameDir
# Discord Rich Presence stays off (our copy also needs VICTORY_DISCORD=1 to turn it on).
$env:VICTORY_DISCORD = '0'
if (-not $NoTunedSettings) {
    # Vice-Tay scripts/play.ps1 -Machine 4070, not -Safe, windowed, no panel, Series X.
    $tuned = [ordered]@{
        VICTORY_TEXTURE_NO_WAIT      = '1'
        VICTORY_RELAY                = '2'
        VICTORY_WIDE_BACK            = '1'
        VICTORY_CARD_REHASH_RUNS     = '1'
        VICTORY_JOIN_COPIES          = '1'
        VICTORY_INDIRECT_WINDOW      = '1'
        VICTORY_RELAY_COPY           = '1'
        VICTORY_CARD_STEPS           = '2'
        VICTORY_TITLE_PRIORITIES     = '0'
        VICTORY_SHOW_AFTER           = '1'
        VICTORY_AMBIENT              = '0.8'
        VICTORY_D3D12_DRED           = 'light'
        VICTORY_WAVE_WIDE_ONLY       = '6f225b641c5b0708,5dc3b25986622949,369cbe906ea72764,2cc4c197ee29e291'
        VICTORY_THIN_THREADS         = '8'
        VICTORY_UNDER_THREADS        = '6'
        VICTORY_LAND_THREADS         = '4'
        VICTORY_MAP_THREADS          = '1'
        VICTORY_PIPELINE_WORKERS     = '3'
        VICTORY_CARD_RESERVE_BUDGETS = '1'
        VICTORY_CARD_BUFFERS_MIN_KB  = '64'
        VICTORY_CARD_BUFFERS_MB      = '1536'
        VICTORY_CARD_WINDOWS_MB      = '1024'
        VICTORY_THIN                 = '1'
        VICTORY_JOIN_PARTS           = '8'
        VICTORY_UPLOAD_AHEAD         = '1'
        VICTORY_INDIRECT_BATCH       = '2'
    }
    # Copia local (OpenBot): pools de 16 nucleos do Vice-Tay, como o anyxboxx run faz desde f99cbee (19,6 -> 23,8 fps no autor).
    if ([Environment]::ProcessorCount -ge 16) { $tuned['VICTORY_THIN_THREADS'] = '12'; $tuned['VICTORY_UNDER_THREADS'] = '8'; $tuned['VICTORY_LAND_THREADS'] = '6' }
    foreach ($name in $tuned.Keys) {
        Set-Item -Path "Env:$name" -Value $tuned[$name]
    }
}
if ($Quality) {
    # Picture first: windows and buffers are read fresh each frame instead of kept as card copies (0 = no windows),
    # with more VRAM headroom for the textures. About half the frame rate of the default set.
    $env:VICTORY_CARD_WINDOWS_MB = '0'
    $env:VICTORY_CARD_BUFFERS_MB = '1024'
    $env:VICTORY_TEXTURE_STAGE_MB = '192'
}
foreach ($name in $Override.Keys) {
    if ($name -notlike 'VICTORY_*') { throw "-Override takes VICTORY_* names only: '$name'." }
    Set-Item -Path "Env:$name" -Value ([string]$Override[$name])
}
if ($AutoA) {
    $env:VICTORY_AUTO_A = '150'
}
if ($FrametimeDump) {
    $env:VICTORY_FRAMETIME_DUMP = '1'
}
# The tuned set carries VICTORY_D3D12_DRED=light; -Dred sets the full mode after it (and after -Override), so it wins.
if ($Dred) {
    $env:VICTORY_D3D12_DRED = '1'
}
if ($BatchCardRehash) {
    $env:VICTORY_CARD_REHASH_RUNS = '1'
}
if ($PSBoundParameters.ContainsKey('SkipPipelines') -and $SkipPipelines) {
    $env:VICTORY_SKIP_PIPELINES = $SkipPipelines
}
# Your own copy of the GDK's offline PC xg_xs.dll (never in this repository): Microsoft's library lays out the textures.
if ($PSBoundParameters.ContainsKey('XgDll') -and $XgDll) {
    if (-not (Test-Path -LiteralPath $XgDll -PathType Leaf)) {
        throw "xg_xs.dll not found: '$XgDll'."
    }
    $env:VICTORY_XG_DLL = (Resolve-Path -LiteralPath $XgDll).Path
    $env:VICTORY_XG_MODE = $XgMode
}

if ($Warm) {
    # Same environment as the game below: the cache key depends on it. Only the game folder's own .rpf files.
    $archives = @(Get-ChildItem -LiteralPath $GameDir -Filter '*.rpf' -File | Sort-Object Name | ForEach-Object { $_.FullName })
    if ($archives.Count -eq 0) {
        throw "-Warm found no .rpf archive in '$GameDir'."
    }
    Write-Host "Warming the shader cache from $($archives.Count) archives (about 7 minutes the first time)..."
    $prevErrorAction = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        & $exe warm
    } finally {
        $ErrorActionPreference = $prevErrorAction
    }
    if ($LASTEXITCODE -ne 0) { throw "victory warm failed (exit code $LASTEXITCODE)." }
}

if ($Precompile) {
    $shaders = Join-Path $GameDir 'shaders.rpf'
    $dlls = @('XboxBvhBuilder.dll', 'amd_fidelityfx_upscaler_xbox.dll') | ForEach-Object { Join-Path $GameDir $_ }
    foreach ($needed in @($shaders) + $dlls) {
        if (-not (Test-Path -LiteralPath $needed -PathType Leaf)) {
            throw "-Precompile needs '$needed'."
        }
    }
    Write-Host 'Precompiling the shaders of shaders.rpf, the game and its DLLs into the cache...'
    $prevErrorAction = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        & $exe warm
    } finally {
        $ErrorActionPreference = $prevErrorAction
    }
    if ($LASTEXITCODE -ne 0) { throw "victory warm failed (exit code $LASTEXITCODE)." }
}

$prevErrorAction = $ErrorActionPreference
try {
    $ErrorActionPreference = 'Continue'
    if ($Console) { & $exe run $gameExe --execute --console $Console } else { & $exe run $gameExe --execute }
} finally {
    $ErrorActionPreference = $prevErrorAction
}
exit $LASTEXITCODE
