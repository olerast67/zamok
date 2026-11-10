# Etap 3: fayl, otobrazhennyy v pamyat (memory-mapped), i zapiranie papki cherez zamok-core.
# Programma-pomoshchnik (mapper.exe, sobiraetsya zdes zhe) otobrazhaet fayl iz otkrytoy papki
# i zakryvaet deskriptor fayla: ostaetsya tolko otobrazhenie, kak u programm, kotorye tak
# derzhat dokument. Papku zapirayut, pomoshchnik chitaet snova. Rezultat opisan v DESIGN.md
# (razdel 6, model ugroz) i v tests\fs\README.md.
# Core zapuskaetsya so svoey papkoy dannyh i svoim kanalom, kak v tests\core\test-core.ps1.
# Zapusk: powershell -NoProfile -ExecutionPolicy Bypass -File tests\fs\test-mmap.ps1
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path "$PSScriptRoot/../..").Path
$root = "$repo\tests\tmp\mmap"; $vault = "$root\.Mmap.zamok"; $mnt = "$root\Mmap"; $src = "$root\src"; $data = "$root\data"
$coreExe = "$repo\core\target\release\zamok-core.exe"
$pw = 'zamok-mmap-test'
$pipeName = "zamok-mmap-test-$PID"
$results = New-Object System.Collections.Generic.List[object]
function Check([string]$name, [scriptblock]$body) {
    try { $r = & $body; if ($r -eq $false) { throw 'returned false' }
        $results.Add([pscustomobject]@{ Test = $name; Ok = $true; Note = "$r" }) }
    catch { $results.Add([pscustomobject]@{ Test = $name; Ok = $false; Note = $_.Exception.Message }) }
}
function WaitFor([scriptblock]$cond, [int]$ms = 10000) {
    $deadline = (Get-Date).AddMilliseconds($ms)
    while (-not $(try { & $cond } catch { $false })) { if ((Get-Date) -gt $deadline) { return $false }; Start-Sleep -Milliseconds 100 }
    $true
}

Get-Process zamok-core -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $coreExe } | Stop-Process -Force
Remove-Item $root -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $src | Out-Null
$size = 1MB
$bytes = New-Object byte[] $size; (New-Object Random 5).NextBytes($bytes); [IO.File]::WriteAllBytes("$src\data.bin", $bytes)
function Want([long]$off) { -join ($bytes[$off..($off + 15)] | ForEach-Object { $_.ToString('x2') }) }

# Pomoshchnik: komandy po odnoy stroke na vhode, otvet stroka na vyhode.
#   map <put>     otobrazit fayl tolko dlya chteniya i zakryt deskriptory fayla i sekcii
#   read <smeshchenie>  16 bayt v hex ili "error <kod>" (oshibka podkachki stranicy)
$mapper = "$root\mapper.exe"
Add-Type -OutputAssembly $mapper -OutputType ConsoleApplication -TypeDefinition @'
using System; using System.Runtime.InteropServices; using System.Text;
using System.Runtime.ExceptionServices; using System.Security;
public static class Mapper {
  [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
  static extern IntPtr CreateFileW(string n, uint a, uint s, IntPtr sa, uint c, uint f, IntPtr t);
  [DllImport("kernel32.dll", SetLastError = true)]
  static extern IntPtr CreateFileMappingW(IntPtr f, IntPtr sa, uint prot, uint hi, uint lo, IntPtr name);
  [DllImport("kernel32.dll", SetLastError = true)]
  static extern IntPtr MapViewOfFile(IntPtr m, uint access, uint hi, uint lo, UIntPtr n);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
  static IntPtr view = IntPtr.Zero;
  [HandleProcessCorruptedStateExceptions, SecurityCritical]
  static string Read(long off) {
    try {
      var sb = new StringBuilder();
      for (int i = 0; i < 16; i++) sb.Append(Marshal.ReadByte(view, (int)off + i).ToString("x2"));
      return sb.ToString();
    } catch (Exception e) { return "error " + e.GetType().Name; }
  }
  public static void Main() {
    string line;
    while ((line = Console.ReadLine()) != null) {
      // Pervaya stroka mozhet nachinatsya s metki UTF-8 (BOM) ot zapisi roditelya
      int k = 0; while (k < line.Length && (line[k] < 'a' || line[k] > 'z')) k++;
      var p = line.Substring(k).Split(new[] { ' ' }, 2);
      if (p[0] == "map") {
        var f = CreateFileW(p[1], 0x80000000, 7, IntPtr.Zero, 3, 0, IntPtr.Zero);
        if (f == new IntPtr(-1)) { Console.WriteLine("error open " + Marshal.GetLastWin32Error()); continue; }
        var m = CreateFileMappingW(f, IntPtr.Zero, 2, 0, 0, IntPtr.Zero);
        CloseHandle(f);
        if (m == IntPtr.Zero) { Console.WriteLine("error mapping " + Marshal.GetLastWin32Error()); continue; }
        view = MapViewOfFile(m, 4, 0, 0, UIntPtr.Zero);
        CloseHandle(m);
        Console.WriteLine(view == IntPtr.Zero ? "error view " + Marshal.GetLastWin32Error() : "mapped");
      } else if (p[0] == "read") {
        Console.WriteLine(Read(long.Parse(p[1])));
      } else if (p[0] == "quit") { return; }
      else { Console.WriteLine("unknown"); }
      Console.Out.Flush();
    }
  }
}
'@
function StartMapper {
    $psi = New-Object Diagnostics.ProcessStartInfo $mapper
    $psi.UseShellExecute = $false; $psi.RedirectStandardInput = $true; $psi.RedirectStandardOutput = $true; $psi.CreateNoWindow = $true
    [Diagnostics.Process]::Start($psi)
}
# Otvet pomoshchnika; esli on upal na chtenii, "crashed"
function Ask($p, [string]$cmd) {
    if ($p.HasExited) { return "crashed (exit $($p.ExitCode))" }
    $p.StandardInput.WriteLine($cmd); $p.StandardInput.Flush()
    $t = $p.StandardOutput.ReadLineAsync()
    if (-not $t.Wait(20000)) { return 'no answer' }
    if ($null -eq $t.Result) { [void]$p.WaitForExit(5000); return "crashed (exit $($p.ExitCode))" }
    $t.Result
}

Push-Location "$repo\core"
& "$env:USERPROFILE\.cargo\bin\cargo.exe" build -q --release -p zamok-core 2>&1 | Out-Null
& "$env:USERPROFILE\.cargo\bin\cargo.exe" run -q --release --example create_vault -- $src $vault $pw | Out-Null
Pop-Location

$psi = New-Object Diagnostics.ProcessStartInfo $coreExe
$psi.UseShellExecute = $false
$psi.EnvironmentVariables['ZAMOK_DATA_DIR'] = $data
$psi.EnvironmentVariables['ZAMOK_PIPE'] = "\\.\pipe\$pipeName"
$psi.EnvironmentVariables['ZAMOK_NO_TRAY'] = '1'
$psi.EnvironmentVariables['ZAMOK_SEARCH_DRY_RUN'] = '1'
$psi.EnvironmentVariables['ZAMOK_RECENT_DIR'] = "$root\recent"
# Okon net: vopros pered zapiraniem zdes ne nuzhen
$psi.EnvironmentVariables['ZAMOK_UI_EXE'] = "$root\no-window.exe"
$core = [Diagnostics.Process]::Start($psi)
[void](WaitFor { (Test-Path "\\.\pipe\$pipeName") -or $core.HasExited } 15000)
function Send([string]$json) {
    $c = New-Object IO.Pipes.NamedPipeClientStream '.', $pipeName, 'InOut'
    $c.Connect(5000)
    $enc = New-Object Text.UTF8Encoding $false
    $w = New-Object IO.StreamWriter $c, $enc; $w.AutoFlush = $true
    $r = New-Object IO.StreamReader $c, $enc
    $w.WriteLine($json); $line = $r.ReadLine(); $c.Dispose()
    $line | ConvertFrom-Json
}
function J([string]$s) { $s.Replace('\', '\\').Replace('"', '\"') }
function Folder { (Send '{"cmd":"status"}').folders | Where-Object { $_.path -eq $mnt } }

$a = $null; $b = $null
try {
    [void](Send "{`"cmd`":`"add`",`"path`":`"$(J $mnt)`",`"vault`":`"$(J $vault)`"}")
    Check 'folder added and unlocked through core' { (Send "{`"cmd`":`"unlock`",`"path`":`"$(J $mnt)`",`"password`":`"$pw`"}").folder.state -eq 'unlocked' }
    # A otobrazhaet fayl i chitaet nachalo; B otobrazhaet tot zhe fayl i nichego ne chitaet
    $a = StartMapper; $b = StartMapper
    Check 'two programs map the file, file handles closed, only the mappings remain' {
        $ra = Ask $a "map $mnt\data.bin"; $rb = Ask $b "map $mnt\data.bin"
        if ($ra -ne 'mapped' -or $rb -ne 'mapped') { throw "A: $ra, B: $rb" }; 'mapped'
    }
    Check 'before lock: mapped page reads the plaintext' { (Ask $a 'read 0') -eq (Want 0) }
    Check 'a mapping alone counts as an open file, the program is named' {
        $f = Folder
        if ($f.open_files -lt 1 -or @($f.open_in) -notcontains 'mapper.exe') { throw "open_files=$($f.open_files) open_in=$($f.open_in)" }
        "open_files=$($f.open_files) open_in=$($f.open_in -join ',')"
    }
    Check 'idle lock waits while only a mapping holds the file' {
        [void](Send "{`"cmd`":`"set_idle_lock`",`"path`":`"$(J $mnt)`",`"secs`":3}")
        Start-Sleep -Seconds 8
        $st = (Folder).state
        [void](Send "{`"cmd`":`"set_idle_lock`",`"path`":`"$(J $mnt)`",`"secs`":0}")
        $st -eq 'unlocked'
    }
    Check 'manual lock is not refused by a mapped file' {
        $r = Send "{`"cmd`":`"lock`",`"path`":`"$(J $mnt)`"}"
        $r.ok -and $r.folder.state -eq 'locked'
    }
    # Nizhe zapisyvaem, chto proishodit; ozhidaniya sootvetstvuyut opisannomu v DESIGN.md
    $afterA0 = Ask $a 'read 0'
    Check 'after lock: page the program already read stays readable in its memory (residue)' {
        if ($afterA0 -ne (Want 0)) { throw "got: $afterA0" }; 'plaintext still readable'
    }
    $afterB0 = Ask $b 'read 0'
    Check 'after lock: another mapping reads that page from the shared cache (residue)' {
        if ($afterB0 -ne (Want 0)) { throw "got: $afterB0" }; 'plaintext still readable'
    }
    $far = 786432
    $afterAFar = Ask $a "read $far"
    Check 'after lock: page nobody read is not served, the program gets an error or crashes' {
        if ($afterAFar -eq (Want $far)) { throw 'plaintext served after lock' }; $afterAFar
    }
    Check 'after lock: the file cannot be opened again' {
        try { [void][IO.File]::ReadAllBytes("$mnt\data.bin"); $false } catch { 'refused' }
    }
    foreach ($p in $a, $b) { if (-not $p.HasExited) { $p.Kill(); [void]$p.WaitForExit(5000) } }
    Check 'mappings gone: counter back to zero after unlock, file reads normally' {
        $u = Send "{`"cmd`":`"unlock`",`"path`":`"$(J $mnt)`",`"password`":`"$pw`"}"
        $u.folder.state -eq 'unlocked' -and $u.folder.open_files -eq 0 -and
            [Linq.Enumerable]::SequenceEqual([byte[]][IO.File]::ReadAllBytes("$mnt\data.bin"), [byte[]]$bytes)
    }
    # Tolko zapis nablyudeniya: programma vyshla, a Windows eshche derzhit sekciyu fayla v keshe
    Check 'program that mapped the file exited: what the counter shows (observation)' {
        $c = StartMapper
        [void](Ask $c "map $mnt\data.bin"); [void](Ask $c 'read 0'); [void](Ask $c 'quit'); [void]$c.WaitForExit(5000)
        Start-Sleep -Seconds 5
        $f = Folder
        "open_files=$($f.open_files) open_in=$(@($f.open_in) -join ',') 5 s after exit"
    }
}
finally {
    foreach ($p in $a, $b) { if ($p -and -not $p.HasExited) { $p.Kill() } }
    try { [void](Send '{"cmd":"quit"}') } catch {}
    if (-not $core.WaitForExit(15000)) { $core.Kill() }
}
$results | Format-Table -AutoSize -Wrap | Out-String -Width 200
"passed: $(@($results | Where-Object Ok).Count) / $($results.Count)"
