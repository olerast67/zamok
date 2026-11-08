# Proverka obshchego ustanovshchika Zamok-Setup-*.exe na nastoyashchey sisteme, gde WinFsp uzhe stoit:
# tihaya ustanovka i udalenie bez prav administratora, WinFsp ne trogaetsya.
# Rabotaet s NASTOYASHCHIM HKCU, zapuskaetsya tolko esli Zamok ne ustanovlen.
# Stsenariy "WinFsp eshche net" zdes ne proveryaetsya: dlya nego nuzhna chistaya sistema.
# Snachala: installer\build-msi.ps1
# Zapusk: powershell -NoProfile -ExecutionPolicy Bypass -File tests\install\test-bundle.ps1
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$repo = (Resolve-Path "$PSScriptRoot/../..").Path
$exe = Get-ChildItem "$repo\installer\out" -Filter 'Zamok-Setup-*.exe' | Sort-Object LastWriteTime | Select-Object -Last 1
$dir = "$env:LOCALAPPDATA\Programs\Zamok"
$cls = 'HKCU:\Software\Classes'
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
function Run([string[]]$a) {
    $p = Start-Process $exe.FullName -ArgumentList $a -PassThru
    if (-not $p.WaitForExit(300000)) { $p.Kill(); return -1 }
    $p.ExitCode
}
function WinFspDir { (Get-ItemProperty 'HKLM:\SOFTWARE\WOW6432Node\WinFsp' -ErrorAction SilentlyContinue).InstallDir }

if (-not $exe) { throw 'build first: installer\build-msi.ps1' }
if ((Test-Path $dir) -or (Test-Path "$cls\.zamok") -or (Test-Path "\\.\pipe\$pipe")) { throw 'Zamok is installed or running: uninstall it first' }
if (-not (WinFspDir)) { throw 'this test expects WinFsp to be installed' }

$tmp = "$repo\tests\tmp"
New-Item -ItemType Directory -Force $tmp | Out-Null
$log = "$tmp\bundle-install.log"
$before = (Get-Item "$(WinFspDir)bin\winfsp-x64.dll").LastWriteTimeUtc

$code = Run @('/quiet', '/norestart', '/log', "`"$log`"")
Check 'bundle installs quietly' { $code -eq 0 -or "exit $code, see $log" -eq '' }
Check 'zamok files are in place' { (Test-Path "$dir\Zamok.exe") -and (Test-Path "$dir\zamok-core.exe") }
Check 'core started' { WaitFor { Test-Path "\\.\pipe\$pipe" } 20000 }
$text = Get-Content $log -Raw
Check 'winfsp package is skipped when already present' { $text -match 'Planned package: WinFsp, .*execute: None' }
Check 'no elevation was needed' { $text -notmatch 'elevated engine' }
Check 'bundle is listed as per-user' { (Get-ChildItem 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall' | Where-Object { (Get-ItemProperty $_.PSPath).BundleProviderKey }).Count -ge 1 }

$code = Run @('/uninstall', '/quiet', '/norestart', '/log', "`"$tmp\bundle-uninstall.log`"")
Check 'bundle uninstalls quietly' { $code -eq 0 }
Check 'uninstall: core stopped, files and registry gone' {
    (WaitFor { -not (Test-Path "\\.\pipe\$pipe") } 10000) -and -not (Test-Path $dir) -and -not (Test-Path "$cls\.zamok")
}
Check 'winfsp stays after uninstall' { (WinFspDir) -and (Get-Item "$(WinFspDir)bin\winfsp-x64.dll").LastWriteTimeUtc -eq $before }

$results | Format-Table -AutoSize -Wrap | Out-String -Width 200
"passed: $(@($results | Where-Object Ok).Count) / $($results.Count)"
