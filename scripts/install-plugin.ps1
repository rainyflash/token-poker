[CmdletBinding()]
param(
    [switch]$Upgrade
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$marketplaceRoot = if (Test-Path -LiteralPath (Join-Path $PSScriptRoot '.agents\plugins\marketplace.json')) {
    (Resolve-Path $PSScriptRoot).Path
}
else {
    (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
}
$marketplaceName = 'token-holdem-community'
$pluginSelector = "token-holdem@$marketplaceName"
$pluginManifestPath = Join-Path $marketplaceRoot 'plugins\token-holdem\.codex-plugin\plugin.json'
$pluginVersion = (
    Get-Content -LiteralPath $pluginManifestPath -Raw -Encoding UTF8 |
        ConvertFrom-Json
).version
$codexCommand = Get-Command codex -ErrorAction SilentlyContinue
if ($null -eq $codexCommand) {
    throw 'Codex CLI was not found. Install or update Codex desktop and make sure the codex command is available.'
}

function Find-CodexDesktopBinary {
    $appPackages = @(
        Get-AppxPackage -Name 'OpenAI.Codex' -ErrorAction SilentlyContinue |
            Sort-Object Version -Descending
    )
    foreach ($package in $appPackages) {
        $candidatePath = Join-Path $package.InstallLocation 'app\resources\codex.exe'
        if (Test-Path -LiteralPath $candidatePath -PathType Leaf) {
            return (Resolve-Path -LiteralPath $candidatePath).Path
        }
    }

    if (-not [string]::IsNullOrWhiteSpace($env:CODEX_CLI_PATH) -and
        (Test-Path -LiteralPath $env:CODEX_CLI_PATH -PathType Leaf)) {
        return (Resolve-Path -LiteralPath $env:CODEX_CLI_PATH).Path
    }

    throw 'Could not locate the App Server bundled with Codex desktop. Update or reinstall Codex desktop first.'
}

function Get-CodexHomeRoot {
    if (-not [string]::IsNullOrWhiteSpace($env:CODEX_HOME)) {
        return [System.IO.Path]::GetFullPath($env:CODEX_HOME)
    }
    return [System.IO.Path]::GetFullPath((Join-Path $env:USERPROFILE '.codex'))
}

function Get-PluginCacheDirectory {
    param([Parameter(Mandatory)][string]$Version)

    $codexHomeRoot = Get-CodexHomeRoot
    return [System.IO.Path]::GetFullPath(
        (Join-Path $codexHomeRoot "plugins\cache\$marketplaceName\token-holdem\$Version")
    )
}

function Prepare-OfficialUsageRuntime {
    param([Parameter(Mandatory)][string]$Version)

    $pluginCacheDirectory = Get-PluginCacheDirectory -Version $Version
    if (-not (Test-Path -LiteralPath $pluginCacheDirectory -PathType Container)) {
        throw "Codex did not create the Token Poker plugin cache: $pluginCacheDirectory"
    }

    $sourceFile = Find-CodexDesktopBinary
    $targetDirectory = Join-Path $pluginCacheDirectory 'bin'
    $targetFile = Join-Path $targetDirectory 'codex-app-server.exe'
    New-Item -ItemType Directory -Path $targetDirectory -Force | Out-Null

    if (Test-Path -LiteralPath $targetFile -PathType Leaf) {
        $sourceDigest = (Get-FileHash -LiteralPath $sourceFile -Algorithm SHA256).Hash
        $targetDigest = (Get-FileHash -LiteralPath $targetFile -Algorithm SHA256).Hash
        if ($sourceDigest -eq $targetDigest) {
            return (Get-Item -LiteralPath $targetFile).Length
        }
    }

    $partialFile = Join-Path $targetDirectory "codex-app-server.$PID.partial"
    try {
        Copy-Item -LiteralPath $sourceFile -Destination $partialFile -Force
        $sourceDigest = (Get-FileHash -LiteralPath $sourceFile -Algorithm SHA256).Hash
        $partialDigest = (Get-FileHash -LiteralPath $partialFile -Algorithm SHA256).Hash
        if ($sourceDigest -ne $partialDigest) {
            throw 'The local Codex App Server copy failed SHA-256 verification.'
        }
        Move-Item -LiteralPath $partialFile -Destination $targetFile -Force
    }
    finally {
        if (Test-Path -LiteralPath $partialFile -PathType Leaf) {
            Remove-Item -LiteralPath $partialFile -Force
        }
    }

    & $targetFile --version *> $null
    if ($LASTEXITCODE -ne 0) {
        throw 'The local Codex App Server copy could not start.'
    }
    return (Get-Item -LiteralPath $targetFile).Length
}

function Invoke-CodexCommand {
    param([Parameter(Mandatory)][string[]]$Arguments)

    $originalErrorPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        & $codexCommand.Source @Arguments *> $null
        return [int]$LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $originalErrorPreference
    }
}

function Copy-VerifiedFile {
    param(
        [Parameter(Mandatory)][string]$SourceFile,
        [Parameter(Mandatory)][string]$TargetFile
    )

    if (-not (Test-Path -LiteralPath $SourceFile -PathType Leaf)) {
        throw "Plugin source file does not exist: $SourceFile"
    }

    $targetDirectory = Split-Path -Parent $TargetFile
    New-Item -ItemType Directory -Path $targetDirectory -Force | Out-Null
    Copy-Item -LiteralPath $SourceFile -Destination $TargetFile -Force

    $sourceDigest = (Get-FileHash -LiteralPath $SourceFile -Algorithm SHA256).Hash
    $targetDigest = (Get-FileHash -LiteralPath $TargetFile -Algorithm SHA256).Hash
    if ($sourceDigest -ne $targetDigest) {
        throw "Plugin cache file failed SHA-256 verification: $TargetFile"
    }
}

function Copy-VerifiedManifest {
    param(
        [Parameter(Mandatory)][string]$SourceFile,
        [Parameter(Mandatory)][string]$TargetFile
    )

    Get-Content -LiteralPath $SourceFile -Raw -Encoding UTF8 | ConvertFrom-Json | Out-Null
    Copy-VerifiedFile -SourceFile $SourceFile -TargetFile $TargetFile
    Get-Content -LiteralPath $TargetFile -Raw -Encoding UTF8 | ConvertFrom-Json | Out-Null
}

function Complete-PluginCacheManifests {
    param([Parameter(Mandatory)][string]$Version)

    $pluginCacheDirectory = Get-PluginCacheDirectory -Version $Version
    if (-not (Test-Path -LiteralPath $pluginCacheDirectory -PathType Container)) {
        throw "Codex did not create the Token Poker plugin cache: $pluginCacheDirectory"
    }

    $pluginSourceDirectory = Join-Path $marketplaceRoot 'plugins\token-holdem'
    Copy-VerifiedManifest `
        -SourceFile (Join-Path $pluginSourceDirectory '.mcp.json') `
        -TargetFile (Join-Path $pluginCacheDirectory '.mcp.json')
    Copy-VerifiedManifest `
        -SourceFile (Join-Path $pluginSourceDirectory '.codex-plugin\plugin.json') `
        -TargetFile (Join-Path $pluginCacheDirectory '.codex-plugin\plugin.json')
}

function Sync-PluginCachePayload {
    param([Parameter(Mandatory)][string]$Version)

    $pluginCacheDirectory = Get-PluginCacheDirectory -Version $Version
    $pluginSourceDirectory = Join-Path $marketplaceRoot 'plugins\token-holdem'
    $payloadManifestPath = Join-Path $pluginSourceDirectory 'release-files.json'
    $payloadManifest = Get-Content -LiteralPath $payloadManifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($payloadManifest.schema_version -ne 1 -or $null -eq $payloadManifest.files) {
        throw "Invalid plugin payload manifest: $payloadManifestPath"
    }
    $payloadFiles = @($payloadManifest.files)
    if ($payloadFiles.Count -eq 0) {
        throw 'The plugin payload manifest must not be empty.'
    }

    Complete-PluginCacheManifests -Version $Version
    Copy-VerifiedFile `
        -SourceFile $payloadManifestPath `
        -TargetFile (Join-Path $pluginCacheDirectory 'release-files.json')
    foreach ($relativePath in $payloadFiles) {
        if ([string]::IsNullOrWhiteSpace($relativePath) -or
            [System.IO.Path]::IsPathRooted($relativePath) -or
            @($relativePath -split '[\/]').Contains('..')) {
            throw "Plugin payload path escapes the plugin root: $relativePath"
        }
        Copy-VerifiedFile `
            -SourceFile (Join-Path $pluginSourceDirectory $relativePath) `
            -TargetFile (Join-Path $pluginCacheDirectory $relativePath)
    }
}

function Stop-TokenPokerCacheProcesses {
    $codexHomeRoot = Get-CodexHomeRoot
    $cacheFamilyDirectory = [System.IO.Path]::GetFullPath(
        (Join-Path $codexHomeRoot "plugins\cache\$marketplaceName\token-holdem")
    )
    $processSnapshot = @(Get-CimInstance Win32_Process)
    $directOwners = @(
        $processSnapshot | Where-Object {
                $executablePath = if ([string]::IsNullOrWhiteSpace($_.ExecutablePath)) {
                    ''
                }
                else {
                    [System.IO.Path]::GetFullPath($_.ExecutablePath)
                }
                $commandLine = if ($null -eq $_.CommandLine) { '' } else { [string]$_.CommandLine }
                $_.ProcessId -ne $PID -and (
                    $executablePath.StartsWith(
                        $cacheFamilyDirectory,
                        [System.StringComparison]::OrdinalIgnoreCase
                    ) -or
                    $commandLine.IndexOf(
                        $cacheFamilyDirectory,
                        [System.StringComparison]::OrdinalIgnoreCase
                    ) -ge 0
                )
            }
    )
    $processIndex = @{}
    foreach ($process in $processSnapshot) {
        $processIndex[[int]$process.ProcessId] = $process
    }
    $processesToStopIndex = @{}
    foreach ($process in $directOwners) {
        $processesToStopIndex[[int]$process.ProcessId] = $process
        $parentProcessId = [int]$process.ParentProcessId
        while ($processIndex.ContainsKey($parentProcessId)) {
            $parentProcess = $processIndex[$parentProcessId]
            if ($parentProcess.Name -notin @('node.exe', 'cmd.exe')) {
                break
            }
            $processesToStopIndex[[int]$parentProcess.ProcessId] = $parentProcess
            $parentProcessId = [int]$parentProcess.ParentProcessId
        }
    }
    $processesToStop = @($processesToStopIndex.Values)
    foreach ($process in ($processesToStop | Sort-Object @{ Expression = { if ($_.Name -eq 'node.exe') { 0 } else { 1 } } }, ParentProcessId)) {
        Stop-Process -Id $process.ProcessId -Force -ErrorAction SilentlyContinue
    }
    if ($processesToStop.Count -gt 0) {
        Start-Sleep -Milliseconds 300
    }
}

$pluginCacheDirectory = Get-PluginCacheDirectory -Version $pluginVersion
$repairInPlace = $Upgrade -and (Test-Path -LiteralPath $pluginCacheDirectory -PathType Container)

if ($Upgrade) {
    # Windows locks running sidecar and MCP payloads. Both cross-version upgrades
    # and same-version repairs must stop only the Token Poker process tree first.
    Stop-TokenPokerCacheProcesses
}

if ($Upgrade -and -not $repairInPlace) {
    if ((Invoke-CodexCommand -Arguments @('plugin', 'remove', $pluginSelector)) -ne 0) {
        Write-Warning 'The current Codex task still locks the old plugin cache. Registration will continue, and Codex will clean the old cache after exit.'
    }
    if ((Invoke-CodexCommand -Arguments @('plugin', 'marketplace', 'remove', $marketplaceName)) -ne 0) {
        throw 'Could not update the Token Poker marketplace registration. Verify the old version is installed and retry.'
    }
}

if (-not $repairInPlace) {
    if ((Invoke-CodexCommand -Arguments @('plugin', 'marketplace', 'add', $marketplaceRoot)) -ne 0) {
        throw 'Could not add the Token Poker marketplace. Use -Upgrade when replacing an installed version.'
    }

    if ((Invoke-CodexCommand -Arguments @('plugin', 'add', $pluginSelector)) -ne 0) {
        throw 'Could not install Token Poker. Use -Upgrade when an older version is already installed.'
    }
}

Sync-PluginCachePayload -Version $pluginVersion
$runtimeBytes = Prepare-OfficialUsageRuntime -Version $pluginVersion
$runtimeSizeMiB = [Math]::Round($runtimeBytes / 1MB, 1)
Write-Output "Token Poker is installed. The official usage runtime copy uses $runtimeSizeMiB MiB. Open it from a new Codex task."
