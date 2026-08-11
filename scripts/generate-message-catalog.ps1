# One-shot migration snapshot, NOT a routinely-regenerated artifact.
#
# docs/MESSAGE_CATALOG.md records the Japanese/English pairing exactly as it
# existed when the product still wrote UI text as literal tr(locale, "ja",
# "en") calls and `if locale == Locale::Ja { "..." } else { "..." }`
# conditionals. scripts/test-message-catalog.ps1 treats that file as the
# authoritative record of what each migrated call site used to mean, and
# diffs it against crates/drill-app/src/i18n_generated.rs to prove the
# tr()/if-else -> registered(locale, "id") migration preserved meaning.
#
# The product has since finished migrating almost every call site to the ID
# form, so this scanner's two textual patterns now match almost nothing in
# current sources. Rerunning it against today's code and overwriting
# docs/MESSAGE_CATALOG.md with that near-empty result destroys the only
# record test-message-catalog.ps1 checks against -- there is no way to
# recover the original mapping from git history alone once the source lines
# have moved. Do not add new UI text by reintroducing tr()/if-Locale::Ja
# literals to make this scanner pick them up; add a new ID pair directly to
# i18n_generated.rs instead (see i18n.rs's docs).
#
# The -MinPairs guard below exists so an accidental rerun fails loudly
# instead of silently truncating the file.
[CmdletBinding()]
param(
    [string]$Output = "docs/MESSAGE_CATALOG.md",
    [switch]$Check,
    # Refuses to overwrite $Output if the freshly-scanned pair count would
    # shrink by more than this fraction. Set lower only if you are
    # deliberately re-snapshotting after removing legacy call sites, and
    # have confirmed nothing still depends on the entries being dropped.
    [double]$MaxShrinkFraction = 0.05
)
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
    if ($existing -ne $content) {
        throw "$Output does not match a fresh scan of crates/drill-app/src. " +
            "If the product has migrated call sites to i18n_generated.rs (the " +
            "usual case), that is expected and $Output should NOT be " +
            "regenerated -- see this script's header comment. Only rerun " +
            "without -Check if you intend to replace the historical snapshot."
    }
} else {
    if (Test-Path $path) {
        $existingCount = @(Get-Content $path | Where-Object { $_ -match '^\| legacy\.\d+ \|' }).Count
        if ($existingCount -gt 0) {
            $shrink = 1.0 - ($rows.Count / [double]$existingCount)
            if ($shrink -gt $MaxShrinkFraction) {
                throw ("Refusing to overwrite $Output`: a fresh scan found $($rows.Count) " +
                    "pairs versus $existingCount currently recorded (a " +
                    "$([math]::Round($shrink * 100))% drop, over the " +
                    "$([math]::Round($MaxShrinkFraction * 100))% guard). This almost " +
                    "always means product code has moved from tr()/if-else literals to " +
                    "registered(locale, `"id`") calls, which this textual scanner cannot " +
                    "see -- NOT that those UI strings were removed. Overwriting now would " +
                    "destroy the historical mapping scripts/test-message-catalog.ps1 relies " +
                    "on. See this script's header comment before proceeding, and pass a " +
                    "lower -MaxShrinkFraction only if you have confirmed the drop is " +
                    "intentional.")
            }
        }
    }
    [IO.File]::WriteAllText($path, $content, [Text.UTF8Encoding]::new($false))
    Write-Host "Generated $Output ($($rows.Count) localized pairs)"
}

