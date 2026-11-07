# P3: запирание без отключения. Окно Проводника открыто на папке, другой процесс держит файл.
# Запуск: powershell -NoProfile -ExecutionPolicy Bypass -File test-p3.ps1
$ErrorActionPreference = 'Stop'
$root = 'C:\dev\zamok\tests\tmp\p3'
$src = Join-Path $root 'src'
$mnt = Join-Path $root 'mnt'
$ctl = Join-Path $root 'control.txt'
$exe = Join-Path $PSScriptRoot 'gatefs\target\release\p3-gatefs.exe'
$results = New-Object System.Collections.Generic.List[object]
function Check([string]$name, [scriptblock]$body) {
    try { $r = & $body; if ($r -eq $false) { throw 'returned false' }
        $results.Add([pscustomobject]@{ Test = $name; Ok = $true; Note = "$r" }) }
    catch { $results.Add([pscustomobject]@{ Test = $name; Ok = $false; Note = $_.Exception.Message }) }
}
function WaitFor([scriptblock]$cond, [int]$sec = 10) {
    $d = (Get-Date).AddSeconds($sec)
    while (-not (& $cond)) { if ((Get-Date) -gt $d) { throw 'timeout' }; Start-Sleep -Milliseconds 100 }
}
function SetState([string]$s) {
    Remove-Item "$root\control.ack.txt" -ErrorAction SilentlyContinue
    Set-Content $ctl $s
    WaitFor { (Test-Path "$root\control.ack.txt") -and ((Get-Content "$root\control.ack.txt") -eq $s) }
    Start-Sleep -Milliseconds 300
}
function StartFs {
    $env:P3_CONTROL = $ctl; $env:P3_MOUNT = $mnt
    $p = Start-Process $exe -ArgumentList "-p `"$src`" -m `"$mnt`"" -PassThru -WindowStyle Hidden
    WaitFor { Test-Path (Join-Path $mnt 'a.txt') }
    $p
}

# Подготовка
Remove-Item $root -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force (Join-Path $src 'docs') | Out-Null
Set-Content (Join-Path $src 'a.txt') 'secret A'
Set-Content (Join-Path $src 'docs\b.txt') 'secret B'
Set-Content $ctl 'unlocked'
$fs = StartFs
$m = Get-Item $mnt -Force; $m.Attributes = $m.Attributes -bor [IO.FileAttributes]::System

# Окно Проводника на папке
$shell = New-Object -ComObject Shell.Application
Start-Process explorer.exe $mnt
$win = $null
WaitFor { $script:win = @($shell.Windows() | Where-Object { try { $_.Document.Folder.Self.Path -eq $mnt } catch { $false } })[0]; $script:win -ne $null } 15
Check 'Explorer window opened on the mounted folder' { "items=$($win.Document.Folder.Items().Count)" }

# Процесс, который держит файл открытым (как Word)
$helper = @"
`$f = [IO.File]::Open('$mnt\a.txt', 'Open', 'ReadWrite', 'ReadWrite, Delete')
`$b = New-Object byte[] 8; `$f.Read(`$b, 0, 8) | Out-Null
Set-Content '$root\h-ready.txt' ([Text.Encoding]::ASCII.GetString(`$b))
while (-not (Test-Path '$root\h-go1.txt')) { Start-Sleep -Milliseconds 100 }
try { `$f.Seek(0, 'Begin') | Out-Null; `$n = `$f.Read(`$b, 0, 8); `$r = 'read ok: ' + [Text.Encoding]::ASCII.GetString(`$b, 0, `$n) } catch { `$r = 'read failed: ' + `$_.Exception.InnerException.Message }
try { `$f.Seek(0, 'Begin') | Out-Null; `$f.Write([byte[]](88,88), 0, 2); `$f.Flush(); `$r += ' | write ok' } catch { `$r += ' | write failed: ' + `$_.Exception.InnerException.Message }
Set-Content '$root\h-locked.txt' `$r
while (-not (Test-Path '$root\h-go2.txt')) { Start-Sleep -Milliseconds 100 }
try { `$f.Seek(0, 'Begin') | Out-Null; `$n = `$f.Read(`$b, 0, 8); `$r = 'read ok: ' + [Text.Encoding]::ASCII.GetString(`$b, 0, `$n) } catch { `$r = 'read failed: ' + `$_.Exception.InnerException.Message }
Set-Content '$root\h-unlocked.txt' `$r
try { `$f.Close() } catch {}
"@
$hp = Start-Process powershell -ArgumentList '-NoProfile', '-Command', $helper -PassThru -WindowStyle Hidden
WaitFor { Test-Path "$root\h-ready.txt" }
Check 'helper process read the file before lock' { Get-Content "$root\h-ready.txt" }

# Запираем
$sw = [Diagnostics.Stopwatch]::StartNew(); SetState 'locked'; $sw.Stop()
Check 'lock took (ms)' { $sw.ElapsedMilliseconds }
Check 'mount point still exists, filesystem process alive' { (Test-Path $mnt) -and -not $fs.HasExited }
Check 'root listing is empty when locked' { $c = @(Get-ChildItem $mnt -Force).Count; if ($c -ne 0) { return "items=$c" -eq '' }; 'items=0' }
function NotFound { $a = Test-Path (Join-Path $mnt 'a.txt'); $b = Test-Path (Join-Path $mnt 'docs\b.txt')
    if ($a -or $b) { throw "still visible: a.txt(open by helper)=$a docs\b.txt=$b" }; 'not found' }
Check 'files are not found when locked (right after lock)' { NotFound }
Start-Sleep -Milliseconds 1500
Check 'files are not found when locked (after 1.5 s, past file info cache)' { NotFound }
Check 'cannot create files when locked' { try { Set-Content (Join-Path $mnt 'new.txt') 'x'; $false } catch { 'denied' } }
Check 'open-files report names the process holding the file' {
    $r = Get-Content "$root\control.open.txt" -Raw
    if ($r -notmatch 'powershell\.exe.*\\a\.txt') { return "report=[$r]" -eq '' }
    ($r -split "`r`n" | Where-Object { $_ -match 'a\.txt' }) -join '; '
}
Check 'Explorer window survives lock and shows empty folder after refresh' {
    $win.Refresh(); Start-Sleep -Milliseconds 2000
    $n = $win.Document.Folder.Items().Count
    if ($n -ne 0) { return "items=$n" -eq '' }
    'items=0'
}
New-Item "$root\h-go1.txt" -ItemType File | Out-Null
WaitFor { Test-Path "$root\h-locked.txt" }
Check 'handle opened before lock: read and write after lock (must fail)' {
    $r = Get-Content "$root\h-locked.txt"
    if ($r -match 'read ok') { return "LEAK: $r" -eq '' }
    $r
}

# Открываем
SetState 'unlocked'
Check 'files visible again after unlock' { (Get-Content (Join-Path $mnt 'a.txt')) -eq 'secret A' }
Check 'Explorer window shows files again after refresh' {
    $win.Refresh(); Start-Sleep -Milliseconds 2000
    $n = $win.Document.Folder.Items().Count; if ($n -lt 2) { return "items=$n" -eq '' }; "items=$n"
}
New-Item "$root\h-go2.txt" -ItemType File | Out-Null
WaitFor { Test-Path "$root\h-unlocked.txt" }
Check 'handle from before lock stays dead after unlock' {
    $r = Get-Content "$root\h-unlocked.txt"; if ($r -match 'read ok') { return "still alive: $r" -eq '' }; $r
}
Check 'source file not modified by write attempt during lock' {
    $f = [IO.File]::Open((Join-Path $src 'a.txt'), 'Open', 'Read', 'ReadWrite, Delete')
    $t = (New-Object IO.StreamReader($f)).ReadToEnd().Trim(); $f.Close(); $t -eq 'secret A'
}

# Повторное подключение: сохраняется ли атрибут System на точке подключения
try { $win.Quit() } catch {}
Stop-Process $fs.Id; WaitFor { -not (Test-Path $mnt) }
$fs = StartFs
Check 'System attribute on mount point survives remount' {
    $a = (Get-Item $mnt -Force).Attributes
    "attrs=[$a] system=$([bool]($a -band [IO.FileAttributes]::System))"
}
Stop-Process $fs.Id -ErrorAction SilentlyContinue
Stop-Process $hp.Id -ErrorAction SilentlyContinue

$results | Format-Table -AutoSize -Wrap | Out-String -Width 220
"passed: $(@($results | Where-Object Ok).Count) / $($results.Count)"
