# Proverka ustanovshchika MSI na nastoyashchey sisteme: tihaya ustanovka i udalenie.
# Rabotaet s NASTOYASHCHIM HKCU i %LOCALAPPDATA%\Programs\Zamok, zapuskaetsya tolko esli Zamok
# ne ustanovlen. Spisok papok %LOCALAPPDATA%\Zamok ne trogaet.
# Snachala: installer\build-msi.ps1
# Zapusk: powershell -NoProfile -ExecutionPolicy Bypass -File tests\install\test-msi.ps1
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$repo = (Resolve-Path "$PSScriptRoot/../..").Path
$msi = Get-ChildItem "$repo\installer\out" -Filter 'Zamok-*.msi' | Sort-Object LastWriteTime | Select-Object -Last 1
$dir = "$env:LOCALAPPDATA\Programs\Zamok"
$cls = 'HKCU:\Software\Classes'
$run = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$pipe = "zamok-$([Security.Principal.WindowsIdentity]::GetCurrent().User.Value)"
$results = New-Object System.Collections.Generic.List[object]
function Check([string]$name, [scriptblock]$body) {
    try { $r = & $body; if ($r -eq $false) { throw 'returned false' }
        $results.Add([pscustomobject]@{ Test = $name; Ok = $true; Note = "$r" }) }
    catch { $results.Add([pscustomobject]@{ Test = $name; Ok = $false; Note = $_.Exception.Message }) }
}
function WaitFor([scriptblock]$cond, [int]$ms = 10000) {
    $deadline = (Get-Date).AddMilliseconds($ms)
    while (-not (& $cond)) { if ((Get-Date) -gt $deadline) { return $false }; Start-Sleep -Milliseconds 200 }
    $true
}
function Send([string]$json) {
    $c = New-Object IO.Pipes.NamedPipeClientStream '.', $pipe, 'InOut'; $c.Connect(5000)
    $w = New-Object IO.StreamWriter $c; $w.AutoFlush = $true; $w.WriteLine($json)
    $r = New-Object IO.StreamReader $c; $line = $r.ReadLine(); $c.Dispose(); $line | ConvertFrom-Json
}

if (-not $msi) { throw 'build the MSI first: installer\build-msi.ps1' }
if ((Test-Path $dir) -or (Test-Path "$cls\.zamok") -or (Test-Path "\\.\pipe\$pipe")) { throw 'Zamok is installed or running: uninstall it first' }

$log = "$repo\tests\tmp\msi-install.log"
New-Item -ItemType Directory -Force (Split-Path $log) | Out-Null
$p = Start-Process msiexec -ArgumentList '/i', "`"$($msi.FullName)`"", '/qn', '/l*v', "`"$log`"" -PassThru -Wait
Check 'msi installs quietly without admin rights' { $p.ExitCode -eq 0 -or "exit $($p.ExitCode), see $log" -eq '' }
Check 'files are in Programs\Zamok' { (Test-Path "$dir\Zamok.exe") -and (Test-Path "$dir\zamok-core.exe") }
Check 'icons were built from this Windows' { (Test-Path "$dir\folder-locked.ico") -and (Test-Path "$dir\folder-open.ico") -and (Test-Path "$dir\open-item.ico") }
Check 'core started through the watchdog' { WaitFor { Test-Path "\\.\pipe\$pipe" } 20000 }
Check 'autostart entry' { (Get-ItemProperty $run).Zamok -eq "`"$dir\zamok-core.exe`" --watch" }
Check 'menu items in the Windows display language' {
    Add-Type -Namespace ZamokMsiTest -Name Lang -MemberDefinition '[DllImport("kernel32.dll")] public static extern ushort GetUserDefaultUILanguage();'
    $ru = ([ZamokMsiTest.Lang]::GetUserDefaultUILanguage() -band 0x3FF) -eq 0x19
    $t = (Get-ItemProperty "$cls\Directory\shell\ZamokUnlock").MUIVerb
    if ($ru) { $t -ne 'Open with password' } else { $t -eq 'Open with password' }
}
Check '.zamok opens Zamok.exe --unlock' {
    (Get-ItemProperty "$cls\Zamok.OpenItem\shell\open\command").'(default)' -eq "`"$dir\Zamok.exe`" --unlock `"%1`""
}
Check 'core answers and keeps the menu conditions' {
    $s = Send '{"cmd":"status"}'
    $at = (Get-ItemProperty "$cls\Directory\shell\ZamokMore").AppliesTo
    $s.ok -and ($s.folders.Count -eq 0 -or (WaitFor { (Get-ItemProperty "$cls\Directory\shell\ZamokMore").AppliesTo -notlike '*zamok-none*' } 8000))
}

$p = Start-Process msiexec -ArgumentList '/x', "`"$($msi.FullName)`"", '/qn', '/l*v', "`"$repo\tests\tmp\msi-uninstall.log`"" -PassThru -Wait
Check 'msi uninstalls quietly' { $p.ExitCode -eq 0 }
Check 'uninstall: core stopped, files, icons and registry entries gone' {
    -not (Test-Path "\\.\pipe\$pipe") -and -not (Test-Path $dir) -and -not (Test-Path "$cls\.zamok") -and
        -not (Test-Path "$cls\Zamok.OpenItem") -and -not (Test-Path "$cls\Directory\shell\ZamokEncrypt") -and
        -not (Test-Path "$cls\Directory\shell\ZamokMore") -and -not (Get-ItemProperty $run).Zamok
}
Check 'uninstall keeps the folder list' { -not (Test-Path "$env:LOCALAPPDATA\Zamok") -or (Test-Path "$env:LOCALAPPDATA\Zamok\core.log") }

$results | Format-Table -AutoSize -Wrap | Out-String -Width 200
"passed: $(@($results | Where-Object Ok).Count) / $($results.Count)"
