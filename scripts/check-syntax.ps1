[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$syntaxFailures = [System.Collections.Generic.List[string]]::new()
$strictUtf8 = [System.Text.UTF8Encoding]::new($false, $true)

Get-ChildItem -LiteralPath (Join-Path $projectRoot 'scripts') -Filter '*.ps1' -File |
    Sort-Object FullName |
    ForEach-Object {
        $tokens = $null
        $parseErrors = $null
        $scriptSource = [System.IO.File]::ReadAllText($_.FullName, $strictUtf8)
        [void][System.Management.Automation.Language.Parser]::ParseInput(
            $scriptSource,
            $_.FullName,
            [ref]$tokens,
            [ref]$parseErrors
        )
        foreach ($parseError in $parseErrors) {
            $syntaxFailures.Add("$($_.Name):$($parseError.Extent.StartLineNumber) $($parseError.Message)")
        }
    }

$nodeScriptRoots = @(
    (Join-Path $projectRoot 'scripts'),
    (Join-Path $projectRoot 'plugins\token-holdem\mcp\src'),
    (Join-Path $projectRoot 'plugins\token-holdem\mcp\test')
)
$nodeScripts = foreach ($root in $nodeScriptRoots) {
    Get-ChildItem -LiteralPath $root -Filter '*.mjs' -File -Recurse
}
$nodeScripts += Get-Item -LiteralPath (Join-Path $projectRoot 'plugins\token-holdem\mcp\build.mjs')

$nodeScripts |
    Sort-Object FullName -Unique |
    ForEach-Object {
        & node --check $_.FullName
        if ($LASTEXITCODE -ne 0) {
            $syntaxFailures.Add("$($_.Name): node --check failed")
        }
    }

if ($syntaxFailures.Count -gt 0) {
    throw "Script syntax validation failed:`n$($syntaxFailures -join "`n")"
}

Write-Output 'PowerShell and Node.js script syntax validation passed.'
