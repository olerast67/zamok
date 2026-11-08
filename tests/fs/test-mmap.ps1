# Этап 3: файл, отображённый в память (memory-mapped), и запирание папки.
# Программа отображает файл, читает его, папку запирают, программа читает снова.
# Запуск: powershell -NoProfile -ExecutionPolicy Bypass -File tests\fs\test-mmap.ps1
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path "$PSScriptRoot/../..").Path
$root = "$repo\tests\tmp\mmap"; $vault = "$root\vault"; $mnt = "$root\mnt"; $src = "$root\src"
Remove-Item $root -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $src | Out-Null
$data = New-Object byte[] (256KB); (New-Object Random 5).NextBytes($data); [IO.File]::WriteAllBytes("$src\data.bin", $data)
Push-Location "$repo\core"; & "$env:USERPROFILE/.cargo/bin/cargo.exe" run -q --release --example create_vault -- $src $vault pw | Out-Null; Pop-Location
$psi = New-Object Diagnostics.ProcessStartInfo "$repo/core/target/release/zamok-mount.exe", "`"$vault`" `"$mnt`" Mmap"
$psi.UseShellExecute = $false; $psi.RedirectStandardInput = $true; $psi.RedirectStandardOutput = $true; $psi.CreateNoWindow = $true
$psi.EnvironmentVariables['ZAMOK_PASSWORD'] = 'pw'
$proc = [Diagnostics.Process]::Start($psi)
if ($proc.StandardOutput.ReadLine() -ne 'mounted') { throw 'mount failed' }
function Send([string]$c) { $proc.StandardInput.WriteLine($c); $proc.StandardInput.Flush(); $proc.StandardOutput.ReadLine() }
$log = "$root\..\mmap-log.txt"; Remove-Item $log -ErrorAction SilentlyContinue
function Out1([string]$t) { Add-Content $log $t }
try {
    $mm = [IO.MemoryMappedFiles.MemoryMappedFile]::CreateFromFile("$mnt\data.bin", [IO.FileMode]::Open, [NullString]::Value, 0, [IO.MemoryMappedFiles.MemoryMappedFileAccess]::Read)
    $view = $mm.CreateViewAccessor(0, 0, [IO.MemoryMappedFiles.MemoryMappedFileAccess]::Read)
    $first = $view.ReadByte(0); $mid = $view.ReadByte(100000)
    Out1 "before lock: read ok (byte0=$first, matches=$($first -eq $data[0]))"
    Out1 "lock: " + (Send 'lock')
    $r1 = try { $b = $view.ReadByte(0); "read ok, byte0 matches=$($b -eq $data[0])" } catch { "read failed: $($_.Exception.InnerException.Message)" }
    Out1 "after lock, page that was already read: $r1"
    $r2 = try { $b = $view.ReadByte(200000); "read ok, matches=$($b -eq $data[200000])" } catch { "read failed: $($_.Exception.GetType().Name)" }
    Out1 "after lock, page never read before: $r2"
    $view.Dispose(); $mm.Dispose()
    $r3 = try { [IO.File]::ReadAllBytes("$mnt\data.bin") | Out-Null; 'new open worked' } catch { 'new open refused' }
    Out1 "after lock, new open: $r3"
} finally {
    try { [void](Send 'quit') } catch {}
    if (-not $proc.WaitForExit(15000)) { $proc.Kill() }
}
Get-Content $log
