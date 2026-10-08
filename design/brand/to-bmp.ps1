# 把一张 PNG 缩放并转成 24bpp BMP —— NSIS / WiX 的安装向导图只吃这种格式：
# 必须是 BMP、必须 24 位（无 alpha），否则安装器直接报错或显示花屏。
#
# 用法：powershell -File to-bmp.ps1 -In a.png -Out b.bmp -Width 150 -Height 57
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
    # 底色跟应用一致，缩放时边缘不会渗出黑边
    $g.Clear([System.Drawing.Color]::FromArgb(245, 246, 247))
    $g.DrawImage($src, (New-Object System.Drawing.Rectangle 0, 0, $Width, $Height))
  } finally {
    $g.Dispose()
  }
  $bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Bmp)
  $bmp.Dispose()
} finally {
  $src.Dispose()
}

Write-Output ("wrote " + $Out)
