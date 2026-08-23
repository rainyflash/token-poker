[CmdletBinding()]
param(
    [switch]$Upgrade,
    [switch]$ValidateOnly,
    [string]$LogPath,
    [ValidatePattern('^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$')]
    [AllowNull()][string]$ExpectedVersion
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$transcriptStarted = $false
$bootstrapDirectory = $null
$bootstrapBaseDirectory = $null
try {
if (-not [string]::IsNullOrWhiteSpace($LogPath)) {
    $resolvedLogPath = [System.IO.Path]::GetFullPath($LogPath)
    $logDirectory = Split-Path -Parent $resolvedLogPath
    New-Item -ItemType Directory -Path $logDirectory -Force | Out-Null
    Start-Transcript -LiteralPath $resolvedLogPath -Append -Force | Out-Null
    $transcriptStarted = $true
}

$runtimeResolverPath = Join-Path $PSScriptRoot 'codex-runtime.ps1'
if (-not (Test-Path -LiteralPath $runtimeResolverPath -PathType Leaf)) {
    throw "The installer package is missing its Codex runtime resolver: $runtimeResolverPath"
}
. $runtimeResolverPath

$packageMarketplaceRoot = if (Test-Path -LiteralPath (Join-Path $PSScriptRoot '.agents\plugins\marketplace.json')) {
    (Resolve-Path $PSScriptRoot).Path
}
else {
    (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
}
$marketplaceName = 'token-holdem-community'
$pluginSelector = "token-holdem@$marketplaceName"
$packagePluginSourceDirectory = Join-Path $packageMarketplaceRoot 'plugins\token-holdem'
$pluginSourceDirectory = $packagePluginSourceDirectory
$pluginManifestPath = Join-Path $pluginSourceDirectory '.codex-plugin\plugin.json'
$pluginVersion = (
    Get-Content -LiteralPath $pluginManifestPath -Raw -Encoding UTF8 |
        ConvertFrom-Json
).version
if (-not [string]::IsNullOrWhiteSpace($ExpectedVersion) -and $pluginVersion -cne $ExpectedVersion) {
    throw "Installer version $pluginVersion does not match the requested version $ExpectedVersion."
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
    param(
        [Parameter(Mandatory)][string]$Version,
        [Parameter(Mandatory)][string]$SourceFile
    )

    $pluginCacheDirectory = Get-PluginCacheDirectory -Version $Version
    if (-not (Test-Path -LiteralPath $pluginCacheDirectory -PathType Container)) {
        throw "Codex did not create the Token Poker plugin cache: $pluginCacheDirectory"
    }

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
    param(
        [Parameter(Mandatory)][string]$ExecutablePath,
        [Parameter(Mandatory)][string[]]$Arguments
    )

    $originalErrorPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        & $ExecutablePath @Arguments *> $null
        return [int]$LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $originalErrorPreference
    }
}

function Get-MarketplaceRegistrationRoot {
    param([Parameter(Mandatory)][string]$ExecutablePath)

    $output = @()
    $exitCode = 1
    $originalErrorPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $output = @(& $ExecutablePath plugin marketplace list 2>&1)
        $exitCode = [int]$LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $originalErrorPreference
    }
    if ($exitCode -ne 0) {
        throw 'Could not inspect configured Codex plugin marketplaces.'
    }

    foreach ($line in $output) {
        $columns = ([string]$line).Trim() -split '\s{2,}', 2
        if ($columns.Count -eq 2 -and $columns[0] -ceq $marketplaceName) {
            return [string]$columns[1]
        }
    }
    return $null
}

function Get-InstalledPluginRegistration {
    param([Parameter(Mandatory)][string]$ExecutablePath)

    $output = @()
    $exitCode = 1
    $originalErrorPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $output = @(& $ExecutablePath plugin list 2>&1)
        $exitCode = [int]$LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $originalErrorPreference
    }
    if ($exitCode -ne 0) {
        throw 'Could not inspect installed Codex plugins.'
    }

    foreach ($line in $output) {
        $columns = ([string]$line).Trim() -split '\s{2,}', 4
        if ($columns.Count -eq 4 -and $columns[0] -ceq $pluginSelector) {
            return [pscustomobject]@{
                Status = [string]$columns[1]
                Version = [string]$columns[2]
                Path = [string]$columns[3]
            }
        }
    }
    return $null
}

function Get-ComparablePath {
    param([Parameter(Mandatory)][string]$Path)

    $resolvedPath = [System.IO.Path]::GetFullPath($Path).TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    )
    if ($resolvedPath.StartsWith('\\?\', [System.StringComparison]::Ordinal)) {
        return $resolvedPath.Substring(4)
    }
    return $resolvedPath
}

function Test-CodexCommand {
    param([Parameter(Mandatory)][string]$ExecutablePath)

    $originalErrorPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        & $ExecutablePath --version *> $null
        return $LASTEXITCODE -eq 0
    }
    catch {
        return $false
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

function Get-ReleasePayloadFiles {
    param([Parameter(Mandatory)][string]$SourceDirectory)

    $payloadManifestPath = Join-Path $SourceDirectory 'release-files.json'
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
        $sourceFile = Join-Path $SourceDirectory $relativePath
        if (-not (Test-Path -LiteralPath $sourceFile -PathType Leaf)) {
            throw "Plugin source file does not exist: $sourceFile"
        }
    }

    foreach ($relativePath in @('.mcp.json', '.codex-plugin/plugin.json')) {
        $manifestPath = Join-Path $SourceDirectory $relativePath
        if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
            throw "Plugin manifest does not exist: $manifestPath"
        }
        Get-Content -LiteralPath $manifestPath -Raw -Encoding UTF8 | ConvertFrom-Json | Out-Null
    }
    return [string[]]$payloadFiles
}

function Get-MarketplacePayloadFingerprint {
    param(
        [Parameter(Mandatory)][string]$SourceRoot,
        [Parameter(Mandatory)][string]$SourceDirectory,
        [Parameter(Mandatory)][string[]]$PayloadFiles
    )

    $relativePaths = @('.mcp.json', '.codex-plugin/plugin.json', 'release-files.json') + $PayloadFiles
    $marketplaceDigest = (
        Get-FileHash -LiteralPath (Join-Path $SourceRoot '.agents\plugins\marketplace.json') -Algorithm SHA256
    ).Hash
    $records = @(
        ".agents/plugins/marketplace.json`t$($marketplaceDigest.ToLowerInvariant())"
    )
    $records += foreach ($relativePath in ($relativePaths | Sort-Object -CaseSensitive)) {
        $digest = (Get-FileHash -LiteralPath (Join-Path $SourceDirectory $relativePath) -Algorithm SHA256).Hash
        "plugins/token-holdem/$relativePath`t$($digest.ToLowerInvariant())"
    }
    $bytes = [System.Text.Encoding]::UTF8.GetBytes(($records -join "`n"))
    $hasher = [System.Security.Cryptography.SHA256]::Create()
    try {
        $digestBytes = $hasher.ComputeHash($bytes)
    }
    finally {
        $hasher.Dispose()
    }
    return ([System.BitConverter]::ToString($digestBytes).Replace('-', '').ToLowerInvariant()).Substring(0, 16)
}

function Assert-MarketplacePayloadMatches {
    param(
        [Parameter(Mandatory)][string]$SourceRoot,
        [Parameter(Mandatory)][string]$TargetRoot,
        [Parameter(Mandatory)][string[]]$PayloadFiles
    )

    $relativePaths = @(
        '.agents/plugins/marketplace.json',
        'plugins/token-holdem/.mcp.json',
        'plugins/token-holdem/.codex-plugin/plugin.json',
        'plugins/token-holdem/release-files.json'
    ) + @($PayloadFiles | ForEach-Object { "plugins/token-holdem/$_" })
    foreach ($relativePath in $relativePaths) {
        $windowsPath = $relativePath.Replace('/', '\')
        $sourceFile = Join-Path $SourceRoot $windowsPath
        $targetFile = Join-Path $TargetRoot $windowsPath
        if (-not (Test-Path -LiteralPath $targetFile -PathType Leaf)) {
            throw "Persistent marketplace file is missing: $relativePath"
        }
        $sourceDigest = (Get-FileHash -LiteralPath $sourceFile -Algorithm SHA256).Hash
        $targetDigest = (Get-FileHash -LiteralPath $targetFile -Algorithm SHA256).Hash
        if ($sourceDigest -cne $targetDigest) {
            throw "Persistent marketplace file failed SHA-256 verification: $relativePath"
        }
    }
}

function Publish-PersistentMarketplace {
    param(
        [Parameter(Mandatory)][string]$SourceRoot,
        [Parameter(Mandatory)][string]$SourceDirectory,
        [Parameter(Mandatory)][string]$Version,
        [Parameter(Mandatory)][string[]]$PayloadFiles,
        [Parameter(Mandatory)][string]$MarketplaceBase
    )

    $fingerprint = Get-MarketplacePayloadFingerprint `
        -SourceRoot $SourceRoot `
        -SourceDirectory $SourceDirectory `
        -PayloadFiles $PayloadFiles
    $marketplaceBase = [System.IO.Path]::GetFullPath($MarketplaceBase)
    $targetRoot = [System.IO.Path]::GetFullPath(
        (Join-Path $marketplaceBase "v$Version-$fingerprint")
    )
    Assert-ChildDirectory -ParentDirectory $marketplaceBase -ChildDirectory $targetRoot
    New-Item -ItemType Directory -Path $marketplaceBase -Force | Out-Null

    if ((Get-ComparablePath -Path $SourceRoot) -ceq (Get-ComparablePath -Path $targetRoot)) {
        return $targetRoot
    }
    if (Test-Path -LiteralPath $targetRoot -PathType Container) {
        Assert-MarketplacePayloadMatches `
            -SourceRoot $SourceRoot `
            -TargetRoot $targetRoot `
            -PayloadFiles $PayloadFiles
        return $targetRoot
    }

    $partialRoot = [System.IO.Path]::GetFullPath(
        (Join-Path $marketplaceBase ".v$Version-$fingerprint.partial-$PID-$([Guid]::NewGuid().ToString('N'))")
    )
    Assert-ChildDirectory -ParentDirectory $marketplaceBase -ChildDirectory $partialRoot
    try {
        Copy-VerifiedFile `
            -SourceFile (Join-Path $SourceRoot '.agents\plugins\marketplace.json') `
            -TargetFile (Join-Path $partialRoot '.agents\plugins\marketplace.json')
        foreach ($relativePath in @('.mcp.json', '.codex-plugin/plugin.json', 'release-files.json') + $PayloadFiles) {
            Copy-VerifiedFile `
                -SourceFile (Join-Path $SourceDirectory $relativePath) `
                -TargetFile (Join-Path $partialRoot "plugins\token-holdem\$relativePath")
        }
        Assert-MarketplacePayloadMatches `
            -SourceRoot $SourceRoot `
            -TargetRoot $partialRoot `
            -PayloadFiles $PayloadFiles
        try {
            Move-Item -LiteralPath $partialRoot -Destination $targetRoot
        }
        catch {
            if (-not (Test-Path -LiteralPath $targetRoot -PathType Container)) {
                throw
            }
            Assert-MarketplacePayloadMatches `
                -SourceRoot $SourceRoot `
                -TargetRoot $targetRoot `
                -PayloadFiles $PayloadFiles
        }
    }
    finally {
        if (Test-Path -LiteralPath $partialRoot -PathType Container) {
            Assert-ChildDirectory -ParentDirectory $marketplaceBase -ChildDirectory $partialRoot
            Remove-Item -LiteralPath $partialRoot -Recurse -Force
        }
    }
    return $targetRoot
}

function Assert-InstalledPluginState {
    param(
        [Parameter(Mandatory)][string]$ExecutablePath,
        [Parameter(Mandatory)][string]$ExpectedMarketplaceRoot,
        [Parameter(Mandatory)][string]$ExpectedPluginDirectory,
        [Parameter(Mandatory)][string]$Version
    )

    $registeredRoot = Get-MarketplaceRegistrationRoot -ExecutablePath $ExecutablePath
    if ($null -eq $registeredRoot -or
        (Get-ComparablePath -Path $registeredRoot) -cne (Get-ComparablePath -Path $ExpectedMarketplaceRoot)) {
        throw 'Codex marketplace registration did not switch to the verified Token Poker package.'
    }
    $registration = Get-InstalledPluginRegistration -ExecutablePath $ExecutablePath
    if ($null -eq $registration -or
        $registration.Version -cne $Version -or
        (Get-ComparablePath -Path $registration.Path) -cne (Get-ComparablePath -Path $ExpectedPluginDirectory)) {
        throw "Codex still reports an older Token Poker plugin instead of version $Version."
    }

    $cacheManifestPath = Join-Path (Get-PluginCacheDirectory -Version $Version) '.codex-plugin\plugin.json'
    $cacheVersion = (
        Get-Content -LiteralPath $cacheManifestPath -Raw -Encoding UTF8 |
            ConvertFrom-Json
    ).version
    if ($cacheVersion -cne $Version) {
        throw "Token Poker cache manifest does not report version $Version."
    }
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

$payloadFiles = Get-ReleasePayloadFiles -SourceDirectory $packagePluginSourceDirectory
if ($ValidateOnly) {
    $validationParent = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
    $validationBase = [System.IO.Path]::GetFullPath(
        (Join-Path $validationParent "token-poker-marketplace-validation-$([Guid]::NewGuid().ToString('N'))")
    )
    Assert-ChildDirectory -ParentDirectory $validationParent -ChildDirectory $validationBase
    try {
        $firstRoot = Publish-PersistentMarketplace `
            -SourceRoot $packageMarketplaceRoot `
            -SourceDirectory $packagePluginSourceDirectory `
            -Version $pluginVersion `
            -PayloadFiles $payloadFiles `
            -MarketplaceBase $validationBase
        $secondRoot = Publish-PersistentMarketplace `
            -SourceRoot $packageMarketplaceRoot `
            -SourceDirectory $packagePluginSourceDirectory `
            -Version $pluginVersion `
            -PayloadFiles $payloadFiles `
            -MarketplaceBase $validationBase
        if ((Get-ComparablePath -Path $firstRoot) -cne (Get-ComparablePath -Path $secondRoot)) {
            throw 'Persistent marketplace publication is not deterministic.'
        }
    }
    finally {
        if (Test-Path -LiteralPath $validationBase -PathType Container) {
            Assert-ChildDirectory -ParentDirectory $validationParent -ChildDirectory $validationBase
            Remove-Item -LiteralPath $validationBase -Recurse -Force
        }
    }
    Write-Output "Token Poker $pluginVersion installer payload is valid."
    return
}

$codexCommandPath = Resolve-CodexCliCommandPath -ExplicitPath $env:CODEX_CLI_PATH
$codexDesktopBinaryPath = Resolve-CodexDesktopBinaryPath `
    -ExplicitPath $env:CODEX_APP_SERVER_PATH `
    -AdditionalCandidates @($codexCommandPath)
if ($null -eq $codexDesktopBinaryPath) {
    throw 'Could not locate the official App Server bundled with Codex desktop. Install or update Codex desktop and retry.'
}

if ($null -eq $codexCommandPath -or -not (Test-CodexCommand -ExecutablePath $codexCommandPath)) {
    $bootstrapBaseDirectory = if (-not [string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
        Join-Path $env:LOCALAPPDATA 'TokenPoker\installer-runtime'
    }
    else {
        Join-Path ([System.IO.Path]::GetTempPath()) 'TokenPoker\installer-runtime'
    }
    $bootstrap = New-CodexBootstrapCopy `
        -SourcePath $codexDesktopBinaryPath `
        -BaseDirectory $bootstrapBaseDirectory
    $bootstrapDirectory = $bootstrap.DirectoryPath
    $codexCommandPath = $bootstrap.CommandPath
}
if (-not (Test-CodexCommand -ExecutablePath $codexCommandPath)) {
    throw 'The official Codex desktop executable could not start from the installer workspace.'
}

if ([string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
    throw 'LOCALAPPDATA is required to persist the Token Poker marketplace.'
}
$persistentMarketplaceBase = [System.IO.Path]::GetFullPath(
    (Join-Path $env:LOCALAPPDATA 'TokenHoldem\marketplace')
)
$marketplaceRoot = Publish-PersistentMarketplace `
    -SourceRoot $packageMarketplaceRoot `
    -SourceDirectory $packagePluginSourceDirectory `
    -Version $pluginVersion `
    -PayloadFiles $payloadFiles `
    -MarketplaceBase $persistentMarketplaceBase
$pluginSourceDirectory = Join-Path $marketplaceRoot 'plugins\token-holdem'

$registeredMarketplaceRoot = Get-MarketplaceRegistrationRoot -ExecutablePath $codexCommandPath
$marketplaceRegistered = $null -ne $registeredMarketplaceRoot
$installedPlugin = Get-InstalledPluginRegistration -ExecutablePath $codexCommandPath
$effectiveUpgrade = $Upgrade -or $marketplaceRegistered
$pluginCacheDirectory = Get-PluginCacheDirectory -Version $pluginVersion
$repairInPlace = $effectiveUpgrade -and
    $marketplaceRegistered -and
    $null -ne $installedPlugin -and
    $installedPlugin.Version -ceq $pluginVersion -and
    (Get-ComparablePath -Path $registeredMarketplaceRoot) -ceq (Get-ComparablePath -Path $marketplaceRoot) -and
    (Get-ComparablePath -Path $installedPlugin.Path) -ceq (Get-ComparablePath -Path $pluginSourceDirectory) -and
    (Test-Path -LiteralPath $pluginCacheDirectory -PathType Container)

if ($repairInPlace) {
    Stop-TokenPokerCacheProcesses
}

if ($marketplaceRegistered -and -not $repairInPlace) {
    if ($null -ne $installedPlugin -and
        (Invoke-CodexCommand -ExecutablePath $codexCommandPath -Arguments @('plugin', 'remove', $pluginSelector)) -ne 0) {
        Write-Warning 'The current Codex task still locks the old plugin cache. Registration will continue, and Codex will clean the old cache after exit.'
    }
    if ((Invoke-CodexCommand -ExecutablePath $codexCommandPath -Arguments @('plugin', 'marketplace', 'remove', $marketplaceName)) -ne 0) {
        throw 'Could not update the Token Poker marketplace registration. Verify the old version is installed and retry.'
    }
}

if (-not $repairInPlace) {
    if ((Invoke-CodexCommand -ExecutablePath $codexCommandPath -Arguments @('plugin', 'marketplace', 'add', $marketplaceRoot)) -ne 0) {
        throw 'Could not register the Token Poker marketplace. Close Codex and retry installation.'
    }

    if ((Invoke-CodexCommand -ExecutablePath $codexCommandPath -Arguments @('plugin', 'add', $pluginSelector)) -ne 0) {
        throw 'Could not install Token Poker. Close Codex and retry installation.'
    }
}

Sync-PluginCachePayload -Version $pluginVersion -PayloadFiles $payloadFiles
$runtimeBytes = Prepare-OfficialUsageRuntime `
    -Version $pluginVersion `
    -SourceFile $codexDesktopBinaryPath
Assert-InstalledPluginState `
    -ExecutablePath $codexCommandPath `
    -ExpectedMarketplaceRoot $marketplaceRoot `
    -ExpectedPluginDirectory $pluginSourceDirectory `
    -Version $pluginVersion
$runtimeSizeMiB = [Math]::Round($runtimeBytes / 1MB, 1)
Write-Output "Token Poker is installed. The official usage runtime copy uses $runtimeSizeMiB MiB. Open it from a new Codex task."
}
finally {
    if ($null -ne $bootstrapDirectory -and $null -ne $bootstrapBaseDirectory) {
        try {
            Remove-CodexBootstrapCopy `
                -DirectoryPath $bootstrapDirectory `
                -BaseDirectory $bootstrapBaseDirectory
        }
        catch {
            Write-Warning "Could not remove the temporary Codex installer runtime: $($_.Exception.Message)"
        }
    }
    if ($transcriptStarted) {
        Stop-Transcript | Out-Null
    }
}
