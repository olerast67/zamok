# Proverka zamok-shell.dll v nastoyashchey ustanovke: perenos roditelskoy papki s papkoy Zamok na
# drugoy disk otmenyaetsya, vnutri diska perenos rabotaet. Nuzhen ustanovlennyy Zamok (scripts\install-dev.ps1) i vtoroy disk NTFS D:.
# Papka sozdaetsya vremennaya i ubiraetsya iz spiska v kontse; chuzhie papki ne trogaet.
# Zapusk: powershell -NoProfile -Sta -ExecutionPolicy Bypass -File tests\install\test-copyhook.ps1
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$repo = (Resolve-Path "$PSScriptRoot/../..").Path
$root = "$repo\tests\tmp\copyhook"
$pw = 'copy-hook-test'
$pipe = "zamok-$([Security.Principal.WindowsIdentity]::GetCurrent().User.Value)"
$parent = "$root\parent"; $mnt = "$parent\folder"; $vault = "$parent\.folder.zamok"
$other = "$root\other"; $mntB = "$other\folder"
$dest = "D:\zamok-hook-$PID"
$results = New-Object System.Collections.Generic.List[object]
function Check([string]$name, [scriptblock]$body) {
    try { $r = & $body; if ($r -eq $false) { throw 'returned false' }
        $results.Add([pscustomobject]@{ Test = $name; Ok = $true; Note = "$r" }) }
    catch { $results.Add([pscustomobject]@{ Test = $name; Ok = $false; Note = $_.Exception.Message }) }
}
function WaitFor([scriptblock]$c, [int]$ms = 10000) { $d = (Get-Date).AddMilliseconds($ms); while (-not (& $c)) { if ((Get-Date) -gt $d) { return $false }; Start-Sleep -Milliseconds 100 }; $true }
function Send([string]$j) { $c = New-Object IO.Pipes.NamedPipeClientStream '.', $pipe, 'InOut'; $c.Connect(5000); $e = New-Object Text.UTF8Encoding $false; $w = New-Object IO.StreamWriter $c, $e; $w.AutoFlush = $true; $r = New-Object IO.StreamReader $c, $e; $w.WriteLine($j); $l = $r.ReadLine(); $c.Dispose(); $l | ConvertFrom-Json }
function J([string]$s) { $s.Replace('\', '\\') }
function Cipher([string]$v) { @(Get-ChildItem -LiteralPath "$v\d" -Recurse -File -Force -ErrorAction SilentlyContinue).Count }
# Tem zhe mehanizmom, chto Provodnik (bez okon: FOF_SILENT, NOCONFIRMATION, NOCONFIRMMKDIR, NOERRORUI)
function ShellMove([string]$src, [string]$to) {
    New-Item -ItemType Directory -Force $to | Out-Null
    (New-Object -ComObject Shell.Application).Namespace($to).MoveHere($src, 4 + 16 + 512 + 1024)
    Start-Sleep -Seconds 6
}
function DestFiles { @(Get-ChildItem -LiteralPath $dest -Recurse -File -Force -ErrorAction SilentlyContinue).Count }

if (-not (Test-Path "\\.\pipe\$pipe")) { throw 'Zamok is not running: scripts\install-dev.ps1' }
Remove-Item $root -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $parent, $other | Out-Null
Push-Location "$repo\core"; & "$env:USERPROFILE\.cargo\bin\cargo.exe" run -q --release --example create_vault -- "$repo\tests\fixtures-src\vault1" $vault $pw | Out-Null; Pop-Location
$start = Cipher $vault
try {
    $r = Send "{`"cmd`":`"add`",`"path`":`"$(J $mnt)`",`"vault`":`"$(J $vault)`"}"
    Check 'test folder is added' { $r.ok }
    Check 'extension is registered' {
        $dll = (Get-ItemProperty 'HKCU:\Software\Classes\CLSID\{3F7A9C12-8E4B-4D61-A5C7-19E2B60D4F83}\InprocServer32').'(default)'
        (WaitFor { Test-Path -LiteralPath $dll } 5000) -and $dll -like '*\Zamok\shell\zamok-shell-*.dll' -and
            (Get-ItemProperty 'HKCU:\Software\Classes\Directory\shellex\CopyHookHandlers\Zamok').'(default)' -like '{3F7A9C12*'
    }
    Start-Sleep -Seconds 4   # core publikuet spisok po tayjmeru
    $u = Send "{`"cmd`":`"unlock`",`"path`":`"$(J $mnt)`",`"password`":`"$pw`"}"
    Check 'folder is open' { $u.ok -and @(Get-ChildItem -LiteralPath $mnt -Recurse -File -Force).Count -gt 0 }

    # Pryamoy perenos samoy papki Windows ne daet perehvatit: dlya tochki podklyucheniya
    # CopyHook ne vyzyvaetsya (DESIGN.md, razdel 11). Proveryaem roditelskuyu papku.
    ShellMove $parent "$dest\parent"
    Check 'parent of an open folder to another drive: refused, nothing copied' {
        (DestFiles) -eq 0 -and (Cipher $vault) -eq $start -and @(Get-ChildItem -LiteralPath $mnt -File -Force).Count -gt 0
    }
    Check 'the extension was really loaded' { @((Get-Process -Id $PID).Modules | Where-Object { $_.ModuleName -like 'zamok-shell-*' }).Count -eq 1 }
    $plain = "$root\plain"; New-Item -ItemType Directory -Force $plain | Out-Null; Set-Content "$plain\a.txt" 'x'
    ShellMove $plain "$dest\plain"
    Check 'ordinary folder to another drive still moves' { (Test-Path "$dest\plain\plain\a.txt") -and -not (Test-Path $plain) }

    [void](Send "{`"cmd`":`"lock`",`"path`":`"$(J $mnt)`"}")
    # Dlya Provodnika papka Zamok - drugoy tom: on kopiruet soderzhimoe, a ne pereimenovyvaet.
    # U zapertoy papki kopirovat nechego, sama ona ostaetsya na meste.
    ShellMove $mnt $other
    Check 'locked folder moved by Explorer: stays in place, vault intact' {
        (WaitFor { Test-Path -LiteralPath $mnt } 10000) -and (Cipher $vault) -eq $start -and
            (Test-Path -LiteralPath "$vault\masterkey.cryptomator")
    }
} finally {
    foreach ($p in @($mnt, $mntB)) { try { [void](Send "{`"cmd`":`"remove`",`"path`":`"$(J $p)`"}") } catch {} }
    Remove-Item -LiteralPath $dest -Recurse -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 1
    Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
}
$results | Format-Table -AutoSize -Wrap | Out-String -Width 200
"passed: $(@($results | Where-Object Ok).Count) / $($results.Count)"
