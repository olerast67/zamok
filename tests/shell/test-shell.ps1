# Etap 5: zamok-shell.dll ne chitaet disk i set, kazhdyy vyzov bystree millisekundy; menyu papki
# cherez IContextMenu 1000 raz podryad ne zamedlyaetsya i ne padaet.
#
# 1. DLL gruzitsya v etot process (kak v Provodnik), spisok papok v svoey obshchey pamyati
#    (ZAMOK_SHELL_MAP), poetomu spisok rabotayushchego core ne trogaetsya. Na vremya vyzovov
#    CopyCallback v ntdll stoyat perehvatchiki NtCreateFile, NtOpenFile, NtQueryAttributesFile,
#    NtQueryFullAttributesFile, NtDeviceIoControlFile: cherez nih idet lyuboe otkrytie fayla,
#    kanala ili soketa (soket = fayl \Device\Afd) i lyuboy zapros atributov po imeni. Schitayutsya
#    tolko vyzovy iz potoka testa. Kontrol: posle zamera tot zhe potok otkryvaet fayl i
#    sprashivaet atributy, perehvatchiki dolzhny eto uvidet. Prav administratora ne nuzhno
#    (ETW yadra i Process Monitor bez nih ne rabotayut).
# 2. Menyu papki v tests\tmp\shell: SHParseDisplayName, IShellFolder.GetUIObjectOf(IContextMenu),
#    QueryContextMenu, kak pri pravom klike v Provodnike, 1000 raz. Punkty Zamok berutsya iz
#    HKCU ustanovlennoy versii (tolko chtenie), bez ustanovki proverka propuskaetsya.
#    Provodnik polzovatelya ne trogaetsya: vse v etom processe.
# Zapusk: powershell -NoProfile -Sta -ExecutionPolicy Bypass -File tests\shell\test-shell.ps1
param([int]$Calls = 20000, [int]$Menus = 1000)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$repo = (Resolve-Path "$PSScriptRoot/../..").Path
$root = "$repo\tests\tmp\shell"
$results = New-Object System.Collections.Generic.List[object]
function Check([string]$name, [scriptblock]$body) {
    try { $r = & $body; if ($r -eq $false) { throw 'returned false' }
        $results.Add([pscustomobject]@{ Test = $name; Ok = $true; Note = "$r" }) }
    catch { $results.Add([pscustomobject]@{ Test = $name; Ok = $false; Note = $_.Exception.Message }) }
}
if ([Threading.Thread]::CurrentThread.GetApartmentState() -ne 'STA') { throw 'run with powershell -Sta' }

# Kompanovshchik pishet preduprezhdenie v stderr: cherez cmd, chtoby Stop ego ne prinyal za oshibku
& cmd /c "cd /d `"$repo\core`" && `"$env:USERPROFILE\.cargo\bin\cargo.exe`" build -q --release -p zamok-shell >nul 2>&1"
if ($LASTEXITCODE) { throw 'cargo build failed' }
Remove-Item $root -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$root\plain" | Out-Null
# Svoya kopiya: zagruzhennuyu DLL nelzya perezapisat sleduyushchey sborkoy
$dll = "$root\zamok-shell-test.dll"
Copy-Item "$repo\core\target\release\zamok_shell.dll" $dll

Add-Type -TypeDefinition @'
using System; using System.Diagnostics; using System.Runtime.InteropServices; using System.Collections.Generic;
public static class ShellTest {
  [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern IntPtr LoadLibraryW(string n);
  [DllImport("kernel32.dll", CharSet = CharSet.Ansi)] static extern IntPtr GetProcAddress(IntPtr m, string n);
  [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern IntPtr GetModuleHandleW(string n);
  [DllImport("kernel32.dll")] static extern IntPtr VirtualAlloc(IntPtr a, UIntPtr s, uint t, uint p);
  [DllImport("kernel32.dll")] static extern bool VirtualProtect(IntPtr a, UIntPtr s, uint p, out uint old);
  [DllImport("kernel32.dll")] static extern bool FlushInstructionCache(IntPtr h, IntPtr a, UIntPtr s);
  [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
  [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
  [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
  static extern IntPtr CreateFileMappingW(IntPtr f, IntPtr sa, uint prot, uint hi, uint lo, string name);
  [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr MapViewOfFile(IntPtr m, uint acc, uint hi, uint lo, UIntPtr n);
  [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
  static extern IntPtr CreateFileW(string n, uint a, uint s, IntPtr sa, uint c, uint f, IntPtr t);
  [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern uint GetFileAttributesW(string n);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);

  [UnmanagedFunctionPointer(CallingConvention.StdCall)]
  delegate int GetClassObject(ref Guid clsid, ref Guid iid, out IntPtr ppv);
  [UnmanagedFunctionPointer(CallingConvention.StdCall)]
  delegate int CreateInstance(IntPtr self, IntPtr outer, ref Guid iid, out IntPtr ppv);
  [UnmanagedFunctionPointer(CallingConvention.StdCall)]
  delegate uint CopyCallbackFn(IntPtr self, IntPtr hwnd, uint func, uint flags, IntPtr src, uint sa, IntPtr dest, uint da);

  public static readonly string[] Hooked = { "NtCreateFile", "NtOpenFile", "NtQueryAttributesFile", "NtQueryFullAttributesFile", "NtDeviceIoControlFile" };
  static IntPtr page, tidSlot;
  static IntPtr[] counters = new IntPtr[5];
  static IntPtr hook;
  static CopyCallbackFn call;
  static IntPtr mapView;

  // Spisok papok v obshchey pamyati tak, kak ego publikuet core (shellhook.rs)
  public static void PublishFolders(string name, string list) {
    var m = CreateFileMappingW(new IntPtr(-1), IntPtr.Zero, 4, 0, 64 * 1024, name);
    if (m == IntPtr.Zero) throw new Exception("mapping " + Marshal.GetLastWin32Error());
    mapView = MapViewOfFile(m, 0xF001F, 0, 0, new UIntPtr(64 * 1024));
    var units = list.ToCharArray();
    Marshal.WriteInt32(mapView, 4, units.Length);
    Marshal.Copy(units, 0, IntPtr.Add(mapView, 8), units.Length);
    Marshal.WriteInt32(mapView, 0, 2);
  }

  public static void Load(string path) {
    var lib = LoadLibraryW(path);
    if (lib == IntPtr.Zero) throw new Exception("LoadLibrary " + Marshal.GetLastWin32Error());
    var get = (GetClassObject)Marshal.GetDelegateForFunctionPointer(GetProcAddress(lib, "DllGetClassObject"), typeof(GetClassObject));
    var clsid = new Guid("3F7A9C12-8E4B-4D61-A5C7-19E2B60D4F83");
    var iidFactory = new Guid("00000001-0000-0000-C000-000000000046");
    IntPtr factory;
    int hr = get(ref clsid, ref iidFactory, out factory);
    if (hr != 0) throw new Exception("DllGetClassObject " + hr.ToString("X"));
    var create = (CreateInstance)Marshal.GetDelegateForFunctionPointer(Marshal.ReadIntPtr(Marshal.ReadIntPtr(factory), 3 * IntPtr.Size), typeof(CreateInstance));
    var iidHook = new Guid("000214FC-0000-0000-C000-000000000046");  // ICopyHookW
    hr = create(factory, IntPtr.Zero, ref iidHook, out hook);
    if (hr != 0) throw new Exception("CreateInstance " + hr.ToString("X"));
    call = (CopyCallbackFn)Marshal.GetDelegateForFunctionPointer(Marshal.ReadIntPtr(Marshal.ReadIntPtr(hook), 3 * IntPtr.Size), typeof(CopyCallbackFn));
  }

  static void Emit(List<byte> b, params byte[] x) { b.AddRange(x); }
  static void Emit64(List<byte> b, IntPtr v) { b.AddRange(BitConverter.GetBytes(v.ToInt64())); }

  // Perehvatchik: zaglushka sistemnogo vyzova v ntdll (mov r10,rcx; mov eax,N; ...; syscall)
  // kopiruetsya celikom, na ee meste perehod v kod, kotoryy uvelichivaet schetchik, esli
  // vyzov iz potoka testa (TEB+0x48), i idet v kopiyu.
  public static void InstallHooks() {
    var ntdll = GetModuleHandleW("ntdll.dll");
    page = VirtualAlloc(IntPtr.Zero, new UIntPtr(4096), 0x3000, 0x40);
    tidSlot = page;
    Marshal.WriteInt64(tidSlot, GetCurrentThreadId());
    for (int i = 0; i < Hooked.Length; i++) {
      var fn = GetProcAddress(ntdll, Hooked[i]);
      if (fn == IntPtr.Zero) throw new Exception("no " + Hooked[i]);
      var head = new byte[32]; Marshal.Copy(fn, head, 0, 32);
      if (!(head[0] == 0x4C && head[1] == 0x8B && head[2] == 0xD1 && head[3] == 0xB8 && head[18] == 0x0F && head[19] == 0x05))
        throw new Exception("unexpected stub in " + Hooked[i]);
      counters[i] = IntPtr.Add(page, 16 + i * 8);
      Marshal.WriteInt64(counters[i], 0);
      var copy = IntPtr.Add(page, 256 + i * 256);
      Marshal.Copy(head, 0, copy, 32);
      var thunk = IntPtr.Add(copy, 64);
      var b = new List<byte>();
      Emit(b, 0x65, 0x48, 0x8B, 0x04, 0x25, 0x48, 0, 0, 0);   // mov rax, gs:[48h]
      Emit(b, 0x49, 0xBB); Emit64(b, tidSlot);                 // mov r11, tidSlot
      Emit(b, 0x49, 0x3B, 0x03);                               // cmp rax, [r11]
      Emit(b, 0x75, 0x0E);                                     // jne +14
      Emit(b, 0x49, 0xBB); Emit64(b, counters[i]);             // mov r11, counter
      Emit(b, 0xF0, 0x49, 0xFF, 0x03);                         // lock inc qword [r11]
      Emit(b, 0x48, 0xB8); Emit64(b, copy);                    // mov rax, copy
      Emit(b, 0xFF, 0xE0);                                     // jmp rax
      var code = b.ToArray(); Marshal.Copy(code, 0, thunk, code.Length);
      var jump = new List<byte>(); Emit(jump, 0x48, 0xB8); Emit64(jump, thunk); Emit(jump, 0xFF, 0xE0);
      uint old;
      if (!VirtualProtect(fn, new UIntPtr(16), 0x40, out old)) throw new Exception("protect");
      Marshal.Copy(jump.ToArray(), 0, fn, jump.Count);
      VirtualProtect(fn, new UIntPtr(16), old, out old);
      FlushInstructionCache(GetCurrentProcess(), fn, new UIntPtr(16));
    }
  }

  public static long[] Counts() {
    var r = new long[Hooked.Length];
    for (int i = 0; i < r.Length; i++) r[i] = Marshal.ReadInt64(counters[i]);
    return r;
  }

  // Odin vyzov CopyCallback
  public static uint Call(uint func, uint flags, string src, string dest) {
    var s = Marshal.StringToHGlobalUni(src); var d = Marshal.StringToHGlobalUni(dest);
    try { return call(hook, IntPtr.Zero, func, flags, s, 0x10, d, 0x10); }
    finally { Marshal.FreeHGlobal(s); Marshal.FreeHGlobal(d); }
  }

  // Zamer: n vyzovov po krugu iz sluchaev, stroki zaraneye v pamyati. Vozvrashchaet vremena
  // vyzovov v mikrosekundah i proveryaet otvety.
  // Vtoroy massiv: processornoe vremya potoka na vyzov (QueryThreadCycleTime), bez vremeni,
  // kogda Windows otdala processor drugim programmam.
  public static double[] CpuTimes;
  [DllImport("kernel32.dll")] static extern bool QueryThreadCycleTime(IntPtr t, out ulong c);
  [DllImport("kernel32.dll")] static extern IntPtr GetCurrentThread();
  static double CyclesPerUs() {
    ulong c0, c1; var sw = Stopwatch.StartNew();
    QueryThreadCycleTime(GetCurrentThread(), out c0);
    while (sw.ElapsedMilliseconds < 100) { }
    QueryThreadCycleTime(GetCurrentThread(), out c1);
    return (c1 - c0) / (sw.Elapsed.TotalMilliseconds * 1000);
  }
  public static double[] Measure(int n, uint[] funcs, string[] srcs, string[] dests, uint[] want) {
    var s = new IntPtr[srcs.Length]; var d = new IntPtr[srcs.Length];
    for (int i = 0; i < s.Length; i++) { s[i] = Marshal.StringToHGlobalUni(srcs[i]); d[i] = Marshal.StringToHGlobalUni(dests[i]); }
    var times = new double[n];
    CpuTimes = new double[n];
    var freq = (double)Stopwatch.Frequency;
    var perUs = CyclesPerUs();
    var th = GetCurrentThread();
    try {
      for (int k = 0; k < n; k++) {
        int c = k % s.Length;
        ulong c0, c1;
        QueryThreadCycleTime(th, out c0);
        long t0 = Stopwatch.GetTimestamp();
        uint r = call(hook, IntPtr.Zero, funcs[c], 0x400, s[c], 0x10, d[c], 0x10);
        long t1 = Stopwatch.GetTimestamp();
        QueryThreadCycleTime(th, out c1);
        if (r != want[c]) throw new Exception("case " + c + ": answer " + r);
        times[k] = (t1 - t0) * 1e6 / freq;
        CpuTimes[k] = (c1 - c0) / perUs;
      }
    } finally {
      for (int i = 0; i < s.Length; i++) { Marshal.FreeHGlobal(s[i]); Marshal.FreeHGlobal(d[i]); }
    }
    return times;
  }

  // Kontrol perehvatchikov: tot zhe potok otkryvaet fayl i sprashivaet atributy
  public static void TouchFile(string path) {
    var h = CreateFileW(path, 0x80000000, 7, IntPtr.Zero, 3, 0, IntPtr.Zero);
    if (h != new IntPtr(-1)) CloseHandle(h);
    GetFileAttributesW(path);
  }
}
'@

$map = "Local\ZamokShellTest-$PID"
$env:ZAMOK_SHELL_MAP = $map
# Vydumannye papki Zamok: odna na C:, odna na D:
$zamok = 'C:\Work\Clients'; $zamok2 = 'D:\Archive\Taxes 2025'
[ShellTest]::PublishFolders($map, "$zamok`n$zamok2")
$IDYES = 6; $IDNO = 7; $MOVE = 1; $COPY = 2

try {
    [ShellTest]::Load($dll)
    Check 'DLL: move of a Zamok folder to another drive is refused' { [ShellTest]::Call($MOVE, 0x400, $zamok, 'E:\Clients') -eq $IDNO }
    Check 'DLL: move of a parent folder to another drive is refused' { [ShellTest]::Call($MOVE, 0x400, 'C:\Work', 'E:\Work') -eq $IDNO }
    Check 'DLL: other folders and copies pass' {
        ([ShellTest]::Call($MOVE, 0x400, 'C:\Work\Other', 'E:\Other') -eq $IDYES) -and
            ([ShellTest]::Call($COPY, 0x400, $zamok, 'E:\Clients') -eq $IDYES) -and
            ([ShellTest]::Call($MOVE, 0x400, $zamok, 'C:\Elsewhere\Clients') -eq $IDYES)
    }

    $funcs = [uint32[]]@($MOVE, $MOVE, $MOVE, $MOVE, $COPY, $MOVE)
    $srcs = [string[]]@($zamok, 'C:\Work', 'C:\Work\Other', $zamok, $zamok2, $zamok2)
    $dests = [string[]]@('E:\Clients', 'E:\Work', 'E:\Other', 'C:\Moved\Clients', 'E:\Taxes', '\\server\share\Taxes')
    $want = [uint32[]]@($IDNO, $IDNO, $IDYES, $IDYES, $IDYES, $IDNO)
    [void][ShellTest]::Measure(2000, $funcs, $srcs, $dests, $want)   # progrev

    [ShellTest]::InstallHooks()
    $before = [ShellTest]::Counts()
    # Po chasam na stene vyzov mozhet popast na pereklyuchenie potokov v sisteme (Windows otdala
    # processor drugoy programme): do 3 popytok. Rabota samoy DLL meryaetsya processornym
    # vremenem potoka, tam takih vybrosov net.
    $times = $null
    for ($try = 1; $try -le 3; $try++) {
        $times = [ShellTest]::Measure($Calls, $funcs, $srcs, $dests, $want)
        if (($times | Measure-Object -Maximum).Maximum -lt 1000) { break }
    }
    $cpu = [ShellTest]::CpuTimes
    $during = [ShellTest]::Counts()
    [ShellTest]::TouchFile("$repo\ROADMAP.md")
    $after = [ShellTest]::Counts()
    $sorted = $times | Sort-Object
    $max = $sorted[-1]; $p999 = $sorted[[int]($sorted.Count * 0.999)]; $avg = ($times | Measure-Object -Average).Average
    $cpuMax = ($cpu | Measure-Object -Maximum).Maximum
    $stat = 'calls {0}, wall avg {1:0.00} us, p99.9 {2:0.0} us, max {3:0.0} us; thread CPU max {4:0.0} us' -f $times.Count, $avg, $p999, $max, $cpuMax
    Check 'DLL: every CopyCallback call is under 1 ms' { if ($cpuMax -ge 1000 -or $p999 -ge 1000) { throw $stat }; $stat }
    Check 'DLL: no file, pipe or socket is opened and no attributes are asked during the calls' {
        $diff = @(for ($i = 0; $i -lt $before.Count; $i++) { if ($during[$i] -ne $before[$i]) { "$([ShellTest]::Hooked[$i])=$($during[$i] - $before[$i])" } })
        if ($diff) { throw ($diff -join ', ') }
        'hooked: ' + ([ShellTest]::Hooked -join ', ')
    }
    Check 'DLL: the hooks do see a file open from the same thread (control)' {
        ($after[0] -gt $during[0]) -and ($after[2] + $after[3] -gt $during[2] + $during[3])
    }
    Check 'DLL: no network library is loaded' {
        $net = @((Get-Process -Id $PID).Modules | Where-Object { $_.ModuleName -match '^(ws2_32|wininet|winhttp|mswsock|dnsapi)\.dll$' } | ForEach-Object ModuleName)
        # PowerShell sam mog zagruzit chto-to iz nih ranshe: smotrim, chto DLL ih ne importiruet
        $bytes = [IO.File]::ReadAllBytes($dll)
        $text = [Text.Encoding]::ASCII.GetString($bytes)
        $imp = @('ws2_32', 'wininet', 'winhttp', 'mswsock', 'dnsapi', 'urlmon') | Where-Object { $text -match "(?i)$_\.dll" }
        if ($imp) { throw "imports $($imp -join ', ')" }
        "process has: $($net -join ', ')"
    }
}
catch { $results.Add([pscustomobject]@{ Test = 'DLL test setup'; Ok = $false; Note = $_.Exception.Message }) }

# 2. Menyu papki 1000 raz
Add-Type -TypeDefinition @'
using System; using System.Diagnostics; using System.Runtime.InteropServices; using System.Text; using System.Collections.Generic;
[ComImport, Guid("000214E6-0000-0000-C000-000000000046"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IShellFolderX {
  void ParseDisplayName(); void EnumObjects(); void BindToObject(); void BindToStorage(); void CompareIDs(); void CreateViewObject(); void GetAttributesOf();
  [PreserveSig] int GetUIObjectOf(IntPtr hwnd, uint cidl, [MarshalAs(UnmanagedType.LPArray)] IntPtr[] apidl, ref Guid riid, IntPtr rgf, out IntPtr ppv);
}
public static class MenuTest {
  [DllImport("shell32.dll", CharSet = CharSet.Unicode)] static extern int SHParseDisplayName(string name, IntPtr bc, out IntPtr pidl, uint sfgao, out uint attrs);
  [DllImport("shell32.dll")] static extern int SHBindToParent(IntPtr pidl, ref Guid riid, [MarshalAs(UnmanagedType.Interface)] out IShellFolderX ppv, out IntPtr last);
  [DllImport("user32.dll")] static extern IntPtr CreatePopupMenu();
  [DllImport("user32.dll")] static extern bool DestroyMenu(IntPtr m);
  [DllImport("user32.dll")] static extern int GetMenuItemCount(IntPtr m);
  [DllImport("user32.dll")] static extern uint GetMenuItemID(IntPtr m, int pos);
  [DllImport("user32.dll")] static extern IntPtr GetSubMenu(IntPtr m, int pos);
  [DllImport("ole32.dll")] static extern void CoTaskMemFree(IntPtr p);
  [DllImport("user32.dll")] static extern uint GetGuiResources(IntPtr p, uint flags);
  [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
  [UnmanagedFunctionPointer(CallingConvention.StdCall)]
  delegate int QueryContextMenuFn(IntPtr self, IntPtr menu, uint index, uint first, uint last, uint flags);
  [UnmanagedFunctionPointer(CallingConvention.StdCall, CharSet = CharSet.Unicode)]
  delegate int GetCommandStringFn(IntPtr self, UIntPtr cmd, uint type, IntPtr reserved, StringBuilder name, uint cch);

  // Odin pravyy klik: menyu papki kak v Provodnike, spisok glagolov punktov (s podmenyu)
  public static List<string> Once(string path) {
    IntPtr pidl; uint a;
    int hr = SHParseDisplayName(path, IntPtr.Zero, out pidl, 0, out a);
    if (hr != 0) throw new Exception("parse " + hr.ToString("X"));
    var verbs = new List<string>();
    try {
      IShellFolderX parent; IntPtr last;
      var iidFolder = new Guid("000214E6-0000-0000-C000-000000000046");
      hr = SHBindToParent(pidl, ref iidFolder, out parent, out last);
      if (hr != 0) throw new Exception("bind " + hr.ToString("X"));
      var iidMenu = new Guid("000214E4-0000-0000-C000-000000000046");
      IntPtr cm;
      hr = parent.GetUIObjectOf(IntPtr.Zero, 1, new[] { last }, ref iidMenu, IntPtr.Zero, out cm);
      Marshal.ReleaseComObject(parent);
      if (hr != 0) throw new Exception("menu " + hr.ToString("X"));
      try {
        var vt = Marshal.ReadIntPtr(cm);
        var query = (QueryContextMenuFn)Marshal.GetDelegateForFunctionPointer(Marshal.ReadIntPtr(vt, 3 * IntPtr.Size), typeof(QueryContextMenuFn));
        var getCmd = (GetCommandStringFn)Marshal.GetDelegateForFunctionPointer(Marshal.ReadIntPtr(vt, 5 * IntPtr.Size), typeof(GetCommandStringFn));
        var menu = CreatePopupMenu();
        try {
          hr = query(cm, menu, 0, 1, 0x7FFF, 0);   // CMF_NORMAL
          if (hr < 0) throw new Exception("query " + hr.ToString("X"));
          Collect(menu, cm, getCmd, verbs);
        } finally { DestroyMenu(menu); }
      } finally { Marshal.Release(cm); }
    } finally { CoTaskMemFree(pidl); }
    return verbs;
  }

  static void Collect(IntPtr menu, IntPtr cm, GetCommandStringFn getCmd, List<string> verbs) {
    int n = GetMenuItemCount(menu);
    for (int i = 0; i < n; i++) {
      var sub = GetSubMenu(menu, i);
      if (sub != IntPtr.Zero) { Collect(sub, cm, getCmd, verbs); continue; }
      uint id = GetMenuItemID(menu, i);
      if (id == 0 || id == 0xFFFFFFFF) continue;
      var sb = new StringBuilder(256);
      try { if (getCmd(cm, new UIntPtr(id - 1), 4, IntPtr.Zero, sb, 256) == 0) verbs.Add(sb.ToString()); } catch { }   // GCS_VERBW
    }
  }

  public static uint GdiObjects() { return GetGuiResources(GetCurrentProcess(), 0); }
  public static uint UserObjects() { return GetGuiResources(GetCurrentProcess(), 1); }
}
'@

$verbsInstalled = Test-Path 'HKCU:\Software\Classes\Directory\shell\ZamokEncrypt'
if (-not $verbsInstalled) {
    $results.Add([pscustomobject]@{ Test = 'menu: 1000 right-clicks'; Ok = $true; Note = 'SKIPPED: Zamok menu items are not installed' })
}
else {
    try {
        $folder = "$root\plain"
        $first = [MenuTest]::Once($folder)
        Check 'menu: the folder menu has the Zamok item' { if ($first -notcontains 'ZamokEncrypt') { throw "verbs: $($first -join ',')" }; "$($first.Count) items" }
        [GC]::Collect(); [GC]::WaitForPendingFinalizers()
        $p = Get-Process -Id $PID
        $mem0 = $p.PrivateMemorySize64; $h0 = $p.HandleCount; $gdi0 = [MenuTest]::GdiObjects(); $usr0 = [MenuTest]::UserObjects()
        $times = New-Object double[] $Menus
        $missing = 0
        $sw = New-Object Diagnostics.Stopwatch
        for ($i = 0; $i -lt $Menus; $i++) {
            $sw.Restart()
            $v = [MenuTest]::Once($folder)
            $sw.Stop()
            $times[$i] = $sw.Elapsed.TotalMilliseconds
            if ($v -notcontains 'ZamokEncrypt') { $missing++ }
        }
        [GC]::Collect(); [GC]::WaitForPendingFinalizers()
        $p.Refresh()
        $tenth = [int]($Menus / 5)
        $early = ($times[0..($tenth - 1)] | Sort-Object)[[int]($tenth / 2)]
        $late = ($times[($Menus - $tenth)..($Menus - 1)] | Sort-Object)[[int]($tenth / 2)]
        $all = ($times | Measure-Object -Average -Maximum)
        $memMb = ($p.PrivateMemorySize64 - $mem0) / 1MB
        $stat = 'median first 20% {0:0.00} ms, last 20% {1:0.00} ms, avg {2:0.00} ms, max {3:0.0} ms' -f $early, $late, $all.Average, $all.Maximum
        Check "menu: $Menus right-clicks in a row, every menu has the Zamok item" { if ($missing) { throw "missing in $missing" }; $true }
        Check 'menu: no slowdown (median of the last 20% within 1.5x of the first 20%)' { if ($late -gt [Math]::Max($early * 1.5, $early + 2)) { throw $stat }; $stat }
        Check 'menu: no leak of handles, GDI and USER objects or memory' {
            $s = 'handles {0:+0;-0;0}, GDI {1:+0;-0;0}, USER {2:+0;-0;0}, private memory {3:+0.0;-0.0;0} MB' -f ($p.HandleCount - $h0), ([MenuTest]::GdiObjects() - $gdi0), ([MenuTest]::UserObjects() - $usr0), $memMb
            if (($p.HandleCount - $h0) -gt 100 -or ([MenuTest]::GdiObjects() - $gdi0) -gt 50 -or ([MenuTest]::UserObjects() - $usr0) -gt 50 -or $memMb -gt 50) { throw $s }
            $s
        }
    }
    catch { $results.Add([pscustomobject]@{ Test = 'menu test'; Ok = $false; Note = $_.Exception.Message }) }
}

$results | Format-Table -AutoSize -Wrap | Out-String -Width 200
"passed: $(@($results | Where-Object Ok).Count) / $($results.Count)"
if (@($results | Where-Object { -not $_.Ok }).Count) { exit 1 }
