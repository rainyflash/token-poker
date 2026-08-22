$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$projectRoot = Split-Path -Parent $PSScriptRoot
$manifest = Join-Path $projectRoot "fuzz\Cargo.toml"
cargo check --manifest-path $manifest --bins
