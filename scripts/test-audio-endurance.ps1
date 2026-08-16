param(
    [ValidateSet('ci', 'short', '30m', '2h')]
    [string]$Profile = 'short',
    [string]$ReportDirectory = 'artifacts/audio-endurance',
    [switch]$Audible
)
$ErrorActionPreference = 'Stop'
$durations = @{ ci = '0.25'; short = '5'; '30m' = '1800'; '2h' = '7200' }
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$report = Join-Path $ReportDirectory "audio-$Profile-$stamp.json"
$arguments = @('run', '--release', '-p', 'drill-audio', '--example', 'audio_device_diagnostic', '--offline', '--', '--duration-seconds', $durations[$Profile], '--report', $report)
if ($Profile -eq 'ci') { $arguments += '--mock' }
if ($Audible) { $arguments += '--audible' }
& cargo @arguments
$code = $LASTEXITCODE
if ($code -eq 77) { Write-Warning 'No default audio device; hardware evidence was not collected.'; exit 77 }
if ($code -ne 0) { throw "Audio endurance failed with exit code $code. Report: $report" }
$evidence = Get-Content -Raw -LiteralPath $report | ConvertFrom-Json
if (-not $evidence.passed) { throw "Audio endurance report did not pass: $report" }
Write-Host "AUDIO-ENDURANCE-PASS profile=$Profile report=$report callbacks=$($evidence.callbacks) underruns=$($evidence.underruns) rss_growth=$($evidence.rss_growth_bytes)"
