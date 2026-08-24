[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$updateScript = Join-Path $projectRoot 'scripts\apply-update.ps1'
$workspaceManifest = Get-Content -LiteralPath (Join-Path $projectRoot 'Cargo.toml') -Raw
$versionMatch = [regex]::Match(
    $workspaceManifest,
    '(?ms)^\[workspace\.package\]\s*.*?^version\s*=\s*"(?<version>[^\"]+)"'
)
if (-not $versionMatch.Success) {
    throw 'Could not read the workspace version.'
}
$currentVersion = [Version]$versionMatch.Groups['version'].Value
$nextVersion = '{0}.{1}.{2}' -f $currentVersion.Major, $currentVersion.Minor, ($currentVersion.Build + 1)

$temporaryBase = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$testRoot = [System.IO.Path]::GetFullPath(
    (Join-Path $temporaryBase "token-poker-forward-update-$([Guid]::NewGuid().ToString('N'))")
)
$temporaryPrefix = $temporaryBase.TrimEnd(
    [System.IO.Path]::DirectorySeparatorChar,
    [System.IO.Path]::AltDirectorySeparatorChar
) + [System.IO.Path]::DirectorySeparatorChar
if (-not $testRoot.StartsWith($temporaryPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Forward-update test path escaped the temporary directory: $testRoot"
}

$packageName = "token-poker-plugin-v$nextVersion-windows-x64"
$sourceParent = Join-Path $testRoot 'source'
$packageRoot = Join-Path $sourceParent $packageName
$archivePath = Join-Path $testRoot "$packageName.zip"
$markerPath = Join-Path $testRoot 'installer-marker.json'
$resultPath = Join-Path $testRoot 'update-result.json'
$originalMarker = $env:TOKEN_POKER_FORWARD_UPDATE_MARKER
$originalExpectedVersion = $env:TOKEN_POKER_FORWARD_UPDATE_EXPECTED_VERSION
$utf8NoBom = [System.Text.UTF8Encoding]::new($false)

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
    New-Item -ItemType Directory -Path $packageRoot -Force | Out-Null
    $installerPath = Join-Path $packageRoot 'install-token-poker.ps1'
    $installerSource = @'
[CmdletBinding()]
param(
    [switch]$Upgrade,
    [AllowNull()][string]$ExpectedVersion
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if (-not $Upgrade) {
    throw 'The update helper did not request an upgrade.'
}
if ($ExpectedVersion -cne $env:TOKEN_POKER_FORWARD_UPDATE_EXPECTED_VERSION) {
    throw "The update helper forwarded an unexpected version: $ExpectedVersion"
}
Start-Sleep -Milliseconds 750
@{
    upgrade = $Upgrade.IsPresent
    expected_version = $ExpectedVersion
} |
    ConvertTo-Json -Compress |
    Set-Content -LiteralPath $env:TOKEN_POKER_FORWARD_UPDATE_MARKER -Encoding UTF8
Write-Output "Synthetic Token Poker $ExpectedVersion installer completed."
'@
    [System.IO.File]::WriteAllText($installerPath, $installerSource, $utf8NoBom)

    $installerFile = Get-Item -LiteralPath $installerPath
    $installerDigest = (Get-FileHash -LiteralPath $installerPath -Algorithm SHA256).Hash.ToLowerInvariant()
    $packageManifest = [ordered]@{
        schema_version = 1
        name = 'token-poker-plugin'
        version = $nextVersion
        target = 'windows-x64'
        unsigned = $true
        files = @(
            [ordered]@{
                path = 'install-token-poker.ps1'
                bytes = $installerFile.Length
                sha256 = $installerDigest
            }
        )
    }
    [System.IO.File]::WriteAllText(
        (Join-Path $packageRoot 'manifest.json'),
        (($packageManifest | ConvertTo-Json -Depth 5) + "`n"),
        $utf8NoBom
    )

    Add-Type -AssemblyName System.IO.Compression
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [System.IO.Compression.ZipFile]::Open(
        $archivePath,
        [System.IO.Compression.ZipArchiveMode]::Create
    )
    try {
        $packagePrefix = $packageRoot.TrimEnd(
            [System.IO.Path]::DirectorySeparatorChar,
            [System.IO.Path]::AltDirectorySeparatorChar
        ) + [System.IO.Path]::DirectorySeparatorChar
        foreach ($file in Get-ChildItem -LiteralPath $packageRoot -Recurse -File) {
            $relativePath = $file.FullName.Substring($packagePrefix.Length).Replace('\', '/')
            $entry = $archive.CreateEntry(
                "$packageName/$relativePath",
                [System.IO.Compression.CompressionLevel]::Optimal
            )
            $sourceStream = $file.OpenRead()
            $destinationStream = $entry.Open()
            try {
                $sourceStream.CopyTo($destinationStream)
            }
            finally {
                $destinationStream.Dispose()
                $sourceStream.Dispose()
            }
        }
    }
    finally {
        $archive.Dispose()
    }
    $archiveFile = Get-Item -LiteralPath $archivePath
    $archiveDigest = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
    $env:TOKEN_POKER_FORWARD_UPDATE_MARKER = $markerPath
    $env:TOKEN_POKER_FORWARD_UPDATE_EXPECTED_VERSION = $nextVersion

    & $updateScript `
        -ArchivePath $archivePath `
        -ExpectedVersion $nextVersion `
        -ExpectedSha256 $archiveDigest `
        -ExpectedBytes $archiveFile.Length `
        -ParentProcessId $PID `
        -DelaySeconds 0

    if (-not (Test-Path -LiteralPath $markerPath -PathType Leaf)) {
        $resultDetail = if (Test-Path -LiteralPath $resultPath -PathType Leaf) {
            Get-Content -LiteralPath $resultPath -Raw -Encoding UTF8
        }
        else {
            'missing update-result.json'
        }
        $logPath = Join-Path $testRoot 'install.log'
        $logDetail = if (Test-Path -LiteralPath $logPath -PathType Leaf) {
            Get-Content -LiteralPath $logPath -Raw -Encoding UTF8
        }
        else {
            'missing install.log'
        }
        throw "The next-version installer did not run. Result: $resultDetail Log: $logDetail"
    }
    Assert-True -Condition (Test-Path -LiteralPath $resultPath -PathType Leaf) -Message 'The update helper did not write its result.'
    $marker = Get-Content -LiteralPath $markerPath -Raw -Encoding UTF8 | ConvertFrom-Json
    $result = Get-Content -LiteralPath $resultPath -Raw -Encoding UTF8 | ConvertFrom-Json
    Assert-True -Condition ($marker.upgrade -eq $true) -Message 'The next-version installer did not receive -Upgrade.'
    Assert-True -Condition ($marker.expected_version -ceq $nextVersion) -Message 'The next-version installer did not receive the expected version.'
    Assert-True -Condition ($result.schema_version -eq 1) -Message 'The updater result schema changed unexpectedly.'
    Assert-True -Condition ($result.version -ceq $nextVersion) -Message 'The updater result reported the wrong version.'
    Assert-True -Condition ($result.status -ceq 'succeeded') -Message 'The updater did not report a verified success.'

    Write-Output "Token Poker forward-update contract passed: $($currentVersion.ToString()) -> $nextVersion."
}
finally {
    $env:TOKEN_POKER_FORWARD_UPDATE_MARKER = $originalMarker
    $env:TOKEN_POKER_FORWARD_UPDATE_EXPECTED_VERSION = $originalExpectedVersion
    if (Test-Path -LiteralPath $testRoot -PathType Container) {
        if (-not $testRoot.StartsWith($temporaryPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
            throw "Refusing to clean an unsafe forward-update test path: $testRoot"
        }
        Remove-Item -LiteralPath $testRoot -Recurse -Force
    }
}
