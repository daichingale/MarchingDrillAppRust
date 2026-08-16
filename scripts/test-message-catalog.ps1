$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$sourceRoot = Join-Path $root "crates/drill-app/src"
$generatedPath = Join-Path $sourceRoot "i18n_generated.rs"
$generated = [IO.File]::ReadAllText($generatedPath)
$product = (Get-ChildItem $sourceRoot -Filter *.rs |
    Where-Object Name -NotIn @('i18n.rs', 'i18n_generated.rs') |
    ForEach-Object { [IO.File]::ReadAllText($_.FullName) }) -join "`n"

$calls = [regex]::Matches($product, '(?:super::)?i18n::registered\([^,]+,\s*"([a-z0-9-]+\.\d{3})"\)')
$ja = [regex]::Matches($generated, '(?s)\(Locale::Ja, "([a-z0-9-]+\.\d{3})"\) =>\s*(?:\{\s*)?"((?:\\.|[^"\\])*)"')
$en = [regex]::Matches($generated, '(?s)\(Locale::En, "([a-z0-9-]+\.\d{3})"\) =>\s*(?:\{\s*)?"((?:\\.|[^"\\])*)"')
$callIds = @($calls | ForEach-Object { $_.Groups[1].Value } | Sort-Object -Unique)
$jaIds = @($ja | ForEach-Object { $_.Groups[1].Value } | Sort-Object -Unique)
$enIds = @($en | ForEach-Object { $_.Groups[1].Value } | Sort-Object -Unique)
if ($calls.Count -ne $callIds.Count) { throw "Registered catalog IDs must not be reused: calls=$($calls.Count) ids=$($callIds.Count)" }
foreach ($id in $callIds) {
    if ($id -notin $jaIds -or $id -notin $enIds) { throw "Registered call lacks a locale pair: $id" }
}
foreach ($entry in $en) {
    if ($entry.Groups[2].Value -match '[\p{IsHiragana}\p{IsKatakana}\p{IsCJKUnifiedIdeographs}]') {
        throw "Japanese glyph leaked into English $($entry.Groups[1].Value)"
    }
}

# The pre-migration inventory remains the authoritative semantic mapping.
# Reconstruct each file's line-order ordinal and compare both locale values.
$legacy = @()
foreach ($line in Get-Content (Join-Path $root 'docs/MESSAGE_CATALOG.md')) {
    if ($line -match '^\| legacy\.\d+ \| (?:tr|conditional) \| `([^:]+):(\d+)` \| (.*) \| (.*) \|$') {
        $legacy += [pscustomobject]@{
            File = $matches[1]; Line = [int]$matches[2]
            Ja = $matches[3].Replace('\|', '|'); En = $matches[4].Replace('\|', '|')
        }
    }
}
$actualJa = @{}; foreach ($entry in $ja) { $actualJa[$entry.Groups[1].Value] = $entry.Groups[2].Value }
$actualEn = @{}; foreach ($entry in $en) { $actualEn[$entry.Groups[1].Value] = $entry.Groups[2].Value }
foreach ($group in $legacy | Group-Object File) {
    $base = [IO.Path]::GetFileNameWithoutExtension($group.Name).Replace('_', '-')
    $ordinal = 0
    foreach ($entry in $group.Group | Sort-Object Line) {
        $ordinal++
        $id = $base + '.' + $ordinal.ToString('000')
        if ($actualJa[$id] -cne $entry.Ja -or $actualEn[$id] -cne $entry.En) {
            throw "Catalog semantic mapping differs from legacy inventory at $id"
        }
    }
}
if ([regex]::IsMatch($product, '(?s)\btr\s*\([^,]+,\s*"')) { throw "Literal tr() call re-entered product source" }
if ([regex]::IsMatch($product, '(?s)if\s+(?:self\.)?locale\s*==\s*Locale::Ja\s*\{\s*"')) { throw "Direct static locale conditional re-entered product source" }

# Reviewed manifest for every pre-migration file+line containing multiple
# localized pairs (6 groups / 13 pairs). These context assertions preserve
# meaning where the legacy inventory could not encode same-line ordering.
$app = [IO.File]::ReadAllText((Join-Path $sourceRoot 'app_state.rs'))
$plugin = [IO.File]::ReadAllText((Join-Path $sourceRoot 'plugin_state.rs'))
$workspace = [IO.File]::ReadAllText((Join-Path $sourceRoot 'workspace_inspector.rs'))
$reviewed = @(
    @($app, 'NearestLine=>i18n::registered(self.locale, "app-state.026")'),
    @($app, 'NearestHash=>i18n::registered(self.locale, "app-state.025")'),
    @($app, 'i18n::registered(self.locale, "app-state.082"),'),
    @($app, 'i18n::registered(self.locale, "app-state.083"),'),
    @($app, 'i18n::registered(self.locale, "app-state.084"),'),
    @($app, 'i18n::registered(self.locale, "app-state.085"),'),
    @($app, 'i18n::registered(self.locale, "app-state.097"),'),
    @($app, 'i18n::registered(self.locale, "app-state.098"),'),
    @($app, 'i18n::registered(self.locale, "app-state.096")'),
    @($plugin, 'egui::Button::new(super::i18n::registered(locale, "plugin-state.010"))'),
    @($plugin, '.on_disabled_hover_text(super::i18n::registered(locale, "plugin-state.009"))'),
    @($workspace, '.button(super::i18n::registered(self.locale, "workspace-inspector.046"))'),
    @($workspace, '.on_hover_text(super::i18n::registered(self.locale, "workspace-inspector.047"))')
)
foreach ($entry in $reviewed) {
    if (-not $entry[0].Contains($entry[1])) { throw "Reviewed multi-pair mapping changed: $($entry[1])" }
}

# Locale-neutral raw UI tokens are deliberately tiny and reviewed: arrows and
# mathematical controls carry no language; DCI 8-to-5 is a notation standard;
# CG is an editable section-name example, not interface prose.
$neutralUi = @('→', '−', '＋', '×', '－15°', '＋15°', '＋ 10%', '－ 10%', 'DCI 8-to-5', 'CG')
$rawUiPattern = '\.(?:heading|label|button|small_button|checkbox|on_hover_text|on_disabled_hover_text|text|hint_text)\(\s*"((?:\\.|[^"\\])*)"'
foreach ($file in Get-ChildItem $sourceRoot -Filter *.rs | Where-Object Name -NotIn @('i18n.rs', 'i18n_generated.rs', 'ui_qa.rs')) {
    foreach ($match in [regex]::Matches([IO.File]::ReadAllText($file.FullName), $rawUiPattern)) {
        if ($match.Groups[1].Value -notin $neutralUi) {
            throw "Uncatalogued raw UI literal in $($file.Name): $($match.Groups[1].Value)"
        }
    }
}
Write-Host "Message catalog PASS: $($calls.Count) unique callsite IDs, complete JA/EN locales, authoritative legacy mapping, no English glyph leak or direct static literals"
