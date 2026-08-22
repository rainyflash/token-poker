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
    [bool]$EnableRendezvous = $true,
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$sidecarPath = Join-Path $projectRoot 'target\release\token-holdem-sidecar.exe'

if (-not $SkipBuild) {
    & cargo build --release --manifest-path (Join-Path $projectRoot 'Cargo.toml') -p token-holdem-sidecar
    if ($LASTEXITCODE -ne 0) {
        throw '志愿归档 sidecar 编译失败。'
    }
}
if (-not (Test-Path -LiteralPath $sidecarPath -PathType Leaf)) {
    throw "缺少 sidecar：$sidecarPath。请移除 -SkipBuild 后重试。"
}

$parent = Split-Path -Parent $DataDirectory
if ([string]::IsNullOrWhiteSpace($parent)) {
    $parent = (Get-Location).Path
}
if (-not (Test-Path -LiteralPath $parent -PathType Container)) {
    New-Item -ItemType Directory -Path $parent | Out-Null
}
$resolvedParent = (Resolve-Path -LiteralPath $parent).Path
$leaf = Split-Path -Leaf $DataDirectory
if ([string]::IsNullOrWhiteSpace($leaf)) {
    throw 'DataDirectory 必须指向一个具体目录，不能是卷根目录。'
}
$resolvedDataDirectory = Join-Path $resolvedParent $leaf
if (-not (Test-Path -LiteralPath $resolvedDataDirectory -PathType Container)) {
    New-Item -ItemType Directory -Path $resolvedDataDirectory | Out-Null
}
$resolvedDataDirectory = (Resolve-Path -LiteralPath $resolvedDataDirectory).Path

$arguments = @(
    '--archive-dir',
    $resolvedDataDirectory,
    '--relay-server',
    '--public-node',
    '--daemon',
    '--volunteer-consent=granted',
    '--network-cost=unmetered',
    '--power-source=ac'
)
if ($EnableRendezvous) {
    $arguments += '--rendezvous-server'
}
foreach ($address in $ListenAddress) {
    $arguments += @('--listen', $address)
}
foreach ($address in $ExternalAddress) {
    $arguments += @('--external-address', $address)
}
& $sidecarPath @arguments
if ($LASTEXITCODE -ne 0) {
    throw "志愿归档节点异常退出，代码 $LASTEXITCODE。"
}
