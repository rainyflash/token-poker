[CmdletBinding()]
param(
    [switch]$Upgrade,
    [switch]$ValidateOnly,
    [string]$LogPath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$transcriptStarted = $false
try {
if (-not [string]::IsNullOrWhiteSpace($LogPath)) {
    $resolvedLogPath = [System.IO.Path]::GetFullPath($LogPath)
    $logDirectory = Split-Path -Parent $resolvedLogPath
    New-Item -ItemType Directory -Path $logDirectory -Force | Out-Null
    Start-Transcript -LiteralPath $resolvedLogPath -Append -Force | Out-Null
    $transcriptStarted = $true
}

$marketplaceRoot = if (Test-Path -LiteralPath (Join-Path $PSScriptRoot '.agents\plugins\marketplace.json')) {
    (Resolve-Path $PSScriptRoot).Path
}
else {
    (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
}
$marketplaceName = 'token-holdem-community'
$pluginSelector = "token-holdem@$marketplaceName"
$pluginSourceDirectory = Join-Path $marketplaceRoot 'plugins\token-holdem'
$pluginManifestPath = Join-Path $pluginSourceDirectory '.codex-plugin\plugin.json'
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

function Test-MarketplaceRegistered {
    $output = @()
    $exitCode = 1
    $originalErrorPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $output = @(& $codexCommand.Source plugin marketplace list 2>&1)
        $exitCode = [int]$LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $originalErrorPreference
    }
    if ($exitCode -ne 0) {
        throw 'Could not inspect configured Codex plugin marketplaces.'
    }

    foreach ($line in $output) {
        $columns = ([string]$line).Trim() -split '\s+', 2
        if ($columns.Count -gt 0 -and $columns[0] -ceq $marketplaceName) {
            return $true
        }
    }
    return $false
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

function Get-ReleasePayloadFiles {
    $payloadManifestPath = Join-Path $pluginSourceDirectory 'release-files.json'
    if (-not (Test-Path -LiteralPath $payloadManifestPath -PathType Leaf)) {
        throw "Plugin payload manifest does not exist: $payloadManifestPath"
    }
    $payloadManifest = Get-Content -LiteralPath $payloadManifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($payloadManifest.schema_version -ne 1 -or $null -eq $payloadManifest.files) {
        throw "Invalid plugin payload manifest: $payloadManifestPath"
    }
    $payloadFiles = @($payloadManifest.files)
    if ($payloadFiles.Count -eq 0) {
        throw 'The plugin payload manifest must not be empty.'
    }

    $seenPaths = [System.Collections.Generic.HashSet[string]]::new(
        [System.StringComparer]::Ordinal
    )
    foreach ($relativePath in $payloadFiles) {
        if ([string]::IsNullOrWhiteSpace($relativePath) -or
            [System.IO.Path]::IsPathRooted($relativePath) -or
            $relativePath.Contains('\') -or
            @($relativePath -split '/').Contains('..')) {
            throw "Plugin payload path escapes the plugin root: $relativePath"
        }
        if (-not $seenPaths.Add([string]$relativePath)) {
            throw "Duplicate plugin payload path: $relativePath"
        }
        $sourceFile = Join-Path $pluginSourceDirectory $relativePath
        if (-not (Test-Path -LiteralPath $sourceFile -PathType Leaf)) {
            throw "Plugin source file does not exist: $sourceFile"
        }
    }

    foreach ($relativePath in @('.mcp.json', '.codex-plugin/plugin.json')) {
        $manifestPath = Join-Path $pluginSourceDirectory $relativePath
        if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
            throw "Plugin manifest does not exist: $manifestPath"
        }
        Get-Content -LiteralPath $manifestPath -Raw -Encoding UTF8 | ConvertFrom-Json | Out-Null
    }
    return [string[]]$payloadFiles
}

function Complete-PluginCacheManifests {
    param([Parameter(Mandatory)][string]$Version)

    $pluginCacheDirectory = Get-PluginCacheDirectory -Version $Version
    if (-not (Test-Path -LiteralPath $pluginCacheDirectory -PathType Container)) {
        throw "Codex did not create the Token Poker plugin cache: $pluginCacheDirectory"
    }

    Copy-VerifiedManifest `
        -SourceFile (Join-Path $pluginSourceDirectory '.mcp.json') `
        -TargetFile (Join-Path $pluginCacheDirectory '.mcp.json')
    Copy-VerifiedManifest `
        -SourceFile (Join-Path $pluginSourceDirectory '.codex-plugin\plugin.json') `
        -TargetFile (Join-Path $pluginCacheDirectory '.codex-plugin\plugin.json')
}

function Sync-PluginCachePayload {
    param(
        [Parameter(Mandatory)][string]$Version,
        [Parameter(Mandatory)][string[]]$PayloadFiles
    )

    $pluginCacheDirectory = Get-PluginCacheDirectory -Version $Version
    $payloadManifestPath = Join-Path $pluginSourceDirectory 'release-files.json'

    Complete-PluginCacheManifests -Version $Version
    Copy-VerifiedFile `
        -SourceFile $payloadManifestPath `
        -TargetFile (Join-Path $pluginCacheDirectory 'release-files.json')
    foreach ($relativePath in $PayloadFiles) {
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

$payloadFiles = Get-ReleasePayloadFiles
if ($ValidateOnly) {
    Write-Output "Token Poker $pluginVersion installer payload is valid."
    return
}

$marketplaceRegistered = Test-MarketplaceRegistered
$effectiveUpgrade = $Upgrade -or $marketplaceRegistered
$pluginCacheDirectory = Get-PluginCacheDirectory -Version $pluginVersion
$repairInPlace = $effectiveUpgrade -and
    $marketplaceRegistered -and
    (Test-Path -LiteralPath $pluginCacheDirectory -PathType Container)

if ($effectiveUpgrade) {
    # Windows locks running sidecar and MCP payloads. Both cross-version upgrades
    # and same-version repairs must stop only the Token Poker process tree first.
    Stop-TokenPokerCacheProcesses
}

if ($marketplaceRegistered -and -not $repairInPlace) {
    if ((Invoke-CodexCommand -Arguments @('plugin', 'remove', $pluginSelector)) -ne 0) {
        Write-Warning 'The current Codex task still locks the old plugin cache. Registration will continue, and Codex will clean the old cache after exit.'
    }
    if ((Invoke-CodexCommand -Arguments @('plugin', 'marketplace', 'remove', $marketplaceName)) -ne 0) {
        throw 'Could not update the Token Poker marketplace registration. Verify the old version is installed and retry.'
    }
}

if (-not $repairInPlace) {
    if ((Invoke-CodexCommand -Arguments @('plugin', 'marketplace', 'add', $marketplaceRoot)) -ne 0) {
        throw 'Could not register the Token Poker marketplace. Close Codex and retry installation.'
    }

    if ((Invoke-CodexCommand -Arguments @('plugin', 'add', $pluginSelector)) -ne 0) {
        throw 'Could not install Token Poker. Close Codex and retry installation.'
    }
}

Sync-PluginCachePayload -Version $pluginVersion -PayloadFiles $payloadFiles
$runtimeBytes = Prepare-OfficialUsageRuntime -Version $pluginVersion
$runtimeSizeMiB = [Math]::Round($runtimeBytes / 1MB, 1)
Write-Output "Token Poker is installed. The official usage runtime copy uses $runtimeSizeMiB MiB. Open it from a new Codex task."
}
finally {
    if ($transcriptStarted) {
        Stop-Transcript | Out-Null
    }
}
