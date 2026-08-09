[CmdletBinding()]
param([string]$Output = "docs/MESSAGE_CATALOG.md", [switch]$Check)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$sourceRoot = Join-Path $root "crates/drill-app/src"
$trPattern = '(?s)\btr\s*\(\s*(?:self\.locale|locale|Locale::(?:Ja|En))\s*,\s*"((?:\\.|[^"\\])*)"\s*,\s*"((?:\\.|[^"\\])*)"\s*,?\s*\)'
$ifPattern = '(?s)if\s+(?:self\.)?locale\s*==\s*Locale::Ja\s*\{\s*"((?:\\.|[^"\\])*)"\s*\}\s*else\s*\{\s*"((?:\\.|[^"\\])*)"\s*\}'
$rows = [Collections.Generic.List[object]]::new()
foreach ($file in Get-ChildItem $sourceRoot -Filter *.rs | Sort-Object Name) {
    $source = [IO.File]::ReadAllText($file.FullName)
    foreach ($kindPattern in @(@("tr", $trPattern), @("conditional", $ifPattern))) {
        foreach ($match in [regex]::Matches($source, $kindPattern[1])) {
            $ja = $match.Groups[1].Value
            $en = $match.Groups[2].Value
            $line = 1 + ($source.Substring(0, $match.Index).Split("`n").Count - 1)
            $rows.Add([pscustomobject]@{ Kind=$kindPattern[0]; File=$file.Name; Line=$line; Ja=$ja; En=$en })
        }
    }
}
$rows = @($rows | Sort-Object Ja, En, File, Line)
$lines = [Collections.Generic.List[string]]::new()
$lines.Add("# DrillForge message catalog inventory")
$lines.Add("")
$lines.Add("Generated from Rust sources. Do not edit manually. ``Text`` enum entries are")
$lines.Add("validated exhaustively in ``crates/drill-app/src/i18n.rs``; this inventory covers")
$lines.Add("the staged literal-pair API and direct locale conditionals.")
$lines.Add("")
$lines.Add("| ID | Mechanism | Source | Japanese | English |")
$lines.Add("|---|---|---|---|---|")
$index = 0
foreach ($row in $rows) {
    $index++
    $ja = $row.Ja.Replace("|", "\|").Replace("`r", "").Replace("`n", "\n")
    $en = $row.En.Replace("|", "\|").Replace("`r", "").Replace("`n", "\n")
    $lines.Add("| legacy.$($index.ToString('0000')) | $($row.Kind) | ``$($row.File):$($row.Line)`` | $ja | $en |")
}
$content = ($lines -join "`n") + "`n"
$path = Join-Path $root $Output
if ($Check) {
    if (-not (Test-Path $path)) { throw "$Output is missing" }
    $existing = [IO.File]::ReadAllText($path).Replace("`r`n", "`n")
    if ($existing -ne $content) { throw "$Output is stale; regenerate it" }
} else {
    [IO.File]::WriteAllText($path, $content, [Text.UTF8Encoding]::new($false))
    Write-Host "Generated $Output ($($rows.Count) localized pairs)"
}

