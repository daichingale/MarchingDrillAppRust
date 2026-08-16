param(
    [ValidateSet("Headless", "Manual")]
    [string]$Mode = "Headless",
    [string]$OutputDirectory = "artifacts/windows-uia-dpi",
    [int]$MaxNodes = 400,
    [int]$StartupTimeoutSeconds = 30
)

# Exit 0 = evidence passed, 1 = gate failed, 2 = explicitly skipped.
$ErrorActionPreference = "Stop"
$skipExit = 2
$started = (Get-Date).ToUniversalTime()
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$reportPath = Join-Path $OutputDirectory "report.json"

function Write-Report([string]$Status, [object]$Runs, [string[]]$Notes) {
    [ordered]@{
        schema_version = 1
        mode = $Mode.ToLowerInvariant()
        status = $Status
        started_at_utc = $started.ToString("o")
        finished_at_utc = (Get-Date).ToUniversalTime().ToString("o")
        platform = [Environment]::OSVersion.VersionString
        bounded_node_limit = $MaxNodes
        runs = $Runs
        notes = $Notes
    } | ConvertTo-Json -Depth 9 | Set-Content -Encoding utf8 $reportPath
}

if ($Mode -eq "Headless") {
    $checks = @()
    try {
        cargo test -p drill-app app_state::ui_qa::tests::menu_widgets_render_headlessly_at_supported_scales -- --exact
        if ($LASTEXITCODE -ne 0) { throw "AccessKit scale smoke failed" }
        $checks += [ordered]@{ name = "accesskit-tree-100-150-200"; status = "passed" }
        cargo test -p drill-app app_state::ui_qa::tests::native_tab_enter_space_and_escape_keyboard_contract -- --exact
        if ($LASTEXITCODE -ne 0) { throw "keyboard focus smoke failed" }
        $checks += [ordered]@{ name = "focus-tab-enter-space-escape"; status = "passed" }
        Write-Report "passed" $checks @(
            "Headless smoke uses deterministic AccessKit trees and logical 100/150/200% scales.",
            "It does not claim Windows UIA bridge, physical monitor DPI, or native screenshots. Run -Mode Manual on an interactive Windows desktop for that gate."
        )
        exit 0
    } catch {
        $checks += [ordered]@{ name = "headless-smoke"; status = "failed"; detail = $_.Exception.Message }
        Write-Report "failed" $checks @("See command output for the failing test.")
        exit 1
    }
}

$runningOnWindows = [System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform(
    [System.Runtime.InteropServices.OSPlatform]::Windows)
if (-not $runningOnWindows -or -not [Environment]::UserInteractive) {
    Write-Report "skipped" @() @("Manual UIA/DPI evidence requires an interactive Windows desktop.")
    exit $skipExit
}

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class DrillForgeDpiNative {
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr hwnd);
}
"@

function Get-BoundedUiaTree($Root, [int]$Limit) {
    $walker = [System.Windows.Automation.TreeWalker]::ControlViewWalker
    $queue = [System.Collections.Generic.Queue[object]]::new()
    $queue.Enqueue([ordered]@{ element = $Root; parent = -1 })
    $nodes = @()
    while ($queue.Count -gt 0 -and $nodes.Count -lt $Limit) {
        $entry = $queue.Dequeue()
        $element = $entry.element
        try {
            $current = $element.Current
            $rect = $current.BoundingRectangle
            $index = $nodes.Count
            $nodes += [ordered]@{
                index = $index; parent = $entry.parent
                name = $current.Name; automation_id = $current.AutomationId
                control_type = $current.ControlType.ProgrammaticName
                keyboard_focusable = $current.IsKeyboardFocusable
                has_keyboard_focus = $current.HasKeyboardFocus
                enabled = $current.IsEnabled
                bounds = [ordered]@{ x = $rect.X; y = $rect.Y; width = $rect.Width; height = $rect.Height }
            }
            $child = $walker.GetFirstChild($element)
            while ($null -ne $child -and ($nodes.Count + $queue.Count) -lt $Limit) {
                $queue.Enqueue([ordered]@{ element = $child; parent = $index })
                $child = $walker.GetNextSibling($child)
            }
        } catch { }
    }
    return ,$nodes
}

cargo build -p drill-app
if ($LASTEXITCODE -ne 0) {
    Write-Report "failed" @() @("drill-app build failed")
    exit 1
}

$exe = Join-Path (Get-Location) "target/debug/drill-app.exe"
$runs = @()
try {
    foreach ($scale in @(1.0, 1.5, 2.0)) {
        $env:DRILLFORGE_QA_SCALE = [string]$scale
        $process = Start-Process -FilePath $exe -PassThru
        $deadline = (Get-Date).AddSeconds($StartupTimeoutSeconds)
        $root = $null
        while ((Get-Date) -lt $deadline -and $null -eq $root) {
            Start-Sleep -Milliseconds 200
            $condition = [System.Windows.Automation.PropertyCondition]::new(
                [System.Windows.Automation.AutomationElement]::ProcessIdProperty, $process.Id)
            $root = [System.Windows.Automation.AutomationElement]::RootElement.FindFirst(
                [System.Windows.Automation.TreeScope]::Children, $condition)
        }
        if ($null -eq $root) { throw "UIA root not found at ${scale}x" }
        $nodes = Get-BoundedUiaTree $root $MaxNodes
        if ($nodes.Count -eq 0) { throw "UIA tree empty at ${scale}x" }
        $window = $root.Current.BoundingRectangle
        $png = Join-Path $OutputDirectory ("scale-{0}.png" -f ([int]($scale * 100)))
        if ($window.Width -gt 0 -and $window.Height -gt 0 -and $window.Width -le 8192 -and $window.Height -le 8192) {
            $bitmap = [System.Drawing.Bitmap]::new([int]$window.Width, [int]$window.Height)
            $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
            $graphics.CopyFromScreen([int]$window.X, [int]$window.Y, 0, 0, $bitmap.Size)
            $bitmap.Save($png, [System.Drawing.Imaging.ImageFormat]::Png)
            $graphics.Dispose(); $bitmap.Dispose()
        }
        $screens = @([System.Windows.Forms.Screen]::AllScreens | ForEach-Object {
            [ordered]@{ device_name = $_.DeviceName; primary = $_.Primary; bounds = [ordered]@{ x=$_.Bounds.X; y=$_.Bounds.Y; width=$_.Bounds.Width; height=$_.Bounds.Height } }
        })
        $runs += [ordered]@{
            requested_logical_scale = $scale
            window_dpi = [DrillForgeDpiNative]::GetDpiForWindow([IntPtr]$root.Current.NativeWindowHandle)
            window_bounds = [ordered]@{ x=$window.X; y=$window.Y; width=$window.Width; height=$window.Height }
            monitors = $screens
            uia_node_count = $nodes.Count
            uia_tree = $nodes
            screenshot = $png
        }
        Stop-Process -Id $process.Id -Force
        $process.WaitForExit()
    }
    Remove-Item Env:DRILLFORGE_QA_SCALE -ErrorAction SilentlyContinue
    Write-Report "passed" $runs @(
        "Requested scales are app zoom factors; window_dpi is the actual GetDpiForWindow result.",
        "Monitor bounds are recorded for mixed-monitor review. A human must move the window between different-DPI monitors and rerun/inspect screenshots.",
        "UIA traversal is breadth-first and bounded; focusable and focused state provide tab-order review inputs without injecting keyboard input."
    )
    exit 0
} catch {
    if ($null -ne $process -and -not $process.HasExited) { Stop-Process -Id $process.Id -Force }
    Remove-Item Env:DRILLFORGE_QA_SCALE -ErrorAction SilentlyContinue
    Write-Report "failed" $runs @($_.Exception.Message)
    exit 1
}
