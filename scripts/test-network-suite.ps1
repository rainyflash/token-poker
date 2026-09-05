[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$nodeExecutable = (Get-Command node -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
$invocations = [System.Collections.Generic.List[string]]::new()

function node {
    $invocations.Add(($args -join ' '))
    & $nodeExecutable -e 'process.exit(23)'
}

$failure = $null
try {
    & (Join-Path $PSScriptRoot 'verify-network-suite.ps1')
}
catch {
    $failure = $_.Exception.Message
}
if ($invocations.Count -ne 1 -or $null -eq $failure -or -not $failure.Contains('退出码 23')) {
    throw '联网测试失败后未立即停止，后续命令可能掩盖真实错误。'
}
Write-Output '联网测试失败即停止的回归验证通过。'
exit 0
