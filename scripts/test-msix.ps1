param(
    [string]$Package = "dist/DrillForge-0.1.0-windows-x64.msix",
    [switch]$Release
)

$ErrorActionPreference = "Stop"
$workspace = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$packagePath = (Resolve-Path (Join-Path $workspace $Package)).Path
$scratch = Join-Path $workspace "dist\msix-validation"
if (Test-Path -LiteralPath $scratch) { Remove-Item -Recurse -Force -LiteralPath $scratch }
New-Item -ItemType Directory -Force -Path $scratch | Out-Null

$makeAppx = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin" -Recurse -Filter makeappx.exe |
    Where-Object FullName -Match '\\x64\\makeappx\.exe$' | Sort-Object FullName -Descending | Select-Object -First 1
if (-not $makeAppx) { throw "Windows SDK makeappx.exe was not found" }
& $makeAppx.FullName unpack /p $packagePath /d $scratch /o | Out-Null
if ($LASTEXITCODE -ne 0) { throw "MSIX unpack validation failed" }

[xml]$manifest = Get-Content -Raw -LiteralPath (Join-Path $scratch "AppxManifest.xml")
$ns = [Xml.XmlNamespaceManager]::new($manifest.NameTable)
$ns.AddNamespace("f", "http://schemas.microsoft.com/appx/manifest/foundation/windows10")
$identity = $manifest.SelectSingleNode("/f:Package/f:Identity", $ns)
if (-not $identity -or $identity.ProcessorArchitecture -ne "x64") { throw "MSIX identity is not x64" }
if ($identity.Version -notmatch '^\d+\.\d+\.\d+\.\d+$') { throw "MSIX version is invalid" }
if ($Release) {
    if ($identity.Publisher -match 'Development' -or $identity.Name -eq 'DrillForge') {
        throw "Release MSIX still uses the development identity"
    }
    $signature = Get-AuthenticodeSignature -LiteralPath $packagePath
    if ($signature.Status -ne [System.Management.Automation.SignatureStatus]::Valid) {
        throw "Release MSIX signature is not valid: $($signature.Status)"
    }
    if ($signature.SignerCertificate.Subject -ne $identity.Publisher) {
        throw "Signer subject does not match manifest publisher"
    }
}

$required = @(
    "DrillForge.exe", "LICENSE", "LICENSE-MIT", "LICENSE-APACHE",
    "THIRD_PARTY_NOTICES.md", "THIRD_PARTY_LICENSES.md", "Assets\Square44x44Logo.png",
    "Assets\Square150x150Logo.png", "Assets\Wide310x150Logo.png", "Assets\StoreLogo.png"
)
foreach ($relative in $required) {
    if (-not (Test-Path -LiteralPath (Join-Path $scratch $relative))) { throw "Missing MSIX payload: $relative" }
}

Add-Type -AssemblyName System.Drawing
$sizes = @{
    "Square44x44Logo.png" = @(44,44); "Square150x150Logo.png" = @(150,150)
    "Wide310x150Logo.png" = @(310,150); "StoreLogo.png" = @(50,50)
}
foreach ($entry in $sizes.GetEnumerator()) {
    $image = [Drawing.Image]::FromFile((Join-Path $scratch "Assets\$($entry.Key)"))
    try {
        if ($image.Width -ne $entry.Value[0] -or $image.Height -ne $entry.Value[1]) {
            throw "Unexpected dimensions for $($entry.Key): $($image.Width)x$($image.Height)"
        }
    } finally { $image.Dispose() }
}

$sidecar = "$packagePath.sha256"
if (-not (Test-Path -LiteralPath $sidecar)) { throw "Missing SHA-256 sidecar" }
$expected = ((Get-Content -Raw -LiteralPath $sidecar) -split '\s+')[0].ToLowerInvariant()
$actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $packagePath).Hash.ToLowerInvariant()
if ($actual -ne $expected) { throw "MSIX SHA-256 sidecar mismatch" }
Write-Output "MSIX preflight passed: $actual"
