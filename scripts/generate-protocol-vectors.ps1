$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location $projectRoot
try {
    cargo run -p token-holdem-network --example generate_protocol_vectors
} finally {
    Pop-Location
}
