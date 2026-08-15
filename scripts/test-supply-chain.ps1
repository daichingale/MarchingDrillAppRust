$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    cargo metadata --locked --offline --format-version 1 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Cargo.lock is inconsistent with workspace manifests" }

    & "$PSScriptRoot/generate-third-party-licenses.ps1" -Check

    $required = @("deny.toml", "Cargo.lock", "THIRD_PARTY_LICENSES.md", "THIRD_PARTY_NOTICES.md")
    foreach ($path in $required) {
        if (-not (Test-Path -LiteralPath $path)) { throw "Required supply-chain file missing: $path" }
    }

    $unsafe = @(Get-ChildItem crates -Recurse -Filter *.rs |
        Where-Object { $_.FullName -notmatch '[\\/]benches[\\/]' } |
        Select-String -Pattern '\bunsafe\s*(\{|fn\b|impl\b|extern\b|trait\b)' |
        Where-Object {
            $_.Path -notlike '*drill-updater\src\transport.rs' -and
            $_.Path -notlike '*drill-project\src\lib.rs' -and
            # Diagnostic-only Windows PSAPI FFI. It reads this process' memory
            # counters into a fully initialized, layout-compatible structure.
            $_.Path -notlike '*drill-audio\examples\audio_device_diagnostic.rs'
        })
    if ($unsafe.Count -ne 0) {
        $unsafe | ForEach-Object { Write-Error "$($_.Path):$($_.LineNumber): unexpected unsafe code" }
        throw "Unsafe code exists outside reviewed platform interop modules"
    }

    $vendor = Get-ChildItem -Directory -Force | Where-Object Name -eq "vendor"
    if ($vendor) { throw "Vendored dependency sources require an explicit security review" }

    $deny = Get-Command cargo-deny -ErrorAction SilentlyContinue
    if ($deny) {
        cargo deny check advisories bans licenses sources
        if ($LASTEXITCODE -ne 0) { throw "cargo-deny policy failed" }
    } else {
        Write-Host "cargo-deny is unavailable; deterministic offline checks passed (CI runs cargo-deny)."
    }
} finally {
    Pop-Location
}
