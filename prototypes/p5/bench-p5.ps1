# P5: скорость WinFsp без шифрования против обычного NTFS.
# Запуск: powershell -NoProfile -ExecutionPolicy Bypass -File bench-p5.ps1
# Варианты: NTFS напрямую и файловая система из P3 (passthrough + состояние) с разным FileInfoTimeout.
param([int]$Files = 20000, [int]$BigMB = 1024)
$ErrorActionPreference = 'Stop'
$root = 'C:\dev\zamok\tests\tmp\p5'
$exe = 'C:\dev\zamok\prototypes\p3\gatefs\target\release\p3-gatefs.exe'
$git = 'C:\Program Files\Git\cmd\git.exe'

function T([scriptblock]$b) { $sw = [Diagnostics.Stopwatch]::StartNew(); & $b | Out-Null; $sw.Stop(); [math]::Round($sw.Elapsed.TotalSeconds, 2) }
function WaitFor([scriptblock]$cond, [int]$sec = 10) {
    $deadline = (Get-Date).AddSeconds($sec)
    while (-not (& $cond)) { if ((Get-Date) -gt $deadline) { throw 'timeout' }; Start-Sleep -Milliseconds 100 }
}

# Исходные данные один раз: дерево мелких файлов и большой файл
$tree = Join-Path $root 'seed\tree'
$big = Join-Path $root 'seed\big.bin'
if (-not (Test-Path $tree)) {
    $rng = New-Object Random 42
    $buf = New-Object byte[] 8192
    for ($i = 0; $i -lt $Files; $i++) {
        $d = Join-Path $tree ("pkg{0:D3}\lib{1:D2}" -f ($i % 250), ([int][math]::Floor($i / 250) % 8))
        if (-not (Test-Path $d)) { New-Item -ItemType Directory -Force $d | Out-Null }
        $rng.NextBytes($buf)
        $fs = [IO.File]::Create((Join-Path $d "file$i.js")); $fs.Write($buf, 0, 1024 + $rng.Next(7168)); $fs.Close()
    }
}
if (-not (Test-Path $big)) {
    $buf = New-Object byte[] (1MB); $rng = New-Object Random 7
    $fs = [IO.File]::Create($big); 1..$BigMB | ForEach-Object { $rng.NextBytes($buf); $fs.Write($buf, 0, $buf.Length) }; $fs.Close()
}

function Bench([string]$name, [string]$work) {
    $r = [ordered]@{ Variant = $name }
    $dst = Join-Path $work 'tree'
    $r['copy tree in (robocopy)'] = T { robocopy $tree $dst /E /NFL /NDL /NJH /NJS /NP /MT:8 }
    $r['list tree (dir /s /b)'] = T { cmd /c "dir /s /b `"$dst`" > nul" }
    $r['git add + commit'] = T { & $git -C $dst init -q; & $git -C $dst add -A 2>$null; & $git -C $dst -c user.email=b@b -c user.name=b commit -q -m x }
    $r['git status (warm)'] = T { & $git -C $dst status --porcelain }
    $r['copy 1 GB file in'] = T { robocopy (Split-Path $big) $work big.bin /NFL /NDL /NJH /NJS /NP /J }
    $r['read 1 GB file'] = T { cmd /c "copy /b `"$work\big.bin`" nul > nul" }
    $r['delete tree'] = T { cmd /c "rmdir /s /q `"$dst`"" }
    Remove-Item (Join-Path $work 'big.bin') -Force
    [pscustomobject]$r
}

$all = New-Object System.Collections.Generic.List[object]

# 1. NTFS
$w = Join-Path $root 'ntfs'; if (Test-Path $w) { cmd /c "rmdir /s /q `"$w`"" }; New-Item -ItemType Directory $w | Out-Null
$all.Add((Bench 'NTFS' $w))

# 2–4. WinFsp passthrough с разным кэшем
foreach ($fit in 0, 1000, 4294967295) {
    $back = Join-Path $root "back-$fit"; $mnt = Join-Path $root "mnt-$fit"
    if (Test-Path $back) { cmd /c "rmdir /s /q `"$back`"" }; New-Item -ItemType Directory $back | Out-Null
    Set-Content (Join-Path $root 'control.txt') 'unlocked'
    $env:P3_CONTROL = Join-Path $root 'control.txt'; $env:P3_MOUNT = $mnt; $env:P3_FIT = "$fit"
    $p = Start-Process $exe -ArgumentList "-p `"$back`" -m `"$mnt`"" -PassThru -WindowStyle Hidden
    WaitFor { Test-Path $mnt }
    Start-Sleep -Milliseconds 500
    $label = if ($fit -eq 4294967295) { 'WinFsp, cache infinite' } else { "WinFsp, cache $fit ms" }
    $all.Add((Bench $label $mnt))
    Stop-Process $p.Id; WaitFor { -not (Test-Path $mnt) }
    cmd /c "rmdir /s /q `"$back`""
}

$all | Format-List | Out-String -Width 200
$md = Join-Path $PSScriptRoot 'results.md'
$cols = $all[0].PSObject.Properties.Name
$lines = @('| ' + ($cols -join ' | ') + ' |', '|' + (($cols | ForEach-Object { '---' }) -join '|') + '|')
foreach ($row in $all) { $lines += '| ' + (($cols | ForEach-Object { $row.$_ }) -join ' | ') + ' |' }
$hdr = "Files: $Files small files (1-8 KB), big file: $BigMB MB. Seconds. $(Get-Date -Format 'yyyy-MM-dd HH:mm')"
Set-Content $md (@($hdr, '') + $lines) -Encoding UTF8
"written $md"
