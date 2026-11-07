# P2: двойной клик (действие по умолчанию) по запертой папке.
# Регистрирует пробную DLL только для текущего пользователя (HKCU), проверяет и убирает её.
# Запуск: powershell -NoProfile -ExecutionPolicy Bypass -File test-p2.ps1
$ErrorActionPreference = 'Stop'
$root = 'C:\dev\zamok\tests\tmp\p2'
$dll = Join-Path $PSScriptRoot 'shellext\target\release\p2_shellext.dll'
$clsid = '{5A3D6C1E-7B2F-4E8A-9C41-2F6B8D0E7A13}'
$log = Join-Path $root 'log.txt'
$results = New-Object System.Collections.Generic.List[object]
function Check([string]$name, [scriptblock]$body) {
    try { $r = & $body; if ($r -eq $false) { throw 'returned false' }
        $results.Add([pscustomobject]@{ Test = $name; Ok = $true; Note = "$r" }) }
    catch { $results.Add([pscustomobject]@{ Test = $name; Ok = $false; Note = $_.Exception.Message }) }
}
function WaitFor([scriptblock]$cond, [int]$sec = 8) {
    $deadline = (Get-Date).AddSeconds($sec)
    while (-not (& $cond)) { if ((Get-Date) -gt $deadline) { return $false }; Start-Sleep -Milliseconds 150 }
    $true
}
function LogText { if (Test-Path $log) { Get-Content $log -Raw } else { '' } }
function ClearLog { Remove-Item $log -ErrorAction SilentlyContinue }
function ExplorerWindows([string]$path) {
    $shell = New-Object -ComObject Shell.Application
    @($shell.Windows() | Where-Object { try { $_.Document.Folder.Self.Path -eq $path } catch { $false } })
}
function CloseWindows([string]$prefix) {
    $shell = New-Object -ComObject Shell.Application
    @($shell.Windows() | Where-Object { try { $_.Document.Folder.Self.Path -like "$prefix*" } catch { $false } }) | ForEach-Object { $_.Quit() }
}

Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class P2 {
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
  public struct SEI { public int cbSize; public uint fMask; public IntPtr hwnd; public string lpVerb; public string lpFile; public string lpParameters;
    public string lpDirectory; public int nShow; public IntPtr hInstApp; public IntPtr lpIDList; public string lpClass; public IntPtr hkeyClass;
    public uint dwHotKey; public IntPtr hIcon; public IntPtr hProcess; }
  [DllImport("shell32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern bool ShellExecuteExW(ref SEI e);
  // Действие по умолчанию, как двойной клик на рабочем столе
  public static bool OpenDefault(string path) { var e = new SEI(); e.cbSize = Marshal.SizeOf(e); e.lpFile = path; e.nShow = 1; e.fMask = 0x00000100; return ShellExecuteExW(ref e); }
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
}
'@

# Подготовка
CloseWindows $root
Remove-Item $root -Recurse -Force -ErrorAction SilentlyContinue
$locked = Join-Path $root 'parent\Locked'; $normal = Join-Path $root 'parent\Normal'
New-Item -ItemType Directory -Force $locked, $normal | Out-Null
Set-Content (Join-Path $locked 'inside.txt') 'x'

# Регистрация только для текущего пользователя
$k = "HKCU:\Software\Classes\CLSID\$clsid"
New-Item -Force "$k\InprocServer32" | Out-Null
Set-ItemProperty $k '(default)' 'Zamok P2 prototype'
Set-ItemProperty "$k\InprocServer32" '(default)' $dll
Set-ItemProperty "$k\InprocServer32" 'ThreadingModel' 'Apartment'
New-Item -Force 'HKCU:\Software\Classes\Directory\shellex\ContextMenuHandlers\ZamokP2' | Out-Null
Set-ItemProperty 'HKCU:\Software\Classes\Directory\shellex\ContextMenuHandlers\ZamokP2' '(default)' $clsid
New-Item -Force 'HKCU:\Software\ZamokP2' | Out-Null
Set-ItemProperty 'HKCU:\Software\ZamokP2' 'Locked' $locked
Set-ItemProperty 'HKCU:\Software\ZamokP2' 'Log' $log

try {
    $shell = New-Object -ComObject Shell.Application
    $parent = Join-Path $root 'parent'

    # 1. Меню, собранное через Shell API в этом процессе: какое действие по умолчанию
    foreach ($case in @(@{ Name = 'Locked'; Expect = $true }, @{ Name = 'Normal'; Expect = $false })) {
        ClearLog
        $item = $shell.Namespace($parent).ParseName($case.Name)
        $verbs = @($item.Verbs() | ForEach-Object { $_.Name })
        Check "1. Context menu of $($case.Name): first (default) verb" {
            $first = $verbs[0]; $ours = $first -like '*Zamok P2*'
            if ($ours -ne $case.Expect) { throw "first=[$first] all=[$($verbs -join ', ')] log=[$((LogText) -replace "`r?`n", ' / ')]" }
            "first=[$first]"
        }
    }
    # 2. Действие по умолчанию у папки (FolderItem.InvokeVerb без имени)
    ClearLog
    $shell.Namespace($parent).ParseName('Locked').InvokeVerb()
    Check '2. Default verb of LOCKED folder runs our command' {
        $ok = WaitFor { (LogText) -match 'invoke' }; $t = (LogText) -replace "`r?`n", ' / '
        if (-not $ok) { throw "no invoke; log: $t" }; $t
    }
    Start-Sleep 1; CloseWindows $root

    # 3. Настоящее окно Проводника: UI Automation «вызывает» элемент списка, как двойной клик
    Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
    Start-Process explorer.exe $parent
    $null = WaitFor { (ExplorerWindows $parent).Count -gt 0 } 10
    $w = ExplorerWindows $parent | Select-Object -First 1
    foreach ($case in @(@{ Name = 'Locked'; Path = $locked; Expect = 'invoke' }, @{ Name = 'Normal'; Path = $normal; Expect = 'none' })) {
        ClearLog
        $w.Navigate($parent); Start-Sleep -Milliseconds 1500
        $ae = [Windows.Automation.AutomationElement]::FromHandle([IntPtr]$w.HWND)
        $cond = New-Object Windows.Automation.AndCondition(
            (New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::ControlTypeProperty, [Windows.Automation.ControlType]::ListItem)),
            (New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::NameProperty, $case.Name)))
        $li = $ae.FindFirst([Windows.Automation.TreeScope]::Descendants, $cond)
        $how = 'none'
        if ($li) {
            $ip = $null
            if ($li.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern, [ref]$ip)) { $ip.Invoke(); $how = 'InvokePattern' }
        }
        $moved = WaitFor { try { $w.Document.Folder.Self.Path -eq $case.Path } catch { $false } } 6
        $inv = (LogText) -match 'invoke'
        $t = (LogText) -replace "`r?`n", ' / '
        Check "3. Explorer window, double-click equivalent on $($case.Name)" {
            if ($how -eq 'none') { throw 'list item not found or not invokable' }
            if ($case.Expect -eq 'invoke' -and -not $inv) { throw "no invoke; navigated=$moved; log: $t" }
            if ($case.Expect -eq 'none' -and $inv) { throw "unexpected invoke; log: $t" }
            "via $how; same window navigated into folder=$moved; log: $t"
        }
    }
    CloseWindows $root

    # 3. Меню правой кнопки собирается без задержки (скорость обработчика)
    Check '4. QueryContextMenu calls logged with flags' { $t = (LogText); ($t -split "`n" | Where-Object { $_ -match 'query' } | Select-Object -First 3) -join ' / ' }
}
finally {
    CloseWindows $root
    Remove-Item 'HKCU:\Software\Classes\Directory\shellex\ContextMenuHandlers\ZamokP2' -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item "HKCU:\Software\Classes\CLSID\$clsid" -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item 'HKCU:\Software\ZamokP2' -Recurse -Force -ErrorAction SilentlyContinue
}

$results | Format-Table -AutoSize -Wrap | Out-String -Width 220
"passed: $(@($results | Where-Object Ok).Count) / $($results.Count)"
"Registry cleaned. The DLL stays loaded in explorer.exe until Explorer restarts."
