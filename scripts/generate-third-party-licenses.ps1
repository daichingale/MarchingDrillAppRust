[CmdletBinding()]
param(
    [string]$Output = "THIRD_PARTY_LICENSES.md",
    [switch]$Check
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    $metadata = cargo metadata --locked --offline --format-version 1 | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw "cargo metadata failed" }

    $packages = @($metadata.packages | Sort-Object name, version | ForEach-Object {
        $source = if ($_.source) { [string]$_.source } else { "workspace" }
        # Registry and git sources are URLs. A Windows metadata string may still
        # use backslashes for a path source; compare the slash form.
        $source = $source.Replace("\", "/")
        $license = if ($_.license) { [string]$_.license } else { "UNKNOWN" }
        "| $($_.name) | $($_.version) | $license | $source |"
    })
    # `* text=auto` checks Cargo.lock out as CRLF on Windows. Hash the LF bytes
    # Git stores, or the inventory hash disagrees with Unix CI while the lock
    # contents are unchanged.
    $lockText = [IO.File]::ReadAllText((Join-Path $root "Cargo.lock")).Replace("`r`n", "`n").Replace("`r", "`n")
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        $hashBytes = $sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($lockText))
    } finally {
        $sha.Dispose()
    }
    $lockHash = ([BitConverter]::ToString($hashBytes)).Replace("-", "").ToLowerInvariant()
    $lines = @(
        "# Third-party dependency inventory",
        "",
        "Generated from committed ``Cargo.lock`` and Cargo package metadata. Do not edit manually.",
        "Regenerate with ``pwsh ./scripts/generate-third-party-licenses.ps1``.",
        "",
        "Cargo.lock SHA-256: ``$lockHash``",
        "",
        "| Package | Version | SPDX license expression | Source |",
        "|---|---:|---|---|"
    ) + $packages + @(
        "",
        "The bundled Noto Sans Japanese font is covered separately by ``assets/OFL-NotoSansJP.txt``.",
        "FFmpeg is not bundled; see ``THIRD_PARTY_NOTICES.md``."
    )
    $content = ($lines -join "`n") + "`n"
    $path = Join-Path $root $Output
    if ($Check) {
        if (-not (Test-Path -LiteralPath $path)) { throw "$Output is missing; regenerate it" }
        $existing = [IO.File]::ReadAllText($path).Replace("`r`n", "`n")
        if ($existing -ne $content) { throw "$Output is stale; regenerate it" }
    } else {
        [IO.File]::WriteAllText($path, $content, [Text.UTF8Encoding]::new($false))
        Write-Host "Generated $Output ($($packages.Count) packages)"
    }
} finally {
    Pop-Location
}

