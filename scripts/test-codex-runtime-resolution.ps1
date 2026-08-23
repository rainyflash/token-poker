[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
. (Join-Path $projectRoot 'scripts\codex-runtime.ps1')

$temporaryBase = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$testRoot = [System.IO.Path]::GetFullPath(
    (Join-Path $temporaryBase "token-poker-codex-resolution-$([Guid]::NewGuid().ToString('N'))")
)
Assert-ChildDirectory -ParentDirectory $temporaryBase -ChildDirectory $testRoot

function Assert-Equal {
    param(
        [AllowNull()][object]$Actual,
        [AllowNull()][object]$Expected,
        [Parameter(Mandatory)][string]$Message
    )
    if ($Actual -cne $Expected) {
        throw "$Message Expected '$Expected', received '$Actual'."
    }
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

try {
    New-Item -ItemType Directory -Path $testRoot -Force | Out-Null

    $cliPath = Join-Path $testRoot 'codex.ps1'
    [System.IO.File]::WriteAllText($cliPath, "exit 0`n", [System.Text.Encoding]::ASCII)
    $commandResolver = {
        param([string]$Name)
        if ($Name -eq 'codex') {
            return [pscustomobject]@{ Source = $cliPath }
        }
        return $null
    }
    $resolvedCli = Resolve-CodexCliCommandPath `
        -ExplicitPath $null `
        -CommandResolver $commandResolver
    Assert-Equal `
        -Actual $resolvedCli `
        -Expected (Resolve-Path -LiteralPath $cliPath).Path `
        -Message 'The PATH-backed Codex command was not resolved.'

    $packageRoot = Join-Path $testRoot 'WindowsApps\OpenAI.Codex_99.0.0.0_x64'
    $desktopBinary = Join-Path $packageRoot 'app\resources\codex.exe'
    New-Item -ItemType Directory -Path (Split-Path -Parent $desktopBinary) -Force | Out-Null
    [System.IO.File]::WriteAllBytes($desktopBinary, [byte[]](0, 1, 2, 3, 4, 5))
    $appPackageResolver = {
        @(
            [pscustomobject]@{
                InstallLocation = $packageRoot
                Version = [version]'99.0.0.0'
            }
        )
    }
    $missingCommandResolver = {
        param([string]$Name)
        return $null
    }
    $missingCli = Resolve-CodexCliCommandPath `
        -ExplicitPath $null `
        -CommandResolver $missingCommandResolver
    Assert-Equal `
        -Actual $missingCli `
        -Expected $null `
        -Message 'The missing-PATH fixture unexpectedly resolved a CLI command.'

    $resolvedDesktop = Resolve-CodexDesktopBinaryPath `
        -ExplicitPath $null `
        -AppPackageResolver $appPackageResolver `
        -KnownPaths @() `
        -AdditionalCandidates @()
    Assert-Equal `
        -Actual $resolvedDesktop `
        -Expected (Resolve-Path -LiteralPath $desktopBinary).Path `
        -Message 'The installed Codex desktop App package was not resolved without PATH.'

    $bootstrapBase = Join-Path $testRoot 'bootstrap'
    $bootstrap = New-CodexBootstrapCopy `
        -SourcePath $resolvedDesktop `
        -BaseDirectory $bootstrapBase
    Assert-True `
        -Condition (Test-Path -LiteralPath $bootstrap.CommandPath -PathType Leaf) `
        -Message 'The executable bootstrap copy was not created.'
    Assert-Equal `
        -Actual (Get-FileHash -LiteralPath $bootstrap.CommandPath -Algorithm SHA256).Hash `
        -Expected (Get-FileHash -LiteralPath $resolvedDesktop -Algorithm SHA256).Hash `
        -Message 'The executable bootstrap copy changed bytes.'
    Remove-CodexBootstrapCopy `
        -DirectoryPath $bootstrap.DirectoryPath `
        -BaseDirectory $bootstrap.BaseDirectory
    Assert-True `
        -Condition (-not (Test-Path -LiteralPath $bootstrap.DirectoryPath)) `
        -Message 'The executable bootstrap copy was not removed.'

    $missingDesktop = Resolve-CodexDesktopBinaryPath `
        -ExplicitPath $null `
        -AppPackageResolver { @() } `
        -KnownPaths @() `
        -AdditionalCandidates @()
    Assert-Equal `
        -Actual $missingDesktop `
        -Expected $null `
        -Message 'The empty desktop fixture unexpectedly resolved an executable.'

    Write-Output 'Codex runtime resolution tests passed.'
}
finally {
    if (Test-Path -LiteralPath $testRoot) {
        Remove-Item -LiteralPath $testRoot -Recurse -Force
    }
}
