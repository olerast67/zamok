# P2, способ A2: статическое действие папки в реестре (HKCU) с условием AppliesTo,
# которое делает его действием по умолчанию только у запертой папки.
# Запуск: powershell -NoProfile -ExecutionPolicy Bypass -File test-p2-static.ps1
$ErrorActionPreference = 'Stop'
$root = 'C:\dev\zamok\tests\tmp\p2'
$parent = Join-Path $root 'parent'
$locked = Join-Path $parent 'Locked'; $normal = Join-Path $parent 'Normal'
$log = Join-Path $root 'static-log.txt'
New-Item -ItemType Directory -Force $locked, $normal | Out-Null
Remove-Item $log -ErrorAction SilentlyContinue

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$shell = New-Object -ComObject Shell.Application
function WaitFor([scriptblock]$cond, [int]$sec = 8) {
    $deadline = (Get-Date).AddSeconds($sec)
    while (-not (& $cond)) { if ((Get-Date) -gt $deadline) { return $false }; Start-Sleep -Milliseconds 150 }
    $true
}
function Win([string]$path) { @($shell.Windows() | Where-Object { try { $_.Document.Folder.Self.Path -eq $path } catch { $false } }) | Select-Object -First 1 }
function LogText { if (Test-Path $log) { (Get-Content $log -Raw).Trim() -replace "`r?`n", ' / ' } else { '' } }

$shellKey = 'HKCU:\Software\Classes\Directory\shell'
$hadDefault = $false; $oldDefault = $null
if (Test-Path $shellKey) { $v = (Get-ItemProperty $shellKey -ErrorAction SilentlyContinue).'(default)'; if ($v) { $hadDefault = $true; $oldDefault = $v } }
$verb = Join-Path $shellKey 'ZamokP2Static'
$out = @()
try {
    if ($env:P2_BASELINE -ne '1') {
    New-Item -Force "$verb\command" | Out-Null
    Set-ItemProperty $verb '(default)' 'Open with password (Zamok P2 static)'
    # Условие: только папка с этим путём
    Set-ItemProperty $verb 'AppliesTo' ("System.ItemPathDisplay:=""$locked""")
    Set-ItemProperty "$verb\command" '(default)' ("cmd.exe /c echo invoke %1>>""$log""")
    Set-ItemProperty $shellKey '(default)' 'ZamokP2Static open'
    }

    foreach ($name in 'Locked', 'Normal') {
        $item = $shell.Namespace($parent).ParseName($name)
        $first = @($item.Verbs() | ForEach-Object { $_.Name })[0]
        $out += "${name}: first verb in menu = [$first]"
    }

    Start-Process explorer.exe $parent
    $null = WaitFor { (Win $parent) -ne $null } 10
    $w = Win $parent
    foreach ($case in @(@{ Name = 'Normal'; Path = $normal }, @{ Name = 'Locked'; Path = $locked }, @{ Name = 'Normal'; Path = $normal })) {
        Remove-Item $log -ErrorAction SilentlyContinue
        @($shell.Windows() | Where-Object { $_.LocationURL -like '*/p2/parent/*' }) | ForEach-Object { $_.Quit() }
        $w.Navigate($parent); Start-Sleep -Milliseconds 1500
        $ae = [Windows.Automation.AutomationElement]::FromHandle([IntPtr]$w.HWND)
        $cond = New-Object Windows.Automation.AndCondition(
            (New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::ControlTypeProperty, [Windows.Automation.ControlType]::ListItem)),
            (New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::NameProperty, $case.Name)))
        $li = $ae.FindFirst([Windows.Automation.TreeScope]::Descendants, $cond)
        $ip = $null
        if ($li -and $li.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern, [ref]$ip)) { $ip.Invoke() }
        $want = ([Uri]$case.Path).AbsoluteUri
        $moved = WaitFor { @($shell.Windows() | Where-Object { $_.LocationURL -eq $want }).Count -gt 0 } 5
        Start-Sleep -Milliseconds 800
        $out += "$($case.Name): Explorer double-click equivalent -> navigated into folder=$moved; command log=[$(LogText)]"
    }
    try { $w.Quit() } catch {}
}
finally {
    if ($hadDefault) { Set-ItemProperty $shellKey '(default)' $oldDefault } else { Remove-ItemProperty $shellKey '(default)' -ErrorAction SilentlyContinue; Clear-ItemProperty $shellKey -Name '(default)' -ErrorAction SilentlyContinue }
    Remove-Item $verb -Recurse -Force -ErrorAction SilentlyContinue
    $left = (Get-ItemProperty $shellKey -ErrorAction SilentlyContinue).'(default)'
    $out += "cleanup: HKCU Directory\shell default now=[$left], verb removed=$(-not (Test-Path $verb))"
}
$out
