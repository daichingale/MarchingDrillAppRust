$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
[xml]$plist = Get-Content -Raw -LiteralPath (Join-Path $root 'packaging/macos/Info.plist')
if ($plist.plist.version -ne '1.0') { throw 'Invalid plist version' }
$text = Get-Content -Raw -LiteralPath (Join-Path $root 'packaging/macos/Info.plist')
foreach ($token in '__VERSION__','__BUILD_NUMBER__','__BUNDLE_ID__') { if (-not $text.Contains($token)) { throw "Missing plist token $token" } }
foreach ($name in 'package-macos.sh','notarize-macos.sh','test-macos-package.sh') {
  $script = Get-Content -Raw -LiteralPath (Join-Path $root "scripts/$name")
  if (-not $script.StartsWith('#!/usr/bin/env bash')) { throw "$name has no bash shebang" }
  if (-not $script.Contains('set -euo pipefail')) { throw "$name lacks strict shell mode" }
}
$notary = Get-Content -Raw -LiteralPath (Join-Path $root 'scripts/notarize-macos.sh')
if (-not $notary.Contains('trap cleanup EXIT INT TERM')) { throw 'Notary key cleanup is not enforced' }
Write-Host 'macOS packaging static structure passed'
