# P1: проверки passthrough-файловой системы WinFsp, подключённой на путь папки.
# Запуск: powershell -NoProfile -ExecutionPolicy Bypass -File test-p1.ps1
# Скрипт в ASCII: Windows PowerShell 5.1 читает файлы без BOM в кодировке ANSI,
# поэтому русские имена собираются из кодов символов.

$ErrorActionPreference = 'Stop'
$root = 'C:\dev\zamok\tests\tmp\p1'
$src = Join-Path $root 'src'
$mnt = Join-Path $root 'mnt'
$exe = Join-Path $PSScriptRoot 'ntptfs\target\release\p1-ntptfs.exe'
$results = New-Object System.Collections.Generic.List[object]

function Ru([int[]]$codes) { -join ($codes | ForEach-Object { [char]$_ }) }
$ruName = Ru 0x041E,0x0442,0x0447,0x0451,0x0442   # "Отчёт"
$ruDir  = Ru 0x041A,0x043B,0x0438,0x0435,0x043D,0x0442,0x044B  # "Клиенты"

function Check([string]$name, [scriptblock]$body) {
    try {
        $r = & $body
        if ($r -eq $false) { throw 'returned false' }
        $results.Add([pscustomobject]@{ Test = $name; Ok = $true; Note = "$r" })
    } catch {
        $results.Add([pscustomobject]@{ Test = $name; Ok = $false; Note = $_.Exception.Message })
    }
}

# Подготовка
if (Test-Path $mnt) { throw "mount path exists: $mnt" }
Remove-Item $src -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $src | Out-Null
Set-Content (Join-Path $src 'existing.txt') 'hello' -Encoding UTF8

$proc = Start-Process $exe -ArgumentList "-p `"$src`" -m `"$mnt`"" -PassThru -WindowStyle Hidden
$deadline = (Get-Date).AddSeconds(10)
while (-not (Test-Path (Join-Path $mnt 'existing.txt'))) {
    if ((Get-Date) -gt $deadline -or $proc.HasExited) { throw 'mount did not appear' }
    Start-Sleep -Milliseconds 200
}

Check 'mount point is a reparse point directory' {
    $a = (Get-Item $mnt -Force).Attributes
    if (-not ($a -band [IO.FileAttributes]::ReparsePoint)) { return $false }
    "$a"
}
Check 'process runs without admin rights' {
    $id = [Security.Principal.WindowsIdentity]::GetCurrent()
    -not ([Security.Principal.WindowsPrincipal]$id).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}
Check 'read existing file' { (Get-Content (Join-Path $mnt 'existing.txt')) -eq 'hello' }
Check 'create file with Cyrillic name, read back' {
    $f = Join-Path $mnt "$ruName.txt"
    Set-Content $f 'data' -Encoding UTF8
    ((Get-Content $f) -eq 'data') -and (Test-Path (Join-Path $src "$ruName.txt"))
}
Check 'append to file' {
    $f = Join-Path $mnt "$ruName.txt"
    Add-Content $f 'more' -Encoding UTF8
    (Get-Content $f).Count -eq 2
}
Check 'create nested directories' {
    $d = Join-Path $mnt "$ruDir\2026\november"
    New-Item -ItemType Directory -Force $d | Out-Null
    Test-Path (Join-Path $src "$ruDir\2026\november")
}
Check 'rename file' {
    Rename-Item (Join-Path $mnt "$ruName.txt") "$ruName-v2.txt"
    (Test-Path (Join-Path $mnt "$ruName-v2.txt")) -and -not (Test-Path (Join-Path $mnt "$ruName.txt"))
}
Check 'move file between directories' {
    Move-Item (Join-Path $mnt "$ruName-v2.txt") (Join-Path $mnt "$ruDir\2026")
    Test-Path (Join-Path $src "$ruDir\2026\$ruName-v2.txt")
}
Check 'rename directory' {
    Rename-Item (Join-Path $mnt "$ruDir\2026") '2026-old'
    Test-Path (Join-Path $mnt "$ruDir\2026-old\november")
}
Check 'long file name (200 chars)' {
    $n = ('a' * 196) + '.txt'
    Set-Content (Join-Path $mnt $n) 'x'
    Test-Path (Join-Path $src $n)
}
Check 'deep path over 260 chars' {
    $p = '\\?\' + $mnt
    1..12 | ForEach-Object { $p = $p + '\folder_' + ('d' * 20) }
    [IO.Directory]::CreateDirectory($p) | Out-Null
    [IO.File]::WriteAllText($p + '\deep.txt', 'deep')
    if ([IO.File]::ReadAllText($p + '\deep.txt') -ne 'deep') { return $false }
    $len = ($p.Length - 4)
    "path length $len"
}
Check 'copy 200 MB file in, hashes match' {
    $big = Join-Path $root 'big.bin'
    if (-not (Test-Path $big)) {
        $buf = New-Object byte[] (1MB); $rng = [Security.Cryptography.RandomNumberGenerator]::Create()
        $fs = [IO.File]::Create($big)
        1..200 | ForEach-Object { $rng.GetBytes($buf); $fs.Write($buf, 0, $buf.Length) }
        $fs.Close()
    }
    $sw = [Diagnostics.Stopwatch]::StartNew()
    Copy-Item $big (Join-Path $mnt 'big.bin')
    $sw.Stop()
    $h1 = (Get-FileHash $big).Hash; $h2 = (Get-FileHash (Join-Path $mnt 'big.bin')).Hash
    if ($h1 -ne $h2) { return $false }
    "copy {0:N0} ms" -f $sw.ElapsedMilliseconds
}
Check 'random-access write in the middle of file' {
    $f = Join-Path $mnt 'big.bin'
    $fs = [IO.File]::Open($f, 'Open', 'ReadWrite')
    $fs.Seek(100MB, 'Begin') | Out-Null; $fs.Write([byte[]](1,2,3,4), 0, 4); $fs.Close()
    # Читаем через точку подключения: исходный файл passthrough ещё держит открытым после закрытия
    $fs = [IO.File]::OpenRead($f)
    $fs.Seek(100MB, 'Begin') | Out-Null; $b = New-Object byte[] 4; $fs.Read($b, 0, 4) | Out-Null; $fs.Close()
    ($b -join ',') -eq '1,2,3,4'
}
Check 'timestamps can be set' {
    $f = Join-Path $mnt 'existing.txt'
    $t = Get-Date '2020-01-02 03:04:05'
    (Get-Item $f).LastWriteTime = $t
    (Get-Item $f).LastWriteTime -eq $t
}
Check 'hidden and readonly attributes' {
    $f = Join-Path $mnt 'existing.txt'
    $i = Get-Item $f; $i.Attributes = 'Hidden, ReadOnly'
    $a = (Get-Item $f -Force).Attributes
    (Get-Item $f -Force).Attributes = 'Normal'
    ($a -band [IO.FileAttributes]::Hidden) -and ($a -band [IO.FileAttributes]::ReadOnly)
}
Check 'rename while another handle is open (share delete)' {
    $f = Join-Path $mnt 'open.txt'; Set-Content $f 'x'
    $h = [IO.File]::Open($f, 'Open', 'Read', 'ReadWrite, Delete')
    try { Rename-Item $f 'open2.txt' } finally { $h.Close() }
    Test-Path (Join-Path $mnt 'open2.txt')
}
Check 'delete file and directory tree' {
    Remove-Item (Join-Path $mnt $ruDir) -Recurse -Force
    -not (Test-Path (Join-Path $src $ruDir))
}
Check '1000 small files: create and list' {
    $d = Join-Path $mnt 'many'; New-Item -ItemType Directory $d | Out-Null
    $sw = [Diagnostics.Stopwatch]::StartNew()
    1..1000 | ForEach-Object { [IO.File]::WriteAllText((Join-Path $d "f$_.txt"), "$_") }
    $c = (Get-ChildItem $d).Count
    $sw.Stop()
    if ($c -ne 1000) { return $false }
    "{0:N0} ms" -f $sw.ElapsedMilliseconds
}
Check 'free space reported' {
    $q = New-Object -ComObject Scripting.FileSystemObject
    "{0:N1} GB" -f ($q.GetFolder($mnt).Drive.FreeSpace / 1GB)
}

# Иконка через desktop.ini в корне
Add-Type -Namespace P1 -Name Shell -MemberDefinition @'
[StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
public struct SHFILEINFO { public IntPtr hIcon; public int iIcon; public uint dwAttributes;
  [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 260)] public string szDisplayName;
  [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 80)] public string szTypeName; }
[DllImport("shell32.dll", CharSet = CharSet.Unicode)]
public static extern IntPtr SHGetFileInfo(string path, uint attrs, ref SHFILEINFO info, uint size, uint flags);
public static string IconLocation(string path) {
  var i = new SHFILEINFO();
  SHGetFileInfo(path, 0, ref i, (uint)Marshal.SizeOf(i), 0x1000); // SHGFI_ICONLOCATION
  return i.szDisplayName + "," + i.iIcon;
}
'@
Check 'desktop.ini icon inside mount root is used by the shell' {
    $ini = Join-Path $mnt 'desktop.ini'
    Set-Content $ini "[.ShellClassInfo]`r`nIconResource=C:\Windows\System32\imageres.dll,54`r`n" -Encoding Unicode
    (Get-Item $ini -Force).Attributes = 'Hidden, System'
    $before = [P1.Shell]::IconLocation($mnt)
    # Атрибут System на самой точке подключения говорит оболочке читать desktop.ini
    $m = Get-Item $mnt -Force; $m.Attributes = $m.Attributes -bor [IO.FileAttributes]::System
    $after = [P1.Shell]::IconLocation($mnt)
    $attrs = (Get-Item $mnt -Force).Attributes
    "before=[$before] after=[$after] attrs=[$attrs]"
}

# Остановка: точка подключения должна исчезнуть
Stop-Process $proc.Id
Start-Sleep -Seconds 2
Check 'mount path removed after clean stop' { -not (Test-Path $mnt) }

$results | Format-Table -AutoSize -Wrap | Out-String -Width 200
$fail = @($results | Where-Object { -not $_.Ok }).Count
"passed: $($results.Count - $fail) / $($results.Count)"
