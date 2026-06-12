# Dev harness: find the client window (via Get-Process MainWindowHandle),
# optionally click a client-area coordinate and/or send keystrokes, then
# capture a screenshot of the client area.
# Usage: drive_client.ps1 [-Shot out.png] [-ClickX 55 -ClickY 480] [-Keys "text{ENTER}"]
param(
    [string]$Shot = "",
    [int]$ClickX = -1,
    [int]$ClickY = -1,
    [string]$Keys = ""
)

Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Win32 {
    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")]
    public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int w, int h2, uint flags);
    public const uint SWP_NOSIZE = 0x1, SWP_SHOWWINDOW = 0x40;
    [DllImport("user32.dll")]
    public static extern bool GetClientRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")]
    public static extern bool ClientToScreen(IntPtr h, ref POINT p);
    [DllImport("user32.dll")]
    public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")]
    public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
    public const uint LEFTDOWN = 0x02, LEFTUP = 0x04;
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
}
"@

$proc = Get-Process client -ErrorAction SilentlyContinue |
    Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if ($null -eq $proc) { Write-Output "WINDOW_NOT_FOUND"; exit 1 }
$h = $proc.MainWindowHandle

[Win32]::SetWindowPos($h, [IntPtr]::Zero, 30, 30, 0, 0, [Win32]::SWP_NOSIZE -bor [Win32]::SWP_SHOWWINDOW) | Out-Null
[Win32]::SetForegroundWindow($h) | Out-Null
Start-Sleep -Milliseconds 300

$origin = New-Object Win32+POINT
[Win32]::ClientToScreen($h, [ref]$origin) | Out-Null
$rect = New-Object Win32+RECT
[Win32]::GetClientRect($h, [ref]$rect) | Out-Null
Write-Output ("CLIENT_AREA {0},{1} {2}x{3}" -f $origin.X, $origin.Y, $rect.R, $rect.B)

if ($ClickX -ge 0) {
    $sx = $origin.X + $ClickX
    $sy = $origin.Y + $ClickY
    [Win32]::SetCursorPos($sx, $sy) | Out-Null
    Start-Sleep -Milliseconds 150
    [Win32]::mouse_event([Win32]::LEFTDOWN, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 60
    [Win32]::mouse_event([Win32]::LEFTUP, 0, 0, 0, [UIntPtr]::Zero)
    Write-Output ("CLICKED {0},{1}" -f $ClickX, $ClickY)
    Start-Sleep -Milliseconds 700
}

if ($Keys -ne "") {
    Add-Type -AssemblyName System.Windows.Forms
    [System.Windows.Forms.SendKeys]::SendWait($Keys)
    Write-Output "KEYS_SENT"
    Start-Sleep -Milliseconds 400
}

if ($Shot -ne "") {
    Add-Type -AssemblyName System.Drawing
    $bmp = New-Object System.Drawing.Bitmap($rect.R, $rect.B)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($origin.X, $origin.Y, 0, 0, $bmp.Size)
    $g.Dispose()
    $bmp.Save($Shot, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    Write-Output "SHOT $Shot"
}
