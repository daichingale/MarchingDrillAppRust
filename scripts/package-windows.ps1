param(
    [string]$Configuration = "release",
    [string]$OutputDirectory = "dist"
)

$ErrorActionPreference = "Stop"
$workspace = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$manifest = Get-Content -Raw -LiteralPath (Join-Path $workspace "Cargo.toml")
$versionMatch = [regex]::Match($manifest, '(?m)^version\s*=\s*"([^"]+)"')
if (-not $versionMatch.Success) {
    throw "Could not determine workspace version"
}

$version = $versionMatch.Groups[1].Value
$binary = Join-Path $workspace "target\$Configuration\drill-app.exe"
if (-not (Test-Path -LiteralPath $binary)) {
    throw "Missing $binary. Run cargo build --workspace --release first."
}

$dist = Join-Path $workspace $OutputDirectory
$stage = Join-Path $dist "DrillForge-$version-windows-x64"
$archive = "$stage.zip"
if (Test-Path -LiteralPath $stage) { Remove-Item -Recurse -Force -LiteralPath $stage }
if (Test-Path -LiteralPath $archive) { Remove-Item -Force -LiteralPath $archive }
New-Item -ItemType Directory -Force -Path $stage | Out-Null

Copy-Item -LiteralPath $binary -Destination (Join-Path $stage "DrillForge.exe")
foreach ($file in @("README.md", "LICENSE", "LICENSE-MIT", "LICENSE-APACHE", "THIRD_PARTY_NOTICES.md", "THIRD_PARTY_LICENSES.md")) {
    Copy-Item -LiteralPath (Join-Path $workspace $file) -Destination $stage
}
New-Item -ItemType Directory -Force -Path (Join-Path $stage "assets") | Out-Null
Copy-Item -LiteralPath (Join-Path $workspace "assets\NotoSansJP.ttf") -Destination (Join-Path $stage "assets")
Copy-Item -LiteralPath (Join-Path $workspace "assets\OFL-NotoSansJP.txt") -Destination (Join-Path $stage "assets")

Compress-Archive -LiteralPath $stage -DestinationPath $archive -CompressionLevel Optimal
$hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $archive).Hash.ToLowerInvariant()
Set-Content -LiteralPath "$archive.sha256" -Value "$hash  $([IO.Path]::GetFileName($archive))" -Encoding ascii
Write-Output $archive
