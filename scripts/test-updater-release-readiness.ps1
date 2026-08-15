param(
    [string]$ReportPath = "artifacts/release-readiness/updater.json"
)

$ErrorActionPreference = "Stop"
$started = (Get-Date).ToUniversalTime()
$checks = @()

function Invoke-ReadinessCheck([string]$Name, [scriptblock]$Command) {
    try {
        & $Command
        if ($LASTEXITCODE -ne 0) { throw "$Name exited with $LASTEXITCODE" }
        $script:checks += [ordered]@{ name = $Name; status = "passed" }
    } catch {
        $script:checks += [ordered]@{ name = $Name; status = "failed"; detail = $_.Exception.Message }
        throw
    }
}

$succeeded = $false
try {
    Invoke-ReadinessCheck "updater-e2e" { cargo test -p drill-updater --test release_readiness }
    Invoke-ReadinessCheck "updater-tests" { cargo test -p drill-updater --lib }
    Invoke-ReadinessCheck "updater-clippy" { cargo clippy -p drill-updater --all-targets -- -D warnings }
    $succeeded = $true
} finally {
    $report = [ordered]@{
        schema_version = 1
        component = "drill-updater"
        status = if ($succeeded) { "passed" } else { "failed" }
        started_at_utc = $started.ToString("o")
        finished_at_utc = (Get-Date).ToUniversalTime().ToString("o")
        guarantees = @(
            "signed manifest verified through production verifier API"
            "package and attestation hashes and signer identity verified"
            "channel and blackout policy exercised"
            "redirect, content type, size, and authenticated HTTPS policy fail closed"
            "download result remains inert; no automatic execution API"
        )
        endpoint = "localhost test server through pure transport seam; no production endpoint claimed"
        checks = $checks
    }
    $parent = Split-Path -Parent $ReportPath
    New-Item -ItemType Directory -Force -Path $parent | Out-Null
    $report | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 $ReportPath
}
