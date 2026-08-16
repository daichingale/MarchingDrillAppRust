param([Parameter(Mandatory = $true)][string]$OutputDirectory)

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null

function New-DrillForgeIcon([string]$Path, [int]$Width, [int]$Height) {
    $bitmap = [Drawing.Bitmap]::new($Width, $Height, [Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.SmoothingMode = [Drawing.Drawing2D.SmoothingMode]::AntiAlias
        $graphics.Clear([Drawing.Color]::FromArgb(18, 61, 42))
        $scale = [Math]::Min($Width, $Height) / 512.0
        $offsetX = ($Width - 512*$scale) / 2
        $offsetY = ($Height - 512*$scale) / 2
        $field = [Drawing.RectangleF]::new($offsetX+76*$scale, $offsetY+118*$scale, 360*$scale, 276*$scale)
        $graphics.FillRectangle([Drawing.SolidBrush]::new([Drawing.Color]::FromArgb(24,86,56)), $field)
        $white = [Drawing.Pen]::new([Drawing.Color]::FromArgb(244,247,245), [Math]::Max(1, 12*$scale))
        $minor = [Drawing.Pen]::new([Drawing.Color]::FromArgb(150,244,247,245), [Math]::Max(1, 6*$scale))
        $graphics.DrawRectangle($white, $field.X, $field.Y, $field.Width, $field.Height)
        $graphics.DrawLine($minor, $offsetX+256*$scale, $offsetY+118*$scale, $offsetX+256*$scale, $offsetY+394*$scale)
        $graphics.DrawLine($minor, $offsetX+76*$scale, $offsetY+256*$scale, $offsetX+436*$scale, $offsetY+256*$scale)
        $dot = [Drawing.SolidBrush]::new([Drawing.Color]::FromArgb(255,201,77))
        foreach ($x in @(168,256,344)) {
            foreach ($y in @(190,322)) {
                $r = 20*$scale
                $graphics.FillEllipse($dot, $offsetX+$x*$scale-$r, $offsetY+$y*$scale-$r, 2*$r, 2*$r)
            }
        }
        $bitmap.Save($Path, [Drawing.Imaging.ImageFormat]::Png)
        $white.Dispose(); $minor.Dispose(); $dot.Dispose()
    } finally {
        $graphics.Dispose(); $bitmap.Dispose()
    }
}

New-DrillForgeIcon (Join-Path $OutputDirectory "Square44x44Logo.png") 44 44
New-DrillForgeIcon (Join-Path $OutputDirectory "Square150x150Logo.png") 150 150
New-DrillForgeIcon (Join-Path $OutputDirectory "Wide310x150Logo.png") 310 150
New-DrillForgeIcon (Join-Path $OutputDirectory "StoreLogo.png") 50 50
