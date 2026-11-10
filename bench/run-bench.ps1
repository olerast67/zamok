# Repeatable speed benchmark for Zamok (ROADMAP stage 9). Same tasks as prototype P5.
#
# Variants: plain NTFS, WinFsp passthrough (prototype P3), Zamok through an isolated
# zamok-mount. Each variant is run -Runs times and the median of every measure is reported.
# The order of variants rotates between runs, so slow drift of the machine hits them equally.
#
# Run: powershell -NoProfile -ExecutionPolicy Bypass -File bench\run-bench.ps1
# Needs: cargo build --release -p zamok-fs -p zamok-crypto --examples
#        passthrough (optional): cargo build --release in prototypes\p3\gatefs,
#        pass its exe with -PassthroughExe.
#
# Touches only tests\tmp\bench (deleted at the end, except with -KeepSeed). It starts its own
# mount processes and stops only those. It does not read or change Defender or system settings.
#
# Defender: the share cannot be measured directly without an exclusion, which would change a
# system setting. Instead the script records how much CPU MsMpEng.exe used during each measure
# (performance counters are readable without admin rights).
param(
    [int]$Runs = 3,
    [int]$Files = 20000,
    [int]$BigMB = 1024,
    [string[]]$Variants = @('ntfs', 'passthrough', 'zamok'),
    [string[]]$Skip = @(),
    [string]$MountExe = '',
    [string]$PassthroughExe = '',
    [string]$Out = '',
    [string]$Label = '',
    [switch]$NoSettle,
    [switch]$KeepSeed
)
$ErrorActionPreference = 'Stop'
# Through -File a list arrives as one string with commas
$Skip = @($Skip | ForEach-Object { $_ -split ',' })
$repo = (Resolve-Path "$PSScriptRoot/..").Path
$root = "$repo\tests\tmp\bench"
if ($MountExe -eq '') { $MountExe = "$repo\core\target\release\zamok-mount.exe" }
if ($Out -eq '') { $Out = "$repo\bench\RESULTS.md" }
$makeVault = "$repo\core\target\release\examples\create_vault.exe"
$git = 'C:\Program Files\Git\cmd\git.exe'
$password = 'zamok-bench-password'

if ((Get-PSDrive C).Free -lt 8GB) { throw 'not enough free space on C:' }
if (-not (Test-Path $makeVault)) { throw "missing $makeVault (cargo build --release -p zamok-crypto --examples)" }
New-Item -ItemType Directory -Force $root | Out-Null

Add-Type -Language CSharp @'
using System;
using System.IO;
public static class BenchSeed {
    // Same shape as P5 (10 files per folder, like node_modules): 1-8 KB files, 2000 folders
    // for 20000 files. Smaller trees keep the same ratio of files to folders.
    public static void Tree(string root, int files) {
        var rng = new Random(42);
        var buf = new byte[8192];
        int pkgs = Math.Max(1, files / 80);
        for (int i = 0; i < files; i++) {
            string d = Path.Combine(root, "pkg" + (i % pkgs).ToString("D3"), "lib" + ((i / pkgs) % 8).ToString("D2"));
            Directory.CreateDirectory(d);
            rng.NextBytes(buf);
            using (var f = File.Create(Path.Combine(d, "file" + i + ".js"))) { f.Write(buf, 0, 1024 + rng.Next(7168)); }
        }
    }
    public static void Big(string path, int mb) {
        var rng = new Random(7);
        var buf = new byte[1 << 20];
        using (var f = File.Create(path)) { for (int i = 0; i < mb; i++) { rng.NextBytes(buf); f.Write(buf, 0, buf.Length); } }
    }
}
'@

$seed = "$root\seed-$Files-$BigMB"
$tree = "$seed\tree"
$big = "$seed\big.bin"
if (-not (Test-Path $tree)) {
    Write-Host "creating seed in $seed"
    New-Item -ItemType Directory -Force $seed | Out-Null
    [BenchSeed]::Tree($tree, $Files)
    [BenchSeed]::Big($big, $BigMB)
}

# CPU time (seconds) of a process from raw performance counters. Needs no admin rights.
function CpuOf([string]$name) {
    $p = Get-CimInstance Win32_PerfRawData_PerfProc_Process -Filter "Name='$name'"
    if ($null -eq $p) { return 0.0 }
    return [double]$p.PercentProcessorTime / 1e7
}
function Snapshot { return @{ Def = (CpuOf 'MsMpEng'); Sys = (CpuOf 'System') } }

# Wait until Defender and the system (lazy writer) calm down after the previous measure.
function Settle {
    if ($NoSettle) { return 0.0 }
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $quiet = 0
    while ($sw.Elapsed.TotalSeconds -lt 30 -and $quiet -lt 2) {
        $a = Snapshot; Start-Sleep -Milliseconds 1000; $b = Snapshot
        if ((($b.Def - $a.Def) + ($b.Sys - $a.Sys)) -lt 0.25) { $quiet++ } else { $quiet = 0 }
    }
    return [math]::Round($sw.Elapsed.TotalSeconds, 1)
}

function Remove-Tree([string]$p) { if (Test-Path $p) { cmd /c "rmdir /s /q `"$p`"" 2>$null | Out-Null } }

$raw = New-Object System.Collections.Generic.List[object]
function Measure-Step([string]$variant, [int]$run, [string]$name, [scriptblock]$action) {
    if ($Skip -contains $name) { return }
    $settled = Settle
    $before = Snapshot
    $sw = [Diagnostics.Stopwatch]::StartNew()
    & $action | Out-Null
    $sw.Stop()
    $after = Snapshot
    $row = [pscustomobject]@{
        Variant = $variant; Run = $run; Measure = $name
        Seconds = [math]::Round($sw.Elapsed.TotalSeconds, 3)
        DefenderCpu = [math]::Round($after.Def - $before.Def, 2)
        SystemCpu = [math]::Round($after.Sys - $before.Sys, 2)
        Settle = $settled
    }
    $raw.Add($row)
    Write-Host ("{0,-12} run {1} {2,-14} {3,9:N2} s  defender cpu {4,7:N2} s  settle {5} s" -f $variant, $run, $name, $row.Seconds, $row.DefenderCpu, $settled)
}

function Run-Tasks([string]$variant, [int]$run, [string]$work) {
    $dst = "$work\tree"
    $env:GIT_CONFIG_GLOBAL = "$root\gitconfig-empty"
    $env:GIT_CONFIG_NOSYSTEM = '1'
    if (-not (Test-Path $env:GIT_CONFIG_GLOBAL)) { Set-Content $env:GIT_CONFIG_GLOBAL '' }
    Measure-Step $variant $run 'unpack tree' { robocopy $tree $dst /E /NFL /NDL /NJH /NJS /NP /MT:8 | Out-Null }
    Measure-Step $variant $run 'list tree' { cmd /c "dir /s /b `"$dst`" > nul" }
    Measure-Step $variant $run 'git add+commit' {
        & $git -C $dst -c safe.directory=* init -q
        & $git -C $dst -c safe.directory=* add -A 2>$null
        & $git -C $dst -c safe.directory=* -c user.email=b@b -c user.name=b commit -q -m x
    }
    Measure-Step $variant $run 'git status' { & $git -C $dst -c safe.directory=* status --porcelain }
    Measure-Step $variant $run 'copy 1 GB' { robocopy (Split-Path $big) $work big.bin /NFL /NDL /NJH /NJS /NP /J | Out-Null }
    Measure-Step $variant $run 'read 1 GB' { cmd /c "copy /b `"$work\big.bin`" nul > nul" }
    Measure-Step $variant $run 'delete tree' { cmd /c "rmdir /s /q `"$dst`"" }
    if (Test-Path "$work\big.bin") { Remove-Item "$work\big.bin" -Force }
}

function Run-Mounted([string]$variant, [int]$run) {
    $back = "$root\$variant-back"; $mnt = "$root\$variant-mnt"
    Remove-Tree $back
    if ($variant -eq 'zamok') {
        $empty = "$root\empty-src"; New-Item -ItemType Directory -Force $empty | Out-Null
        & $makeVault $empty $back $password | Out-Null
        $psi = New-Object Diagnostics.ProcessStartInfo $MountExe, "`"$back`" `"$mnt`" Bench"
        $psi.EnvironmentVariables['ZAMOK_PASSWORD'] = $password
    } else {
        New-Item -ItemType Directory -Force $back | Out-Null
        $control = "$root\p3-control.txt"
        Set-Content $control 'unlocked'
        $psi = New-Object Diagnostics.ProcessStartInfo $PassthroughExe, "-p `"$back`" -m `"$mnt`""
        $psi.EnvironmentVariables['P3_CONTROL'] = $control
        $psi.EnvironmentVariables['P3_MOUNT'] = $mnt
        $psi.EnvironmentVariables['P3_FIT'] = '1000'
    }
    $psi.UseShellExecute = $false; $psi.RedirectStandardInput = $true
    $psi.RedirectStandardOutput = $true; $psi.CreateNoWindow = $true
    $proc = [Diagnostics.Process]::Start($psi)
    try {
        if ($variant -eq 'zamok') {
            if ($proc.StandardOutput.ReadLine() -ne 'mounted') { throw 'zamok-mount did not mount' }
        } else {
            $deadline = (Get-Date).AddSeconds(15)
            while (-not (Test-Path $mnt)) {
                if ((Get-Date) -gt $deadline) { throw 'passthrough did not mount' }
                Start-Sleep -Milliseconds 100
            }
        }
        Run-Tasks $variant $run $mnt
    } finally {
        if ($variant -eq 'zamok') { $proc.StandardInput.WriteLine('quit') }
        if (-not $proc.WaitForExit(20000)) { $proc.Kill(); $proc.WaitForExit(5000) | Out-Null }
        Remove-Tree $back
        Remove-Tree $mnt
    }
}

$usable = @($Variants | Where-Object { $_ -ne 'passthrough' -or ($PassthroughExe -ne '' -and (Test-Path $PassthroughExe)) })
if ($usable.Count -ne $Variants.Count) { Write-Host 'passthrough exe not given: skipping that variant' }
try {
    for ($run = 1; $run -le $Runs; $run++) {
        # Rotate the order so that background drift does not always hit the same variant
        $shift = ($run - 1) % $usable.Count
        $order = @($usable[$shift..($usable.Count - 1)]) + @(if ($shift -gt 0) { $usable[0..($shift - 1)] })
        foreach ($v in $order) {
            if ($v -eq 'ntfs') {
                $w = "$root\ntfs"; Remove-Tree $w; New-Item -ItemType Directory $w | Out-Null
                try { Run-Tasks 'ntfs' $run $w } finally { Remove-Tree $w }
            } else {
                Run-Mounted $v $run
            }
        }
    }
} finally {
    if (-not $KeepSeed) { Remove-Tree $seed }
    if (-not $KeepSeed) { Remove-Tree "$root\empty-src"; Remove-Item "$root\*.txt", "$root\gitconfig-empty" -ErrorAction SilentlyContinue }
}

function Median([double[]]$v) { $s = $v | Sort-Object; return $s[[int][math]::Floor(($s.Count - 1) / 2)] }
$measures = @('unpack tree', 'list tree', 'git add+commit', 'git status', 'copy 1 GB', 'read 1 GB', 'delete tree') | Where-Object { $Skip -notcontains $_ }
$sb = New-Object Text.StringBuilder
[void]$sb.AppendLine("Label: $Label. Files: $Files, big file: $BigMB MB, runs: $Runs, settle: $(-not $NoSettle). Seconds, median (min-max).")
[void]$sb.AppendLine("")
[void]$sb.AppendLine("| Variant | " + ($measures -join ' | ') + " |")
[void]$sb.AppendLine("|---|" + (($measures | ForEach-Object { '---' }) -join '|') + "|")
foreach ($v in $usable) {
    $cells = foreach ($m in $measures) {
        $s = @($raw | Where-Object { $_.Variant -eq $v -and $_.Measure -eq $m } | ForEach-Object { $_.Seconds })
        "{0:N2} ({1:N2}-{2:N2})" -f (Median $s), ($s | Measure-Object -Minimum).Minimum, ($s | Measure-Object -Maximum).Maximum
    }
    [void]$sb.AppendLine("| $v | " + ($cells -join ' | ') + " |")
}
[void]$sb.AppendLine("")
[void]$sb.AppendLine("MsMpEng.exe CPU seconds during the measure (median):")
[void]$sb.AppendLine("")
[void]$sb.AppendLine("| Variant | " + ($measures -join ' | ') + " |")
[void]$sb.AppendLine("|---|" + (($measures | ForEach-Object { '---' }) -join '|') + "|")
foreach ($v in $usable) {
    $cells = foreach ($m in $measures) {
        $s = @($raw | Where-Object { $_.Variant -eq $v -and $_.Measure -eq $m } | ForEach-Object { $_.DefenderCpu })
        "{0:N1}" -f (Median $s)
    }
    [void]$sb.AppendLine("| $v | " + ($cells -join ' | ') + " |")
}
$text = $sb.ToString()
Write-Host $text
$rawOut = "$root\raw-$(Get-Date -Format 'yyyyMMdd-HHmmss').csv"
$raw | Export-Csv $rawOut -NoTypeInformation
Write-Host "raw rows: $rawOut"
Add-Content -Path $Out -Value $text -Encoding UTF8
