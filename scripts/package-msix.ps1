param(
    [string]$OutputDirectory = "dist",
    [string]$IdentityName = "DrillForge",
    [string]$Publisher = "CN=DrillForge Development",
    [string]$Version = "0.1.0.0"
)

$ErrorActionPreference = "Stop"
$workspace = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$binary = Join-Path $workspace "target\release\drill-app.exe"
if (-not (Test-Path -LiteralPath $binary)) { throw "Build the release binary first" }
$dist = Join-Path $workspace $OutputDirectory
$layout = Join-Path $dist "msix-layout"
if (Test-Path -LiteralPath $layout) { Remove-Item -Recurse -Force -LiteralPath $layout }
New-Item -ItemType Directory -Force -Path (Join-Path $layout "Assets") | Out-Null
Copy-Item -LiteralPath $binary -Destination (Join-Path $layout "DrillForge.exe")
Copy-Item -LiteralPath (Join-Path $workspace "packaging\windows\AppxManifest.xml") -Destination $layout
[xml]$manifest = Get-Content -Raw -LiteralPath (Join-Path $layout "AppxManifest.xml")
$ns = [Xml.XmlNamespaceManager]::new($manifest.NameTable)
$ns.AddNamespace("f", "http://schemas.microsoft.com/appx/manifest/foundation/windows10")
$identity = $manifest.SelectSingleNode("/f:Package/f:Identity", $ns)
if (-not $identity) { throw "MSIX manifest has no Identity" }
if ($IdentityName -notmatch '^[A-Za-z0-9.-]{3,50}$') { throw "Invalid MSIX identity name" }
if ($Publisher -notmatch '^CN=.+') { throw "Invalid MSIX publisher subject" }
if ($Version -notmatch '^\d+\.\d+\.\d+\.\d+$') { throw "Invalid MSIX version" }
$identity.SetAttribute("Name", $IdentityName)
$identity.SetAttribute("Publisher", $Publisher)
$identity.SetAttribute("Version", $Version)
$manifest.Save((Join-Path $layout "AppxManifest.xml"))
foreach ($file in @("LICENSE", "LICENSE-MIT", "LICENSE-APACHE", "THIRD_PARTY_NOTICES.md", "THIRD_PARTY_LICENSES.md")) {
    Copy-Item -LiteralPath (Join-Path $workspace $file) -Destination $layout
}
& (Join-Path $PSScriptRoot "generate-windows-assets.ps1") -OutputDirectory (Join-Path $layout "Assets")
$makeAppx = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin" -Recurse -Filter makeappx.exe |
    Where-Object FullName -Match '\\x64\\makeappx\.exe$' | Sort-Object FullName -Descending | Select-Object -First 1
if (-not $makeAppx) { throw "Windows SDK makeappx.exe was not found" }
$package = Join-Path $dist "DrillForge-$($Version.Substring(0, $Version.LastIndexOf('.')))-windows-x64.msix"
if (Test-Path -LiteralPath $package) { Remove-Item -Force -LiteralPath $package }
& $makeAppx.FullName pack /d $layout /p $package /o
if ($LASTEXITCODE -ne 0) { throw "makeappx failed with exit code $LASTEXITCODE" }
$hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $package).Hash.ToLowerInvariant()
Set-Content -LiteralPath "$package.sha256" -Value "$hash  $([IO.Path]::GetFileName($package))" -Encoding ascii
Write-Output $package
