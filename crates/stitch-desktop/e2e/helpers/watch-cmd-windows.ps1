# Poll for visible ConsoleWindowClass titled like system32\cmd.exe.
# Writes VISIBLE_CMD lines to -LogPath while -MarkerPath is absent.
param(
  [Parameter(Mandatory = $true)][string]$LogPath,
  [Parameter(Mandatory = $true)][string]$MarkerPath,
  [int]$TimeoutSec = 180
)

Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class StitchWinEnum {
  public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc lpEnumFunc, IntPtr lParam);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)]
  public static extern int GetClassName(IntPtr hWnd, StringBuilder lpClassName, int nMaxCount);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)]
  public static extern int GetWindowText(IntPtr hWnd, StringBuilder lpString, int nMaxCount);
}
"@

"" | Set-Content -Path $LogPath -Encoding utf8
$deadline = (Get-Date).AddSeconds($TimeoutSec)

while ((Get-Date) -lt $deadline) {
  if (Test-Path -LiteralPath $MarkerPath) { break }
  [StitchWinEnum]::EnumWindows({
    param($h, $l)
    if (-not [StitchWinEnum]::IsWindowVisible($h)) { return $true }
    $cls = New-Object System.Text.StringBuilder 256
    [void][StitchWinEnum]::GetClassName($h, $cls, $cls.Capacity)
    if ($cls.ToString() -ne "ConsoleWindowClass") { return $true }
    $title = New-Object System.Text.StringBuilder 512
    [void][StitchWinEnum]::GetWindowText($h, $title, $title.Capacity)
    $t = $title.ToString()
    if ($t -match '(?i)system32\\cmd\.exe' -or $t -eq 'cmd.exe' -or $t -eq 'C:\WINDOWS\system32\cmd.exe') {
      Add-Content -Path $LogPath -Value ("VISIBLE_CMD " + $t) -Encoding utf8
    }
    return $true
  }, [IntPtr]::Zero) | Out-Null
  Start-Sleep -Milliseconds 120
}
