# P2: настоящий двойной клик мышью по папке в окне Проводника.
# Перед кликом окно ставится поверх остальных, и скрипт проверяет, что под курсором именно оно.
# Запуск: powershell -NoProfile -ExecutionPolicy Bypass -File test-p2-mouse.ps1
$ErrorActionPreference = 'Stop'
$root = 'C:\dev\zamok\tests\tmp\p2'
$dll = Join-Path $PSScriptRoot 'shellext\target\release\p2_shellext.dll'
$clsid = '{5A3D6C1E-7B2F-4E8A-9C41-2F6B8D0E7A13}'
$log = Join-Path $root 'log.txt'
$parent = Join-Path $root 'parent'
$locked = Join-Path $parent 'Locked'; $normal = Join-Path $parent 'Normal'
New-Item -ItemType Directory -Force $locked, $normal | Out-Null

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class M {
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, IntPtr e);
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
  [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint flags);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint f);
  [DllImport("user32.dll")] public static extern int GetDoubleClickTime();
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  public static bool TopIs(int x, int y, IntPtr top) { var p = new POINT { X = x, Y = y }; return GetAncestor(WindowFromPoint(p), 2) == top; }
  public static void DoubleClick(int x, int y) {
    SetCursorPos(x, y); System.Threading.Thread.Sleep(150);
    for (int i = 0; i < 2; i++) { mouse_event(2, 0, 0, 0, IntPtr.Zero); mouse_event(4, 0, 0, 0, IntPtr.Zero); System.Threading.Thread.Sleep(60); }
  }
}
'@

[void][M]::SetProcessDPIAware()
function WaitFor([scriptblock]$cond, [int]$sec = 8) {
    $deadline = (Get-Date).AddSeconds($sec)
    while (-not (& $cond)) { if ((Get-Date) -gt $deadline) { return $false }; Start-Sleep -Milliseconds 150 }
    $true
}
function LogText { if (Test-Path $log) { (Get-Content $log -Raw) -replace "`r?`n", ' / ' } else { '' } }
$shell = New-Object -ComObject Shell.Application
function Win([string]$path) { @($shell.Windows() | Where-Object { try { $_.Document.Folder.Self.Path -eq $path } catch { $false } }) | Select-Object -First 1 }

# Регистрация (HKCU)
$k = "HKCU:\Software\Classes\CLSID\$clsid"
New-Item -Force "$k\InprocServer32" | Out-Null
Set-ItemProperty "$k\InprocServer32" '(default)' $dll; Set-ItemProperty "$k\InprocServer32" 'ThreadingModel' 'Apartment'
New-Item -Force 'HKCU:\Software\Classes\Directory\shellex\ContextMenuHandlers\ZamokP2' | Out-Null
Set-ItemProperty 'HKCU:\Software\Classes\Directory\shellex\ContextMenuHandlers\ZamokP2' '(default)' $clsid
New-Item -Force 'HKCU:\Software\ZamokP2' | Out-Null
Set-ItemProperty 'HKCU:\Software\ZamokP2' 'Locked' $locked; Set-ItemProperty 'HKCU:\Software\ZamokP2' 'Log' $log

$out = @()
try {
    Start-Process explorer.exe $parent
    $null = WaitFor { (Win $parent) -ne $null } 10
    $w = Win $parent
    $h = [IntPtr]$w.HWND
    [void][M]::SetWindowPos($h, [IntPtr](-1), 100, 100, 900, 600, 0x40)   # поверх всех, 900x600
    Start-Sleep 1
    foreach ($name in 'Locked', 'Normal') {
        Remove-Item $log -ErrorAction SilentlyContinue
        $w.Navigate($parent); Start-Sleep -Milliseconds 1500
        $ae = [Windows.Automation.AutomationElement]::FromHandle($h)
        $cond = New-Object Windows.Automation.AndCondition(
            (New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::ControlTypeProperty, [Windows.Automation.ControlType]::ListItem)),
            (New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::NameProperty, $name)))
        $li = $ae.FindFirst([Windows.Automation.TreeScope]::Descendants, $cond)
        if (-not $li) { $out += "${name}: item not found"; continue }
        $r = $li.Current.BoundingRectangle
        $x = [int]($r.X + 20); $y = [int]($r.Y + $r.Height / 2)
        if (-not [M]::TopIs($x, $y, $h)) { $out += "${name}: point ($x,$y) is not over the test window, click skipped"; continue }
        [M]::DoubleClick($x, $y)
        $target = if ($name -eq 'Locked') { $locked } else { $normal }
        $moved = WaitFor { try { $w.Document.Folder.Self.Path -eq $target } catch { $false } } 6
        Start-Sleep -Milliseconds 500
        $out += "${name}: double-click at ($x,$y); same window now inside folder=$moved; log=[$(LogText)]"
    }
    $mods = @((Get-Process explorer).Modules | Where-Object { $_.ModuleName -eq 'p2_shellext.dll' }).Count
    $out += "p2_shellext.dll loaded in explorer.exe: $($mods -gt 0)"
}
finally {
    try { $w.Quit() } catch {}
    Remove-Item 'HKCU:\Software\Classes\Directory\shellex\ContextMenuHandlers\ZamokP2' -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item "HKCU:\Software\Classes\CLSID\$clsid" -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item 'HKCU:\Software\ZamokP2' -Recurse -Force -ErrorAction SilentlyContinue
}
$out
