[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$ArchivePath,
    [Parameter(Mandatory)][ValidatePattern('^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$')][string]$ExpectedVersion,
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-f]{64}$')][string]$ExpectedSha256,
    [Parameter(Mandatory)][ValidateRange(1, 536870912)][long]$ExpectedBytes,
    [Parameter(Mandatory)][ValidateRange(1, 2147483647)][int]$ParentProcessId,
    [ValidateRange(0, 30)][int]$DelaySeconds = 2,
    [switch]$ValidateOnly
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$maximumExpandedBytes = 1GB
$stageDirectory = [System.IO.Path]::GetFullPath((Split-Path -Parent $ArchivePath))
$resolvedArchive = [System.IO.Path]::GetFullPath($ArchivePath)
$expectedArchiveName = "token-poker-plugin-v$ExpectedVersion-windows-x64.zip"
$expectedArchiveRoot = "token-poker-plugin-v$ExpectedVersion-windows-x64/"
$partialDirectory = [System.IO.Path]::GetFullPath(
    (Join-Path $stageDirectory "package.partial-$PID")
)
$packageDirectory = [System.IO.Path]::GetFullPath((Join-Path $stageDirectory 'package'))
$logPath = Join-Path $stageDirectory 'install.log'
$resultPath = Join-Path $stageDirectory 'update-result.json'

function Assert-ChildPath {
    param(
        [Parameter(Mandatory)][string]$Parent,
        [Parameter(Mandatory)][string]$Child
    )

    $parentPrefix = [System.IO.Path]::GetFullPath($Parent).TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    ) + [System.IO.Path]::DirectorySeparatorChar
    $resolvedChild = [System.IO.Path]::GetFullPath($Child)
    if (-not $resolvedChild.StartsWith($parentPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Update path escapes its staging directory: $resolvedChild"
    }
}

function Assert-SafeRelativePath {
    param([Parameter(Mandatory)][string]$RelativePath)

    if ([string]::IsNullOrWhiteSpace($RelativePath) -or
        [System.IO.Path]::IsPathRooted($RelativePath) -or
        $RelativePath.Contains('\') -or
        @($RelativePath -split '/').Contains('..')) {
        throw "Unsafe update package path: $RelativePath"
    }
}

function Write-UpdateResult {
    param(
        [Parameter(Mandatory)][ValidateSet('validated', 'succeeded', 'failed')][string]$Status,
        [Parameter(Mandatory)][string]$Message
    )

    $temporaryResult = "$resultPath.$PID.partial"
    @{
        schema_version = 1
        version = $ExpectedVersion
        status = $Status
        message = $Message
        completed_at_unix_ms = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    } |
        ConvertTo-Json |
        Set-Content -LiteralPath $temporaryResult -Encoding UTF8
    Move-Item -LiteralPath $temporaryResult -Destination $resultPath -Force
}

function Expand-VerifiedPackage {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [System.IO.Compression.ZipFile]::OpenRead($resolvedArchive)
    try {
        $expandedBytes = 0L
        foreach ($entry in $archive.Entries) {
            if (-not $entry.FullName.StartsWith(
                    $expectedArchiveRoot,
                    [System.StringComparison]::Ordinal
                )) {
                throw "Unexpected ZIP root: $($entry.FullName)"
            }
            $relativePath = $entry.FullName.Substring($expectedArchiveRoot.Length)
            if ([string]::IsNullOrEmpty($relativePath)) {
                continue
            }
            Assert-SafeRelativePath -RelativePath $relativePath
            $expandedBytes += [long]$entry.Length
            if ($expandedBytes -gt $maximumExpandedBytes) {
                throw 'The expanded update package exceeds the safety limit.'
            }

            $destinationPath = [System.IO.Path]::GetFullPath(
                (Join-Path $partialDirectory ($relativePath.Replace('/', '\')))
            )
            Assert-ChildPath -Parent $partialDirectory -Child $destinationPath
            if ($entry.FullName.EndsWith('/', [System.StringComparison]::Ordinal)) {
                New-Item -ItemType Directory -Path $destinationPath -Force | Out-Null
                continue
            }

            $destinationParent = Split-Path -Parent $destinationPath
            New-Item -ItemType Directory -Path $destinationParent -Force | Out-Null
            $sourceStream = $entry.Open()
            $destinationStream = [System.IO.File]::Create($destinationPath)
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
}

function Assert-PackageManifest {
    $manifestPath = Join-Path $partialDirectory 'manifest.json'
    if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
        throw 'The update package does not contain manifest.json.'
    }
    $manifest = Get-Content -LiteralPath $manifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($manifest.schema_version -ne 1 -or
        $manifest.name -ne 'token-poker-plugin' -or
        $manifest.version -ne $ExpectedVersion -or
        $manifest.target -ne 'windows-x64' -or
        $manifest.unsigned -ne $true) {
        throw 'The update package manifest does not match the requested release.'
    }

    $declaredFiles = @($manifest.files)
    if ($declaredFiles.Count -eq 0) {
        throw 'The update package manifest has no files.'
    }
    $declaredPaths = [System.Collections.Generic.HashSet[string]]::new(
        [System.StringComparer]::Ordinal
    )
    foreach ($entry in $declaredFiles) {
        $relativePath = [string]$entry.path
        Assert-SafeRelativePath -RelativePath $relativePath
        if (-not $declaredPaths.Add($relativePath)) {
            throw "Duplicate path in update package manifest: $relativePath"
        }
        $filePath = [System.IO.Path]::GetFullPath(
            (Join-Path $partialDirectory ($relativePath.Replace('/', '\')))
        )
        Assert-ChildPath -Parent $partialDirectory -Child $filePath
        if (-not (Test-Path -LiteralPath $filePath -PathType Leaf)) {
            throw "Update package file is missing: $relativePath"
        }
        $file = Get-Item -LiteralPath $filePath
        if ([long]$entry.bytes -ne $file.Length) {
            throw "Update package file has an unexpected size: $relativePath"
        }
        $actualDigest = (Get-FileHash -LiteralPath $filePath -Algorithm SHA256).Hash.ToLowerInvariant()
        if ([string]$entry.sha256 -cne $actualDigest) {
            throw "Update package file failed SHA-256 verification: $relativePath"
        }
    }

    $partialPrefix = $partialDirectory.TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    ) + [System.IO.Path]::DirectorySeparatorChar
    $actualFiles = @(
        Get-ChildItem -LiteralPath $partialDirectory -Recurse -File |
            ForEach-Object {
                $resolvedFile = [System.IO.Path]::GetFullPath($_.FullName)
                if (-not $resolvedFile.StartsWith(
                        $partialPrefix,
                        [System.StringComparison]::OrdinalIgnoreCase
                    )) {
                    throw "Extracted file escaped the package directory: $resolvedFile"
                }
                $resolvedFile.Substring($partialPrefix.Length).Replace('\', '/')
            } |
            Where-Object { $_ -ne 'manifest.json' }
    )
    if ($actualFiles.Count -ne $declaredPaths.Count) {
        throw 'The update package contains files not covered by its manifest.'
    }
    foreach ($relativePath in $actualFiles) {
        if (-not $declaredPaths.Contains($relativePath)) {
            throw "Undeclared file in update package: $relativePath"
        }
    }
}

try {
    Assert-ChildPath -Parent $stageDirectory -Child $resolvedArchive
    Assert-ChildPath -Parent $stageDirectory -Child $partialDirectory
    Assert-ChildPath -Parent $stageDirectory -Child $packageDirectory
    if ((Split-Path -Leaf $resolvedArchive) -cne $expectedArchiveName) {
        throw 'The staged archive name does not match the requested release.'
    }
    $archiveFile = Get-Item -LiteralPath $resolvedArchive
    if ($archiveFile.Length -ne $ExpectedBytes) {
        throw 'The staged archive size changed after download.'
    }
    $archiveDigest = (Get-FileHash -LiteralPath $resolvedArchive -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($archiveDigest -cne $ExpectedSha256) {
        throw 'The staged archive failed SHA-256 verification.'
    }

    if (Test-Path -LiteralPath $partialDirectory) {
        Remove-Item -LiteralPath $partialDirectory -Recurse -Force
    }
    New-Item -ItemType Directory -Path $partialDirectory -Force | Out-Null
    Expand-VerifiedPackage
    Assert-PackageManifest
    if (Test-Path -LiteralPath $packageDirectory) {
        Remove-Item -LiteralPath $packageDirectory -Recurse -Force
    }
    Move-Item -LiteralPath $partialDirectory -Destination $packageDirectory

    $installerPath = Join-Path $packageDirectory 'install-token-poker.ps1'
    if (-not (Test-Path -LiteralPath $installerPath -PathType Leaf)) {
        throw 'The verified package does not contain the Token Poker installer.'
    }
    if ($ValidateOnly) {
        Write-UpdateResult -Status 'validated' -Message 'The update package passed validation.'
        Write-Output 'The update package passed isolated updater validation.'
        return
    }
    if ($DelaySeconds -gt 0) {
        Start-Sleep -Seconds $DelaySeconds
    }
    "Starting Token Poker $ExpectedVersion update from parent process $ParentProcessId." |
        Set-Content -LiteralPath $logPath -Encoding UTF8
    & $installerPath -Upgrade *>&1 | Tee-Object -FilePath $logPath -Append
    Write-UpdateResult -Status 'succeeded' -Message 'Token Poker was updated. Restart Codex.'
}
catch {
    $failureMessage = $_.Exception.Message
    $failureMessage | Add-Content -LiteralPath $logPath -Encoding UTF8
    Write-UpdateResult -Status 'failed' -Message $failureMessage
    exit 1
}
finally {
    if (Test-Path -LiteralPath $partialDirectory) {
        Assert-ChildPath -Parent $stageDirectory -Child $partialDirectory
        Remove-Item -LiteralPath $partialDirectory -Recurse -Force
    }
}
