[CmdletBinding(DefaultParameterSetName = 'Hardware')]
param(
    [Parameter(Mandatory, ParameterSetName = 'Hardware')]
    [ValidateNotNullOrEmpty()]
    [string] $Adapter,

    [Parameter(Mandatory, ParameterSetName = 'Warp')]
    [switch] $Warp,

    [ValidatePattern('^[A-Za-z0-9_-]{1,40}$')]
    [string] $Label = 'rt',

    [string] $Registry = (Join-Path $env:LOCALAPPDATA 'AnyXboxX-RT'),

    [switch] $Offline
)

$ErrorActionPreference = 'Stop'
$Registry = [IO.Path]::GetFullPath($Registry)
$repo = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $repo
$oldAdapter = $env:VICTORY_D3D12_ADAPTER
$oldWarp = $env:VICTORY_WARP
$lock = $null
$owner = $null
try {
    $sha = git rev-parse --verify HEAD
    if ($LASTEXITCODE -ne 0) { throw 'Unable to read the Git revision.' }
    $parent = git log -1 '--format=%P'
    $dirty = @(git status --porcelain=v1)
    $build = @('test', '--manifest-path', 'third_party/victory/Cargo.toml', '--locked',
        '--release', '-p', 'victory-shader', '--lib', '--no-run', '--message-format=json')
    if ($Offline) { $build += '--offline' }
    $executables = @(cargo @build | ForEach-Object {
        $message = $_ | ConvertFrom-Json
        if ($message.reason -eq 'compiler-artifact' -and $message.target.name -eq 'victory_shader' -and $message.executable) {
            $message.executable
        }
        if ($message.reason -eq 'compiler-message' -and $message.message.rendered) {
            [Console]::Error.Write($message.message.rendered)
        }
    })
    if ($LASTEXITCODE -ne 0 -or $executables.Count -ne 1) { throw 'The release test executable could not be built.' }
    $exe = $executables[0]

    $env:VICTORY_D3D12_ADAPTER = $Adapter
    $env:VICTORY_WARP = if ($PSCmdlet.ParameterSetName -eq 'Warp') { '1' } else { '0' }
    New-Item -ItemType Directory -Path $Registry -Force | Out-Null
    $stamp = [DateTime]::UtcNow.ToString("yyyyMMddTHHmmssfff'Z'")
    $log = Join-Path $Registry "$stamp-$Label-$($sha.Substring(0, 8))-$PID.log"
    $test = 'emit::intersect_tests::bench::synthetic_rt_frames'
    $run = @($test, '--exact', '--ignored', '--nocapture', '--test-threads=1')
    @(
        "sha=$sha"
        "parents=$parent"
        "worktree=$repo"
        "worktree_status=$(if ($dirty.Count) { $dirty -join ' | ' } else { 'clean' })"
        "rustc=$(rustc -V)"
        "cargo=$(cargo -V)"
        "windows=$([Environment]::OSVersion.Version)"
        "build=cargo $($build -join ' ')"
        "run=& '$exe' $($run -join ' ')"
        "exe_sha256=$((Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash)"
    ) | Set-Content -LiteralPath $log -Encoding UTF8
    Get-ChildItem Env: | Where-Object {
        $_.Name -match '^(VICTORY_|CARGO_PROFILE_RELEASE_|RUSTFLAGS$|CARGO_ENCODED_RUSTFLAGS$)'
    } | Sort-Object Name | ForEach-Object { "env:$($_.Name)=$($_.Value)" } | Add-Content -LiteralPath $log -Encoding UTF8

    $lockPath = Join-Path $Registry 'gpu.lock'
    $ownerPath = Join-Path $Registry 'gpu-lock.txt'
    $deadline = [DateTime]::UtcNow.AddMinutes(10)
    'Waiting for the shared GPU turn.' | Tee-Object -FilePath $log -Append
    while ($null -eq $owner) {
        try {
            $lock = [IO.File]::Open($lockPath, 'OpenOrCreate', 'ReadWrite', 'None')
            $owner = [IO.File]::Open($ownerPath, 'CreateNew', 'Write', 'Read')
        } catch [IO.IOException] {
            if ($null -ne $lock) { $lock.Dispose(); $lock = $null }
            if (($_.Exception.HResult -band 0xffff) -notin (32, 33, 80, 183) -or [DateTime]::UtcNow -ge $deadline) { throw }
            Start-Sleep -Seconds 2
        }
    }
    $ownerText = "owner=$(git branch --show-current) pid=$PID benchmark=$Label sha=$sha started=$([DateTime]::UtcNow.ToString('o'))`n"
    $ownerBytes = [Text.Encoding]::UTF8.GetBytes($ownerText)
    $owner.Write($ownerBytes, 0, $ownerBytes.Length)
    $owner.Flush()
    "gpu_acquired_utc=$([DateTime]::UtcNow.ToString('o'))" | Tee-Object -FilePath $log -Append
    if (Get-Command nvidia-smi -ErrorAction SilentlyContinue) {
        nvidia-smi --query-gpu=name,driver_version,pci.bus_id,memory.used,utilization.gpu,temperature.gpu --format=csv |
            Tee-Object -FilePath $log -Append
    }
    & $exe @run 2>&1 | Tee-Object -FilePath $log -Append | Where-Object { $_ -notmatch '^sample,[^,]+,(off|on),' }
    $result = $LASTEXITCODE
    "exit=$result log=$log" | Tee-Object -FilePath $log -Append
    if ($result -ne 0) { throw "Synthetic RT failed with exit code $result. See $log" }
} finally {
    try {
        if ($null -ne $owner) {
            $owner.Dispose()
            [IO.File]::Delete($ownerPath)
        }
    } finally {
        if ($null -ne $lock) { $lock.Dispose() }
        $env:VICTORY_D3D12_ADAPTER = $oldAdapter
        $env:VICTORY_WARP = $oldWarp
        Pop-Location
    }
}
