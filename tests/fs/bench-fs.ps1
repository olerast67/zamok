# Этап 3: скорость зашифрованной папки Zamok против NTFS (те же задачи, что в P5).
# Запуск: powershell -NoProfile -ExecutionPolicy Bypass -File tests\fs/bench-fs.ps1
# Варианты: NTFS напрямую и Zamok (шифрование, FileInfoTimeout = 0).
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
$w = Join-Path $root 'ntfs'; if (Test-Path $w) { cmd /c "rmdir /s /q `"$w`"" }; New-Item -ItemType Directory $w | Out-Null
$all.Add((Bench 'NTFS' $w))
cmd /c "rmdir /s /q `"$w`""

$repo = (Resolve-Path "$PSScriptRoot/../..").Path
$vault = Join-Path $root 'zamok-vault'; $mnt = Join-Path $root 'zamok-mnt'; $empty = Join-Path $root 'empty-src'
foreach ($d in $vault) { if (Test-Path $d) { cmd /c "rmdir /s /q `"$d`"" } }
New-Item -ItemType Directory -Force $empty | Out-Null
Push-Location "$repo\core"; & "$env:USERPROFILE\.cargo/bin\cargo.exe" run -q --release --example create_vault -- $empty $vault bench | Out-Null; Pop-Location
$psi = New-Object Diagnostics.ProcessStartInfo "$repo\core/target/release\zamok-mount.exe", "`"$vault`" `"$mnt`" Bench"
$psi.UseShellExecute = $false; $psi.RedirectStandardInput = $true; $psi.RedirectStandardOutput = $true; $psi.CreateNoWindow = $true
$psi.EnvironmentVariables['ZAMOK_PASSWORD'] = 'bench'
$proc = [Diagnostics.Process]::Start($psi)
if ($proc.StandardOutput.ReadLine() -ne 'mounted') { throw 'mount failed' }
try { $all.Add((Bench 'Zamok (encrypted)' $mnt)) }
finally { $proc.StandardInput.WriteLine('quit'); if (-not $proc.WaitForExit(20000)) { $proc.Kill() } }
cmd /c "rmdir /s /q `"$vault`""

$all | Format-List | Out-String -Width 200
$md = Join-Path $PSScriptRoot 'bench-results.md'
$cols = $all[0].PSObject.Properties.Name
$lines = @('| ' + ($cols -join ' | ') + ' |')
$lines += '|' + (($cols | ForEach-Object { '---' }) -join '|') + '|'
foreach ($row in $all) { $lines += '| ' + (($cols | ForEach-Object { $row.$_ }) -join ' | ') + ' |' }
$hdr = "Files: $Files small files (1-8 KB), big file: $BigMB MB. Seconds. $(Get-Date -Format 'yyyy-MM-dd HH:mm')"
Set-Content $md (@($hdr, '') + $lines) -Encoding UTF8
"written $md"
