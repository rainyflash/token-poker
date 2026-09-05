[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$checks = @(
    @('scripts/verify-volunteer-network.mjs'),
    @('scripts/verify-p2p-hand.mjs'),
    @('scripts/verify-p2p-hand.mjs', '--relay'),
    @('scripts/verify-dynamic-table.mjs'),
    @('scripts/verify-safe-leave.mjs'),
    @('scripts/verify-complete-session.mjs')
)

foreach ($arguments in $checks) {
    & node @arguments
    if ($LASTEXITCODE -ne 0) {
        throw "联网验证失败：node $($arguments -join ' ')，退出码 $LASTEXITCODE"
    }
}
Write-Output '全部六项联网验证通过。'
