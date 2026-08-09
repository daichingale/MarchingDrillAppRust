[CmdletBinding()]
param(
    [switch]$RequireTools
)

$ErrorActionPreference = 'Stop'
$workspace = Split-Path -Parent $PSScriptRoot
Push-Location $workspace
try {
    & cargo run --locked --release -p drill-export --bin video_export_e2e
    $status = $LASTEXITCODE
    if ($status -eq 77) {
        if ($RequireTools) {
            throw 'FFmpeg E2E gate unavailable. Install ffmpeg/ffprobe or set DRILLFORGE_FFMPEG and DRILLFORGE_FFPROBE to absolute paths.'
        }
        Write-Warning 'FFmpeg E2E gate unavailable (exit 77); no test success is being claimed.'
        exit 77
    }
    if ($status -ne 0) {
        throw "FFmpeg E2E gate failed with exit code $status."
    }
} finally {
    Pop-Location
}
