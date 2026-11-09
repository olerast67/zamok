# Stage 3 load test: one folder with 100,000 small files through a real mount.
# Isolated: its own vault under tests\tmp\load, its own zamok-mount, nothing else is touched.
# Everything the script made is deleted at the end, even after a failure.
# Run: powershell -NoProfile -ExecutionPolicy Bypass -File tests\fs\test-load.ps1 [-Count 1000]
# Needs: cargo build --release -p zamok-fs (see README.md).
param(
    [int]$Count = 100000,
    [int]$FileBytes = 1024,
    [int]$Sample = 3000
)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path "$PSScriptRoot/../..").Path
$root = "$repo\tests\tmp\load"
$src = "$root\src"; $vault = "$root\vault"; $mnt = "$root\mnt"
$mountExe = "$repo\core\target\release\zamok-mount.exe"
$pw = 'zamok-load-test'

$free = (Get-PSDrive C).Free
if ($free -lt 6GB) { throw "not enough free space on C: ($free bytes)" }
# Files take at least one 4 KiB cluster each, plus the file table
$budget = [long]$Count * 6KB
if ($budget -gt 2GB) { throw "the test would need about $budget bytes, over the 2 GB limit" }

Add-Type -Language CSharp @'
using System;
using System.IO;
public static class LoadTest {
    public static string Name(int i) { return "file-" + i.ToString("D6") + ".bin"; }
    public static byte[] Body(int i, int size) {
        // Content depends on the file number, so a mixed-up file is caught
        var b = new byte[size];
        ulong x = (ulong)i * 0x9E3779B97F4A7C15UL + 1;
        for (int k = 0; k < size; k++) {
            x ^= x << 13; x ^= x >> 7; x ^= x << 17;
            b[k] = (byte)x;
        }
        return b;
    }
    public static long Create(string dir, int count, int size) {
        var sw = System.Diagnostics.Stopwatch.StartNew();
        for (int i = 0; i < count; i++) File.WriteAllBytes(Path.Combine(dir, Name(i)), Body(i, size));
        return sw.ElapsedMilliseconds;
    }
    public static int Listed(string dir, out long ms) {
        var sw = System.Diagnostics.Stopwatch.StartNew();
        int n = 0;
        foreach (var f in Directory.EnumerateFiles(dir)) n++;
        ms = sw.ElapsedMilliseconds;
        return n;
    }
    // Which file of a sample is opened depends only on the seed
    public static int Verify(string dir, int count, int size, int sample, int seed, out long ms) {
        var r = new Random(seed);
        var sw = System.Diagnostics.Stopwatch.StartNew();
        int bad = 0;
        for (int s = 0; s < sample; s++) {
            int i = r.Next(count);
            byte[] got = File.ReadAllBytes(Path.Combine(dir, Name(i)));
            byte[] want = Body(i, size);
            if (got.Length != want.Length) { bad++; continue; }
            for (int k = 0; k < got.Length; k++) if (got[k] != want[k]) { bad++; break; }
        }
        ms = sw.ElapsedMilliseconds;
        return bad;
    }
    public static long Delete(string dir, int count) {
        var sw = System.Diagnostics.Stopwatch.StartNew();
        for (int i = 0; i < count; i++) File.Delete(Path.Combine(dir, Name(i)));
        return sw.ElapsedMilliseconds;
    }
    public static long Size(string dir, out int files) {
        long total = 0; files = 0;
        foreach (var f in new DirectoryInfo(dir).EnumerateFiles("*", SearchOption.AllDirectories)) { total += f.Length; files++; }
        return total;
    }
}
'@

$results = New-Object System.Collections.Generic.List[object]
function Row([string]$step, [string]$value) {
    $results.Add([pscustomobject]@{ Step = $step; Value = $value })
    Write-Host ("{0,-46} {1}" -f $step, $value)
}
function Mem([string]$when) {
    $proc.Refresh()
    Row "memory, $when" ("private {0:N0} MB, working set {1:N0} MB" -f ($proc.PrivateMemorySize64 / 1MB), ($proc.WorkingSet64 / 1MB))
}
function Send([string]$cmd) { $proc.StandardInput.WriteLine($cmd); $proc.StandardInput.Flush(); $proc.StandardOutput.ReadLine() }

Remove-Item $root -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $src | Out-Null
Push-Location "$repo\core"
& "$env:USERPROFILE\.cargo\bin\cargo.exe" run -q --release --example create_vault -- $src $vault $pw | Out-Null
Pop-Location
if (-not (Test-Path "$vault\masterkey.cryptomator")) { throw 'vault was not created' }

$psi = New-Object Diagnostics.ProcessStartInfo $mountExe, "`"$vault`" `"$mnt`" LoadTest"
$psi.UseShellExecute = $false; $psi.RedirectStandardInput = $true; $psi.RedirectStandardOutput = $true; $psi.CreateNoWindow = $true
$psi.EnvironmentVariables['ZAMOK_PASSWORD'] = $pw
$proc = [Diagnostics.Process]::Start($psi)
if ($proc.StandardOutput.ReadLine() -ne 'mounted') { throw 'mount failed' }

$failed = $false
try {
    $dir = Join-Path $mnt 'flat'
    New-Item -ItemType Directory $dir | Out-Null
    Row 'files x bytes' ("{0:N0} x {1}" -f $Count, $FileBytes)
    Mem 'empty folder'

    $ms = [LoadTest]::Create($dir, $Count, $FileBytes)
    Row 'create' ("{0:N1} s, {1:N0} files/s" -f ($ms / 1000), ($Count * 1000 / [math]::Max(1, $ms)))
    Mem 'after create'

    $files = 0; $size = [LoadTest]::Size($vault, [ref]$files)
    Row 'vault on disk' ("{0:N0} MB in {1:N0} files" -f ($size / 1MB), $files)
    if ($size -gt 2GB) { throw "vault is over 2 GB: $size" }

    $ms = 0L; $n = [LoadTest]::Listed($dir, [ref]$ms)
    Row 'list' ("{0:N1} s, {1:N0} names" -f ($ms / 1000), $n)
    if ($n -ne $Count) { throw "listed $n, expected $Count" }

    $ms = 0L; $bad = [LoadTest]::Verify($dir, $Count, $FileBytes, $Sample, 11, [ref]$ms)
    Row "open and read $Sample" ("{0:N1} s, {1} wrong" -f ($ms / 1000), $bad)
    if ($bad -ne 0) { throw "$bad files with wrong content" }
    Mem 'after reading'

    $sw = [Diagnostics.Stopwatch]::StartNew()
    if ((Send 'lock') -ne 'locked') { throw 'lock failed' }
    Row 'lock' ("{0:N2} s" -f $sw.Elapsed.TotalSeconds)
    if (Test-Path (Join-Path $dir ([LoadTest]::Name(0)))) { throw 'a file is visible after lock' }
    Mem 'locked'

    $sw = [Diagnostics.Stopwatch]::StartNew()
    if ((Send "unlock $pw") -ne 'unlocked') { throw 'unlock failed' }
    Row 'unlock' ("{0:N2} s" -f $sw.Elapsed.TotalSeconds)

    $ms = 0L; $n = [LoadTest]::Listed($dir, [ref]$ms)
    Row 'list after unlock (names decrypted again)' ("{0:N1} s, {1:N0} names" -f ($ms / 1000), $n)
    if ($n -ne $Count) { throw "listed $n after unlock, expected $Count" }

    $ms = 0L; $bad = [LoadTest]::Verify($dir, $Count, $FileBytes, $Sample, 22, [ref]$ms)
    Row "verify $Sample after unlock" ("{0:N1} s, {1} wrong" -f ($ms / 1000), $bad)
    if ($bad -ne 0) { throw "$bad files with wrong content after unlock" }
    Mem 'after unlock and verify'
    $proc.Refresh()
    Row 'peak working set' ("{0:N0} MB" -f ($proc.PeakWorkingSet64 / 1MB))

    $ms = [LoadTest]::Delete($dir, $Count)
    Row 'delete every file' ("{0:N1} s, {1:N0} files/s" -f ($ms / 1000), ($Count * 1000 / [math]::Max(1, $ms)))
    $ms = 0L; $n = [LoadTest]::Listed($dir, [ref]$ms)
    if ($n -ne 0) { throw "$n files left after delete" }
    Mem 'after delete'
}
catch {
    $failed = $true
    Write-Host "FAILED: $($_.Exception.Message)"
}
finally {
    try {
        $proc.StandardInput.WriteLine('quit'); $proc.StandardInput.Flush()
        if (-not $proc.WaitForExit(120000)) { Stop-Process -Id $proc.Id -Force; $proc.WaitForExit() }
    } catch { Write-Host "cannot stop the test mount: $($_.Exception.Message)" }
    # Only this test's folder; the mount point is gone once zamok-mount has exited
    Start-Sleep -Milliseconds 500
    cmd /c rd /s /q "`"$root`"" 2>$null
    if (Test-Path $root) { Start-Sleep -Seconds 3; cmd /c rd /s /q "`"$root`"" 2>$null }
    if (Test-Path $root) { Write-Host "WARNING: could not delete $root"; $failed = $true }
}
if ($failed) { exit 1 }
Write-Host 'load test passed'
