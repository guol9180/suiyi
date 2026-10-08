# Convert a PNG into a 24bpp BMP for the installer artwork.
#
# NSIS / WiX only accept BMP for the wizard images, and they must be 24-bit
# without alpha, otherwise the installer errors out or shows garbage.
#
# Usage: powershell -File to-bmp.ps1 -In a.png -Out b.bmp -Width 150 -Height 57
#
# NOTE: keep this file pure ASCII. Windows PowerShell 5.1 reads BOM-less files
# as ANSI, so non-ASCII comments get mis-decoded and can swallow the code after
# them (we lost a whole conversion that way: the bitmap stayed all black).
param(
  [Parameter(Mandatory = $true)][string]$In,
  [Parameter(Mandatory = $true)][string]$Out,
  [Parameter(Mandatory = $true)][int]$Width,
  [Parameter(Mandatory = $true)][int]$Height
)

Add-Type -AssemblyName System.Drawing

$src = [System.Drawing.Image]::FromFile($In)
try {
  $bmp = New-Object System.Drawing.Bitmap $Width, $Height, ([System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  try {
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
    # Same base colour as the app, so scaled edges do not bleed dark pixels
    $g.Clear([System.Drawing.Color]::FromArgb(245, 246, 247))
    if ($src.Width -eq $Width -and $src.Height -eq $Height) {
      # Same size already: copy 1:1, no resampling (resampling flattens small text)
      # Must use the Rectangle overload: DrawImageUnscaled(image, x, y) renders an
      # all-black image for a 32bpp source onto a 24bpp target, as observed here.
      $g.DrawImageUnscaled($src, (New-Object System.Drawing.Rectangle 0, 0, $Width, $Height))
    } else {
      $g.DrawImage($src, (New-Object System.Drawing.Rectangle 0, 0, $Width, $Height))
    }
  } finally {
    $g.Dispose()
  }
  $bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Bmp)
  $bmp.Dispose()
} finally {
  $src.Dispose()
}

Write-Output ("wrote " + $Out)
