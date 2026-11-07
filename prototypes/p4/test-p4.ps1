# P4: сбои и опасные действия с подключённой папкой.
# Запуск: powershell -NoProfile -ExecutionPolicy Bypass -File test-p4.ps1
# Работает только с тестовыми данными в C:\dev\zamok\tests\tmp\p4.
$ErrorActionPreference = 'Stop'
$root = 'C:\dev\zamok\tests\tmp\p4'
$exe = 'C:\dev\zamok\prototypes\p3\gatefs\target\release\p3-gatefs.exe'
$results = New-Object System.Collections.Generic.List[object]
function Check([string]$name, [scriptblock]$body) {
    try { $r = & $body; if ($r -eq $false) { throw 'returned false' }
        $results.Add([pscustomobject]@{ Test = $name; Ok = $true; Note = "$r" }) }
    catch { $results.Add([pscustomobject]@{ Test = $name; Ok = $false; Note = $_.Exception.Message }) }
}
function WaitFor([scriptblock]$cond, [int]$sec = 10) {
    $deadline = (Get-Date).AddSeconds($sec)
    while (-not (& $cond)) { if ((Get-Date) -gt $deadline) { return $false }; Start-Sleep -Milliseconds 100 }
    $true
}

Add-Type -TypeDefinition @'
using System; using System.IO; using System.Runtime.InteropServices; using System.Text;
public static class P4 {
  [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
  static extern IntPtr CreateFileW(string n, uint a, uint s, IntPtr sa, uint c, uint f, IntPtr t);
  [DllImport("kernel32.dll", SetLastError = true)]
  static extern bool DeviceIoControl(IntPtr h, uint code, byte[] inb, int inl, byte[] outb, int outl, out int ret, IntPtr ov);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
  [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern bool RemoveDirectoryW(string p);
  const uint FSCTL_SET = 0x900A4, FSCTL_GET = 0x900A8, FSCTL_DEL = 0x900AC, TAG = 0xA0000003;
  static IntPtr Open(string p, bool write) {
    // FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT
    var h = CreateFileW(p, write ? 0x40000000u | 0x80000000u : 0x80u, 7, IntPtr.Zero, 3, 0x02000000 | 0x00200000, IntPtr.Zero);
    if (h == new IntPtr(-1)) throw new IOException("open failed " + Marshal.GetLastWin32Error());
    return h;
  }
  // Записать точку подключения тома, как это делает WinFsp: \Device\Volume{GUID}\
  public static void SetMountPoint(string dir, string target) {
    byte[] t = Encoding.Unicode.GetBytes(target);
    int pathLen = t.Length * 2 + 4; // substitute + nul + print + nul
    var buf = new byte[8 + 8 + pathLen];
    BitConverter.GetBytes(TAG).CopyTo(buf, 0);
    BitConverter.GetBytes((ushort)(8 + pathLen)).CopyTo(buf, 4);
    BitConverter.GetBytes((ushort)0).CopyTo(buf, 8);
    BitConverter.GetBytes((ushort)t.Length).CopyTo(buf, 10);
    BitConverter.GetBytes((ushort)(t.Length + 2)).CopyTo(buf, 12);
    BitConverter.GetBytes((ushort)t.Length).CopyTo(buf, 14);
    t.CopyTo(buf, 16); t.CopyTo(buf, 16 + t.Length + 2);
    var h = Open(dir, true); int r;
    try { if (!DeviceIoControl(h, FSCTL_SET, buf, buf.Length, null, 0, out r, IntPtr.Zero)) throw new IOException("set failed " + Marshal.GetLastWin32Error()); }
    finally { CloseHandle(h); }
  }
  // Цель точки подключения или null, если это не точка подключения
  public static string GetMountTarget(string dir) {
    var h = Open(dir, false); var outb = new byte[16384]; int r;
    try {
      if (!DeviceIoControl(h, FSCTL_GET, null, 0, outb, outb.Length, out r, IntPtr.Zero)) return null;
      if (BitConverter.ToUInt32(outb, 0) != TAG) return null;
      int off = BitConverter.ToUInt16(outb, 8), len = BitConverter.ToUInt16(outb, 10);
      return Encoding.Unicode.GetString(outb, 16 + off, len);
    } finally { CloseHandle(h); }
  }
  // Мёртвая точка подключения: указывает на том WinFsp, которого больше нет
  public static bool IsStale(string dir) {
    var t = GetMountTarget(dir);
    if (t == null || !t.StartsWith(@"\Device\Volume{")) return false;
    try { Directory.GetFileSystemEntries(dir); return false; } catch { return true; }
  }
  public static void RemoveStale(string dir) {
    var h = Open(dir, true); int r;
    var hdr = new byte[8]; BitConverter.GetBytes(TAG).CopyTo(hdr, 0);
    try { if (!DeviceIoControl(h, FSCTL_DEL, hdr, 8, null, 0, out r, IntPtr.Zero)) throw new IOException("delete reparse failed " + Marshal.GetLastWin32Error()); }
    finally { CloseHandle(h); }
    if (!RemoveDirectoryW(dir)) throw new IOException("rmdir failed " + Marshal.GetLastWin32Error());
  }
}
public static class P4Op {
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
  public struct OP { public IntPtr hwnd; public uint wFunc; public string pFrom; public string pTo; public ushort fFlags; public bool aborted; public IntPtr map; public string title; }
  [DllImport("shell32.dll", CharSet = CharSet.Unicode)] static extern int SHFileOperation(ref OP op);
  public static int Run(uint f, string from, string to) {
    var o = new OP(); o.wFunc = f; o.pFrom = from + "\0\0"; o.pTo = to == null ? null : to + "\0\0";
    o.fFlags = 0x4 | 0x10 | 0x200 | 0x400; return SHFileOperation(ref o);
  }
}
'@

function Fresh([string]$name) {
    $d = Join-Path $root $name
    if (Test-Path $d) { cmd /c rmdir /s /q "$d" | Out-Null }
    New-Item -ItemType Directory -Force (Join-Path $d 'src\docs') | Out-Null
    1..5 | ForEach-Object { Set-Content (Join-Path $d "src\docs\f$_.txt") "file $_" }
    Set-Content (Join-Path $d 'control.txt') 'unlocked'
    $d
}
function StartFs([string]$d, [string]$mnt) {
    $env:P3_CONTROL = Join-Path $d 'control.txt'; $env:P3_MOUNT = $mnt
    $p = Start-Process $exe -ArgumentList "-p `"$d\src`" -m `"$mnt`"" -PassThru -WindowStyle Hidden
    $ok = WaitFor { Test-Path (Join-Path $mnt 'docs') } 8
    [pscustomobject]@{ Proc = $p; Mounted = $ok }
}
function SrcCount([string]$d) { @(Get-ChildItem (Join-Path $d 'src') -Recurse -File -Force).Count }

# A. Падение процесса при открытом окне Проводника и открытом файле
$d = Fresh 'a'; $mnt = Join-Path $d 'mnt'; $fs = StartFs $d $mnt
$shell = New-Object -ComObject Shell.Application
Start-Process explorer.exe $mnt
$null = WaitFor { @($shell.Windows() | Where-Object { try { $_.Document.Folder.Self.Path -eq $mnt } catch { $false } }).Count -gt 0 } 15
$h = [IO.File]::Open((Join-Path $mnt 'docs\f1.txt'), 'Open', 'Read', 'ReadWrite, Delete')
Stop-Process $fs.Proc.Id -Force
Check 'A. crash: mount path disappears' { WaitFor { -not (Test-Path $mnt) } 5 }
Check 'A. crash: source data intact' { (SrcCount $d) -eq 5 }
Check 'A. crash: open handle now fails' { try { $b = New-Object byte[] 4; $h.Read($b, 0, 4) | Out-Null; 'read still works?' -eq '' } catch { 'fails' } }
try { $h.Close() } catch {}
Check 'A. crash: Explorer window state' {
    $w = @($shell.Windows() | Where-Object { try { $_.Document.Folder.Self.Path -like "$d*" } catch { $false } })
    "windows on path=$($w.Count)"
}
@($shell.Windows() | Where-Object { try { $_.Document.Folder.Self.Path -like "$d*" } catch { $false } }) | ForEach-Object { $_.Quit() }
Check 'A. remount after crash works without cleanup' { $f2 = StartFs $d $mnt; Stop-Process $f2.Proc.Id; $f2.Mounted }

# B. Мёртвая точка подключения, как после выключения питания
$d = Fresh 'b'; $mnt = Join-Path $d 'mnt'
New-Item -ItemType Directory $mnt | Out-Null
[P4]::SetMountPoint($mnt, '\Device\Volume{11111111-2222-3333-4444-555555555555}\')
Check 'B. stale mount point created (non-admin)' { [P4]::GetMountTarget($mnt) }
Check 'B. stale mount point: what the user sees' { try { Get-ChildItem $mnt | Out-Null; 'lists fine?' } catch { $_.Exception.GetType().Name + ': ' + $_.Exception.Message } }
Check 'B. stale detected by IsStale' { [P4]::IsStale($mnt) }
Check 'B. mounting on top of stale point fails' { $f = StartFs $d $mnt; Stop-Process $f.Proc.Id -ErrorAction SilentlyContinue; if ($f.Mounted) { 'mounted anyway' -eq '' } else { 'mount failed as expected' } }
Check 'B. cleanup removes stale point' { [P4]::RemoveStale($mnt); -not (Test-Path $mnt) }
Check 'B. mount works after cleanup' { $f = StartFs $d $mnt; Stop-Process $f.Proc.Id; $f.Mounted }
Check 'B. live mount is not reported stale' { $f = StartFs $d $mnt; $s = [P4]::IsStale($mnt); Stop-Process $f.Proc.Id; -not $s }

# C. Перемещение открытой папки внутри того же диска
$d = Fresh 'c'; $mnt = Join-Path $d 'mnt'; $fs = StartFs $d $mnt
Check 'C. Move-Item (rename) of the mount point' {
    try { Move-Item $mnt (Join-Path $d 'moved'); "moved; new path exists=$(Test-Path (Join-Path $d 'moved\docs')) old=$(Test-Path $mnt) src files=$(SrcCount $d)" }
    catch { 'refused: ' + $_.Exception.Message }
}
Stop-Process $fs.Proc.Id -ErrorAction SilentlyContinue; Start-Sleep 1
Check 'C. after stop: leftovers' { "moved exists=$(Test-Path (Join-Path $d 'moved')) mnt exists=$(Test-Path $mnt) src files=$(SrcCount $d)" }

$d = Fresh 'c2'; $mnt = Join-Path $d 'mnt'; New-Item -ItemType Directory (Join-Path $d 'dest') | Out-Null; $fs = StartFs $d $mnt
Check 'C2. Explorer move (SHFileOperation) of the mount point into another folder' {
    $rc = [P4Op]::Run(1, $mnt, (Join-Path $d 'dest'))
    "rc=$rc dest\mnt\docs files=$(@(Get-ChildItem (Join-Path $d 'dest\mnt\docs') -ErrorAction SilentlyContinue).Count) old exists=$(Test-Path $mnt) src files=$(SrcCount $d)"
}
Stop-Process $fs.Proc.Id -ErrorAction SilentlyContinue; Start-Sleep 1
Check 'C2. after stop: what is left' { "dest\mnt is plain folder with files=$(@(Get-ChildItem (Join-Path $d 'dest\mnt') -Recurse -File -ErrorAction SilentlyContinue).Count) src files=$(SrcCount $d)" }

# D. Удаление открытой папки в Проводнике (без корзины)
$d = Fresh 'd'; $mnt = Join-Path $d 'mnt'; $fs = StartFs $d $mnt
Check 'D. Explorer delete of the unlocked mount point: do source files survive?' {
    $rc = [P4Op]::Run(3, $mnt, $null)
    "rc=$rc mnt exists=$(Test-Path $mnt) src files=$(SrcCount $d) of 5"
}
Stop-Process $fs.Proc.Id -ErrorAction SilentlyContinue; Start-Sleep 1

# E. То же, когда папка заперта
$d = Fresh 'e'; $mnt = Join-Path $d 'mnt'; $fs = StartFs $d $mnt
Set-Content (Join-Path $d 'control.txt') 'locked'; $null = WaitFor { Test-Path (Join-Path $d 'control.ack.txt') }; Start-Sleep -Milliseconds 300
Check 'E. Explorer delete of the locked mount point' {
    $rc = [P4Op]::Run(3, $mnt, $null)
    "rc=$rc mnt exists=$(Test-Path $mnt) fs alive=$(-not $fs.Proc.HasExited) src files=$(SrcCount $d) of 5"
}
Stop-Process $fs.Proc.Id -ErrorAction SilentlyContinue; Start-Sleep 1

$results | Format-Table -AutoSize -Wrap | Out-String -Width 220
"passed: $(@($results | Where-Object Ok).Count) / $($results.Count)"
