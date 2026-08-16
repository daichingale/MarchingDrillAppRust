[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("reviewed")]
    [string]$Approve
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    $env:UPDATE_GOLDENS = $Approve
    cargo test -p drill-conformance --test golden_approval --locked -- --nocapture
    if ($LASTEXITCODE -ne 0) { throw "Golden generation failed" }
    Remove-Item Env:UPDATE_GOLDENS -ErrorAction SilentlyContinue
    cargo test -p drill-conformance --test golden_approval --locked -- --nocapture
    if ($LASTEXITCODE -ne 0) { throw "Generated goldens do not verify" }
    Write-Host "Goldens generated and verified. Review 'git diff -- crates/drill-conformance/tests/golden' before committing."
}
finally {
    Remove-Item Env:UPDATE_GOLDENS -ErrorAction SilentlyContinue
    Pop-Location
}
