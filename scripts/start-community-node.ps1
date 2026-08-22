[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$DataDirectory,
    [Parameter(Mandatory)]
    [string[]]$ExternalAddress,
    [string[]]$ListenAddress = @(
        '/ip4/0.0.0.0/tcp/4001',
        '/ip4/0.0.0.0/udp/4001/quic-v1'
    ),
    [ValidateRange(1, 512)]
    [int]$MaxReservations = 64,
    [ValidateRange(1, 128)]
    [int]$MaxCircuits = 16,
    [ValidateRange(60, 86400)]
    [int]$MaxCircuitSeconds = 7200,
    [ValidateRange(65536, 1073741824)]
    [long]$MaxCircuitBytes = 67108864,
    [bool]$EnableArchive = $true,
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$sidecarPath = Join-Path $projectRoot 'target\release\token-holdem-sidecar.exe'

if (-not $SkipBuild) {
    & cargo build --locked --release --manifest-path (Join-Path $projectRoot 'Cargo.toml') -p token-holdem-sidecar
    if ($LASTEXITCODE -ne 0) {
        throw '社区节点 sidecar 编译失败。'
    }
}
if (-not (Test-Path -LiteralPath $sidecarPath -PathType Leaf)) {
    throw "缺少 sidecar：$sidecarPath。请移除 -SkipBuild 后重试。"
}

$absoluteDataDirectory = [System.IO.Path]::GetFullPath($DataDirectory)
$dataRoot = [System.IO.Path]::GetPathRoot($absoluteDataDirectory)
if ($absoluteDataDirectory.TrimEnd('\') -eq $dataRoot.TrimEnd('\')) {
    throw 'DataDirectory 必须指向具体目录，不能是卷根目录。'
}
New-Item -ItemType Directory -Path $absoluteDataDirectory -Force | Out-Null
$absoluteDataDirectory = (Resolve-Path -LiteralPath $absoluteDataDirectory).Path
$nodeKeyPath = Join-Path $absoluteDataDirectory 'libp2p-identity-key'

$arguments = @(
    '--rendezvous-server',
    '--relay-server',
    '--public-node',
    '--daemon',
    '--volunteer-consent=granted',
    '--network-cost=unmetered',
    '--power-source=ac',
    "--node-key-file=$nodeKeyPath",
    "--relay-max-reservations=$MaxReservations",
    "--relay-max-circuits=$MaxCircuits",
    "--relay-circuit-seconds=$MaxCircuitSeconds",
    "--relay-circuit-bytes=$MaxCircuitBytes"
)
if ($EnableArchive) {
    $arguments += "--archive-dir=$absoluteDataDirectory"
}
foreach ($address in $ListenAddress) {
    $arguments += @('--listen', $address)
}
foreach ($address in $ExternalAddress) {
    $arguments += @('--external-address', $address)
}

& $sidecarPath @arguments
if ($LASTEXITCODE -ne 0) {
    throw "社区节点异常退出，代码 $LASTEXITCODE。"
}
