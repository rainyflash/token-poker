Set-StrictMode -Version Latest

function Resolve-ExistingFilePath {
    param([AllowNull()][object]$Candidate)

    if ($null -eq $Candidate) {
        return $null
    }

    $candidatePath = if ($Candidate -is [string]) {
        [string]$Candidate
    }
    else {
        $value = $null
        foreach ($propertyName in @('Source', 'Path', 'Definition')) {
            $property = $Candidate.PSObject.Properties[$propertyName]
            if ($null -ne $property -and -not [string]::IsNullOrWhiteSpace([string]$property.Value)) {
                $value = [string]$property.Value
                break
            }
        }
        $value
    }

    if ([string]::IsNullOrWhiteSpace($candidatePath) -or
        -not (Test-Path -LiteralPath $candidatePath -PathType Leaf)) {
        return $null
    }
    return (Resolve-Path -LiteralPath $candidatePath).Path
}

function Resolve-CodexCliCommandPath {
    [CmdletBinding()]
    param(
        [AllowNull()][string]$ExplicitPath,
        [scriptblock]$CommandResolver = {
            param([string]$Name)
            Get-Command $Name -ErrorAction SilentlyContinue
        }
    )

    $resolvedExplicitPath = Resolve-ExistingFilePath -Candidate $ExplicitPath
    if ($null -ne $resolvedExplicitPath) {
        return $resolvedExplicitPath
    }

    # Prefer the shell-resolved command because npm and developer installs may
    # provide a runnable wrapper before the Windows App execution alias.
    foreach ($commandName in @('codex', 'codex.exe')) {
        $commands = try {
            @(& $CommandResolver $commandName)
        }
        catch {
            @()
        }
        foreach ($command in $commands) {
            $resolvedPath = Resolve-ExistingFilePath -Candidate $command
            if ($null -ne $resolvedPath) {
                return $resolvedPath
            }
        }
    }
    return $null
}

function Get-CodexPluginCommandStatus {
    [CmdletBinding()]
    param(
        [AllowNull()][string]$ExecutablePath,
        [scriptblock]$CommandRunner = {
            param([string]$CommandPath, [string[]]$Arguments)
            $originalPreference = $ErrorActionPreference
            try {
                $ErrorActionPreference = 'Continue'
                $commandOutput = @(& $CommandPath @Arguments 2>&1)
                $exitCode = if ($null -eq $LASTEXITCODE) { -1 } else { [int]$LASTEXITCODE }
                return [pscustomobject]@{
                    ExitCode = $exitCode
                    Output = ($commandOutput -join "`n")
                }
            }
            finally {
                $ErrorActionPreference = $originalPreference
            }
        }
    )

    if ([string]::IsNullOrWhiteSpace($ExecutablePath)) {
        return [pscustomobject]@{ Usable = $false; Detail = '未找到 Codex 命令。' }
    }

    foreach ($probeCommand in @('plugin marketplace list', 'plugin list')) {
        try {
            $probeResult = & $CommandRunner $ExecutablePath ($probeCommand -split ' ')
            if ($probeResult.ExitCode -ne 0) {
                return [pscustomobject]@{
                    Usable = $false
                    Detail = "$probeCommand 失败，退出码 $($probeResult.ExitCode)：$($probeResult.Output)"
                }
            }
        }
        catch {
            return [pscustomobject]@{
                Usable = $false
                Detail = "$probeCommand 无法执行：$($_.Exception.Message)"
            }
        }
    }
    return [pscustomobject]@{ Usable = $true; Detail = '' }
}

function Get-DefaultCodexDesktopPaths {
    $paths = [System.Collections.Generic.List[string]]::new()
    if (-not [string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
        foreach ($relativePath in @(
                'Programs\Codex\app\resources\codex.exe',
                'Programs\Codex\resources\codex.exe',
                'Programs\OpenAI Codex\app\resources\codex.exe',
                'Codex\app\resources\codex.exe'
            )) {
            $paths.Add((Join-Path $env:LOCALAPPDATA $relativePath))
        }
    }
    foreach ($programFilesRoot in @($env:ProgramFiles, ${env:ProgramFiles(x86)})) {
        if ([string]::IsNullOrWhiteSpace($programFilesRoot)) {
            continue
        }
        foreach ($relativePath in @(
                'Codex\app\resources\codex.exe',
                'Codex\resources\codex.exe',
                'OpenAI\Codex\app\resources\codex.exe'
            )) {
            $paths.Add((Join-Path $programFilesRoot $relativePath))
        }
    }
    return [string[]]$paths
}

function Resolve-CodexDesktopBinaryPath {
    [CmdletBinding()]
    param(
        [AllowNull()][string]$ExplicitPath,
        [scriptblock]$AppPackageResolver = {
            if ($null -eq (Get-Command Get-AppxPackage -ErrorAction SilentlyContinue)) {
                return @()
            }
            @(
                Get-AppxPackage -Name 'OpenAI.Codex' -ErrorAction SilentlyContinue |
                    Sort-Object Version -Descending
            )
        },
        [AllowNull()][string[]]$KnownPaths,
        [AllowNull()][string[]]$AdditionalCandidates
    )

    $candidates = [System.Collections.Generic.List[object]]::new()
    if (-not [string]::IsNullOrWhiteSpace($ExplicitPath)) {
        $candidates.Add($ExplicitPath)
    }

    $packages = try {
        @(& $AppPackageResolver)
    }
    catch {
        @()
    }
    foreach ($package in ($packages | Sort-Object Version -Descending)) {
        $installLocationProperty = $package.PSObject.Properties['InstallLocation']
        if ($null -eq $installLocationProperty -or
            [string]::IsNullOrWhiteSpace([string]$installLocationProperty.Value)) {
            continue
        }
        $candidates.Add(
            (Join-Path ([string]$installLocationProperty.Value) 'app\resources\codex.exe')
        )
    }

    if (-not $PSBoundParameters.ContainsKey('KnownPaths')) {
        $KnownPaths = Get-DefaultCodexDesktopPaths
    }
    foreach ($candidate in @($KnownPaths) + @($AdditionalCandidates)) {
        if (-not [string]::IsNullOrWhiteSpace($candidate)) {
            $candidates.Add($candidate)
        }
    }

    foreach ($candidate in $candidates) {
        $resolvedPath = Resolve-ExistingFilePath -Candidate $candidate
        if ($null -ne $resolvedPath -and
            [System.IO.Path]::GetExtension($resolvedPath) -ieq '.exe') {
            return $resolvedPath
        }
    }
    return $null
}

function Assert-ChildDirectory {
    param(
        [Parameter(Mandatory)][string]$ParentDirectory,
        [Parameter(Mandatory)][string]$ChildDirectory
    )

    $resolvedParent = [System.IO.Path]::GetFullPath($ParentDirectory).TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    )
    $resolvedChild = [System.IO.Path]::GetFullPath($ChildDirectory)
    $parentPrefix = $resolvedParent + [System.IO.Path]::DirectorySeparatorChar
    if (-not $resolvedChild.StartsWith(
            $parentPrefix,
            [System.StringComparison]::OrdinalIgnoreCase
        )) {
        throw "Codex bootstrap directory escaped its parent: $resolvedChild"
    }
}

function Remove-CodexBootstrapCopy {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$DirectoryPath,
        [Parameter(Mandatory)][string]$BaseDirectory
    )

    Assert-ChildDirectory -ParentDirectory $BaseDirectory -ChildDirectory $DirectoryPath
    if (Test-Path -LiteralPath $DirectoryPath -PathType Container) {
        Remove-Item -LiteralPath $DirectoryPath -Recurse -Force
    }
}

function New-CodexBootstrapCopy {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$SourcePath,
        [Parameter(Mandatory)][string]$BaseDirectory
    )

    $resolvedSource = Resolve-ExistingFilePath -Candidate $SourcePath
    if ($null -eq $resolvedSource -or
        [System.IO.Path]::GetExtension($resolvedSource) -ine '.exe') {
        throw "Codex desktop executable does not exist: $SourcePath"
    }

    $resolvedBase = [System.IO.Path]::GetFullPath($BaseDirectory)
    New-Item -ItemType Directory -Path $resolvedBase -Force | Out-Null
    $bootstrapDirectory = [System.IO.Path]::GetFullPath(
        (Join-Path $resolvedBase "codex-$PID-$([Guid]::NewGuid().ToString('N'))")
    )
    Assert-ChildDirectory -ParentDirectory $resolvedBase -ChildDirectory $bootstrapDirectory
    New-Item -ItemType Directory -Path $bootstrapDirectory -Force | Out-Null

    $commandPath = Join-Path $bootstrapDirectory 'codex.exe'
    try {
        Copy-Item -LiteralPath $resolvedSource -Destination $commandPath -Force
        $sourceDigest = (Get-FileHash -LiteralPath $resolvedSource -Algorithm SHA256).Hash
        $copyDigest = (Get-FileHash -LiteralPath $commandPath -Algorithm SHA256).Hash
        if ($sourceDigest -ne $copyDigest) {
            throw 'The temporary Codex executable failed SHA-256 verification.'
        }
        return [pscustomobject]@{
            CommandPath = $commandPath
            DirectoryPath = $bootstrapDirectory
            BaseDirectory = $resolvedBase
        }
    }
    catch {
        Remove-CodexBootstrapCopy `
            -DirectoryPath $bootstrapDirectory `
            -BaseDirectory $resolvedBase
        throw
    }
}
