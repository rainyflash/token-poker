[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$entrypointSource = Join-Path $projectRoot 'scripts\install-token-poker.cmd'
$temporaryBase = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$testRoot = [System.IO.Path]::GetFullPath(
    (Join-Path $temporaryBase "token-poker-installer-$([Guid]::NewGuid().ToString('N'))")
)
$temporaryPrefix = $temporaryBase.TrimEnd(
    [System.IO.Path]::DirectorySeparatorChar,
    [System.IO.Path]::AltDirectorySeparatorChar
) + [System.IO.Path]::DirectorySeparatorChar
if (-not $testRoot.StartsWith($temporaryPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Installer test path escaped the temporary directory: $testRoot"
}

$originalEnvironment = @{
    TOKEN_POKER_INSTALLER_NO_PAUSE = $env:TOKEN_POKER_INSTALLER_NO_PAUSE
    TOKEN_POKER_TEST_ARGUMENT_FILE = $env:TOKEN_POKER_TEST_ARGUMENT_FILE
    TOKEN_POKER_TEST_EXIT_CODE = $env:TOKEN_POKER_TEST_EXIT_CODE
    LOCALAPPDATA = $env:LOCALAPPDATA
}

function Assert-True {
    param(
        [Parameter(Mandatory)][bool]$Condition,
        [Parameter(Mandatory)][string]$Message
    )
    if (-not $Condition) {
        throw $Message
    }
}

function Invoke-Entrypoint {
    param([Parameter(Mandatory)][string]$Entrypoint)

    $output = @(& $Entrypoint 2>&1)
    return @{
        exit_code = [int]$LASTEXITCODE
        output = ($output -join "`n")
    }
}

try {
    $packageRoot = Join-Path $testRoot 'Package & (Spaces)'
    New-Item -ItemType Directory -Path $packageRoot -Force | Out-Null
    Copy-Item -LiteralPath $entrypointSource -Destination (Join-Path $packageRoot 'Install Token Poker.cmd')

    $requiredFiles = @(
        'manifest.json',
        'install-token-poker.ps1',
        '.agents\plugins\marketplace.json',
        'plugins\token-holdem\.mcp.json',
        'plugins\token-holdem\.codex-plugin\plugin.json',
        'plugins\token-holdem\release-files.json'
    )
    foreach ($relativePath in $requiredFiles) {
        $filePath = Join-Path $packageRoot $relativePath
        New-Item -ItemType Directory -Path (Split-Path -Parent $filePath) -Force | Out-Null
        [System.IO.File]::WriteAllText($filePath, "`n", [System.Text.Encoding]::ASCII)
    }

    $installerProbe = @'
[CmdletBinding()]
param([string]$LogPath)

@{
    log_path = $LogPath
} |
    ConvertTo-Json -Compress |
    Set-Content -LiteralPath $env:TOKEN_POKER_TEST_ARGUMENT_FILE -Encoding ASCII
exit [int]$env:TOKEN_POKER_TEST_EXIT_CODE
'@
    [System.IO.File]::WriteAllText(
        (Join-Path $packageRoot 'install-token-poker.ps1'),
        $installerProbe,
        [System.Text.Encoding]::ASCII
    )
    $argumentFile = Join-Path $testRoot 'arguments.txt'
    $env:TOKEN_POKER_INSTALLER_NO_PAUSE = '1'
    $env:TOKEN_POKER_TEST_ARGUMENT_FILE = $argumentFile
    $env:LOCALAPPDATA = Join-Path $testRoot 'Local App Data'

    $entrypoint = Join-Path $packageRoot 'Install Token Poker.cmd'
    $env:TOKEN_POKER_TEST_EXIT_CODE = '0'
    $success = Invoke-Entrypoint -Entrypoint $entrypoint
    Assert-True `
        -Condition ($success.exit_code -eq 0) `
        -Message "Entrypoint success returned $($success.exit_code):`n$($success.output)"
    Assert-True -Condition ($success.output.Contains('installed successfully')) -Message 'Entrypoint omitted its success message.'
    Assert-True `
        -Condition (Test-Path -LiteralPath $argumentFile -PathType Leaf) `
        -Message "Entrypoint did not run the installer probe:`n$($success.output)"
    $probeResult = Get-Content -LiteralPath $argumentFile -Raw -Encoding ASCII | ConvertFrom-Json
    Assert-True -Condition (-not [string]::IsNullOrWhiteSpace($probeResult.log_path)) -Message 'Entrypoint did not pass an installer log path.'
    $entrypointSourceText = Get-Content -LiteralPath $entrypointSource -Raw -Encoding ASCII
    foreach ($requiredArgument in @('-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy Bypass', '-File', '-LogPath')) {
        Assert-True -Condition ($entrypointSourceText.Contains($requiredArgument)) -Message "Entrypoint omitted $requiredArgument."
    }

    $env:TOKEN_POKER_TEST_EXIT_CODE = '23'
    $failure = Invoke-Entrypoint -Entrypoint $entrypoint
    Assert-True -Condition ($failure.exit_code -eq 23) -Message "Entrypoint failed to preserve exit code 23: $($failure.exit_code)."
    Assert-True -Condition ($failure.output.Contains('installation failed with exit code 23')) -Message 'Entrypoint omitted its failure message.'

    Remove-Item -LiteralPath (Join-Path $packageRoot 'manifest.json') -Force
    Remove-Item -LiteralPath $argumentFile -Force
    $incomplete = Invoke-Entrypoint -Entrypoint $entrypoint
    Assert-True -Condition ($incomplete.exit_code -eq 2) -Message "Incomplete package returned $($incomplete.exit_code)."
    Assert-True -Condition ($incomplete.output.Contains('package is incomplete')) -Message 'Entrypoint omitted its incomplete-package message.'
    Assert-True -Condition (-not (Test-Path -LiteralPath $argumentFile)) -Message 'Incomplete package still launched PowerShell.'

    Write-Output 'Token Poker installer entrypoint tests passed.'
}
finally {
    foreach ($name in $originalEnvironment.Keys) {
        [Environment]::SetEnvironmentVariable($name, $originalEnvironment[$name], 'Process')
    }
    if (Test-Path -LiteralPath $testRoot) {
        Remove-Item -LiteralPath $testRoot -Recurse -Force
    }
}
