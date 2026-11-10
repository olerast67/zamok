# Snimki okon dlya README i sayta: docs/images/screens/<ru|en>. Nastoyashchiy Zamok.exe s
# nastoyashchim zamok-core, core so svoey papkoy dannyh i svoim kanalom (kak test-ui.ps1).
# Papki vydumannye: C:\Demo\Documents, Projects, Photos. Skript sozdaet C:\Demo, tolko esli ee
# net, i udalyaet v kontse. Okna snimayutsya PrintWindow: chuzhie okna poverh ne meshayut.
# Menyu Provodnika snimaetsya v etom processe (IContextMenu ustanovlennoy versii), Provodnik
# polzovatelya ne trogaetsya.
# Zapusk: powershell -NoProfile -Sta -ExecutionPolicy Bypass -File tests\ui\screens.ps1 -Lang ru
# -Theme light ili dark: tema okon nezavisimo ot Windows (ZAMOK_THEME), dlya proverki kontrasta;
# s nim menyu Provodnika ne snimaetsya, snimki luchshe klast v svoyu papku (-Out).
param([ValidateSet('ru', 'en')][string]$Lang = 'ru', [string]$Out = '', [ValidateSet('', 'light', 'dark')][string]$Theme = '')
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$repo = (Resolve-Path "$PSScriptRoot/../..").Path
if (-not $Out) { $Out = "$repo\docs\images\screens\$Lang" }
$demo = 'C:\Demo'
$root = "$repo\tests\tmp\screens"
$coreExe = "$repo\core\target\release\zamok-core.exe"
$pipeName = "zamok-screens-$PID"
$pw = 'Korova letit na lunu 2026'
if (Test-Path $demo) { throw "$demo already exists: not touching it" }

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Drawing, System.Windows.Forms
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System; using System.Drawing; using System.Drawing.Imaging; using System.Runtime.InteropServices; using System.Text; using System.Threading;
public static class Shot {
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
  [DllImport("user32.dll")] static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
  [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr h, int a, out RECT r, int size);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  // Okno celikom bez nevidimyh ramok (DWMWA_EXTENDED_FRAME_BOUNDS), dazhe esli ego zakryvayut drugie
  public static void Window(IntPtr h, string path) {
    RECT w, f; GetWindowRect(h, out w); DwmGetWindowAttribute(h, 9, out f, 16);
    using (var bmp = new Bitmap(w.R - w.L, w.B - w.T, PixelFormat.Format32bppArgb)) {
      using (var g = Graphics.FromImage(bmp)) { var dc = g.GetHdc(); PrintWindow(h, dc, 2); g.ReleaseHdc(dc); }
      using (var crop = bmp.Clone(new Rectangle(f.L - w.L, f.T - w.T, f.R - f.L, f.B - f.T), PixelFormat.Format24bppRgb)) crop.Save(path, ImageFormat.Png);
    }
  }

  [ComImport, Guid("000214E6-0000-0000-C000-000000000046"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IShellFolderX { void ParseDisplayName(); void EnumObjects(); void BindToObject(); void BindToStorage(); void CompareIDs(); void CreateViewObject(); void GetAttributesOf();
    [PreserveSig] int GetUIObjectOf(IntPtr hwnd, uint cidl, [MarshalAs(UnmanagedType.LPArray)] IntPtr[] apidl, ref Guid riid, IntPtr rgf, out IntPtr ppv); }
  [DllImport("shell32.dll", CharSet = CharSet.Unicode)] static extern int SHParseDisplayName(string name, IntPtr bc, out IntPtr pidl, uint sfgao, out uint attrs);
  [DllImport("shell32.dll")] static extern int SHBindToParent(IntPtr pidl, ref Guid riid, [MarshalAs(UnmanagedType.Interface)] out IShellFolderX ppv, out IntPtr last);
  [DllImport("user32.dll")] static extern IntPtr CreatePopupMenu();
  [DllImport("user32.dll")] static extern bool DestroyMenu(IntPtr m);
  [DllImport("user32.dll")] static extern int GetMenuItemCount(IntPtr m);
  [DllImport("user32.dll")] static extern uint GetMenuItemID(IntPtr m, int pos);
  [DllImport("user32.dll")] static extern bool GetMenuItemRect(IntPtr h, IntPtr m, uint pos, out RECT r);
  [DllImport("user32.dll")] static extern int TrackPopupMenuEx(IntPtr m, uint flags, int x, int y, IntPtr h, IntPtr p);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern IntPtr FindWindowW(string cls, string name);
  [DllImport("user32.dll")] static extern bool PostMessageW(IntPtr h, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr h);
  [UnmanagedFunctionPointer(CallingConvention.StdCall)] delegate int QueryFn(IntPtr self, IntPtr menu, uint index, uint first, uint last, uint flags);
  [UnmanagedFunctionPointer(CallingConvention.StdCall, CharSet = CharSet.Unicode)] delegate int VerbFn(IntPtr self, UIntPtr cmd, uint type, IntPtr res, StringBuilder name, uint cch);

  // Menyu papki kak po pravomu kliku; snimok stroki s glagolom verb. Menyu zakryvaetsya Esc.
  public static string MenuItem(string folder, string verb, IntPtr owner, int x, int y, string path) {
    IntPtr pidl; uint a;
    if (SHParseDisplayName(folder, IntPtr.Zero, out pidl, 0, out a) != 0) return "parse";
    IShellFolderX parent; IntPtr last; var iidF = new Guid("000214E6-0000-0000-C000-000000000046");
    SHBindToParent(pidl, ref iidF, out parent, out last);
    var iidM = new Guid("000214E4-0000-0000-C000-000000000046"); IntPtr cm;
    if (parent.GetUIObjectOf(owner, 1, new[] { last }, ref iidM, IntPtr.Zero, out cm) != 0) return "menu";
    var vt = Marshal.ReadIntPtr(cm);
    var query = (QueryFn)Marshal.GetDelegateForFunctionPointer(Marshal.ReadIntPtr(vt, 3 * IntPtr.Size), typeof(QueryFn));
    var getVerb = (VerbFn)Marshal.GetDelegateForFunctionPointer(Marshal.ReadIntPtr(vt, 5 * IntPtr.Size), typeof(VerbFn));
    var menu = CreatePopupMenu();
    query(cm, menu, 0, 1, 0x7FFF, 0);
    int pos = -1;
    for (int i = 0; i < GetMenuItemCount(menu); i++) {
      uint id = GetMenuItemID(menu, i); if (id == 0 || id == 0xFFFFFFFF) continue;
      var sb = new StringBuilder(256);
      if (getVerb(cm, new UIntPtr(id - 1), 4, IntPtr.Zero, sb, 256) == 0 && sb.ToString() == verb) { pos = i; break; }
    }
    if (pos < 0) return "no item " + verb;
    string result = "timeout";
    var t = new Thread(() => {
      Thread.Sleep(1500);
      RECT r;
      if (GetMenuItemRect(owner, menu, (uint)pos, out r)) {
        using (var bmp = new Bitmap(r.R - r.L, r.B - r.T, PixelFormat.Format24bppRgb)) {
          using (var g = Graphics.FromImage(bmp)) g.CopyFromScreen(r.L, r.T, 0, 0, bmp.Size);
          bmp.Save(path, ImageFormat.Png);
        }
        result = "ok";
      } else result = "rect";
      var mw = FindWindowW("#32768", null);
      PostMessageW(mw, 0x100, new IntPtr(0x1B), IntPtr.Zero);
      PostMessageW(mw, 0x101, new IntPtr(0x1B), IntPtr.Zero);
    });
    t.Start();
    SetForegroundWindow(owner);
    TrackPopupMenuEx(menu, 0x100 | 0x80, x, y, owner, IntPtr.Zero);   // TPM_RETURNCMD | TPM_NONOTIFY
    t.Join();
    DestroyMenu(menu); Marshal.Release(cm); Marshal.ReleaseComObject(parent);
    return result;
  }
}
'@
# Snimki v nastoyashchih tochkah ekrana, bez rastyagivaniya
[void][Shot]::SetProcessDpiAwarenessContext([IntPtr]-4)

$AE = [Windows.Automation.AutomationElement]
function Window([int]$procId) {
    $cond = New-Object Windows.Automation.PropertyCondition ($AE::ProcessIdProperty), $procId
    $w = $null; $deadline = (Get-Date).AddSeconds(15)
    while (-not $w -and (Get-Date) -lt $deadline) { $w = $AE::RootElement.FindFirst('Children', $cond); if (-not $w) { Start-Sleep -Milliseconds 200 } }
    $w
}
function El($w, [string]$id) { $w.FindFirst('Descendants', (New-Object Windows.Automation.PropertyCondition ($AE::AutomationIdProperty), $id)) }
function SetValue($w, $id, $v) { (El $w $id).GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).SetValue($v) }
function Press($w, $id) { (El $w $id).GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke() }
function WaitFor([scriptblock]$c, [int]$ms = 20000) { $d = (Get-Date).AddMilliseconds($ms); while (-not $(try { & $c } catch { $false })) { if ((Get-Date) -gt $d) { throw "timeout: $c" }; Start-Sleep -Milliseconds 100 } }
function Send([string]$json) {
    $c = New-Object IO.Pipes.NamedPipeClientStream '.', $pipeName, 'InOut'; $c.Connect(5000)
    $enc = New-Object Text.UTF8Encoding $false
    $wr = New-Object IO.StreamWriter $c, $enc; $wr.AutoFlush = $true; $r = New-Object IO.StreamReader $c, $enc
    $wr.WriteLine($json); $line = $r.ReadLine(); $c.Dispose(); $line | ConvertFrom-Json
}
function J([string]$s) { $s.Replace('\', '\\') }
function StartUi([string]$a, [string]$pipe = $pipeName) {
    $p = New-Object Diagnostics.ProcessStartInfo $uiExe; $p.UseShellExecute = $false; $p.Arguments = $a
    $p.EnvironmentVariables['ZAMOK_PIPE'] = $pipe; $p.EnvironmentVariables['ZAMOK_LANG'] = $Lang
    if ($Theme) { $p.EnvironmentVariables['ZAMOK_THEME'] = $Theme }
    [Diagnostics.Process]::Start($p)
}
function Snap($w, [string]$name) {
    Start-Sleep -Milliseconds 800
    # Okno eshche ne poluchilo razmer (pustoy pryamougolnik): eshche raz cherez polsekundy
    foreach ($i in 1..10) {
        try { [Shot]::Window([IntPtr]$w.Current.NativeWindowHandle, "$Out\$name.png"); break }
        catch { if ($i -eq 10) { throw }; Start-Sleep -Milliseconds 500 }
    }
    "saved $Lang/$name.png"
}
function Sample([string]$dir) {
    New-Item -ItemType Directory -Force $dir | Out-Null
    [IO.File]::WriteAllText("$dir\notes.txt", 'sample')
}

Remove-Item $root -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $root, $Out | Out-Null
& dotnet publish "$repo\ui\Zamok.UI\Zamok.UI.csproj" -c Release -o "$root-publish" -v q 2>&1 | Out-Null
$uiExe = "$root-publish\Zamok.exe"
$psi = New-Object Diagnostics.ProcessStartInfo $coreExe
$psi.UseShellExecute = $false
$psi.EnvironmentVariables['ZAMOK_DATA_DIR'] = "$root\data"
$psi.EnvironmentVariables['ZAMOK_PIPE'] = "\\.\pipe\$pipeName"
$psi.EnvironmentVariables['ZAMOK_NO_TRAY'] = '1'
$psi.EnvironmentVariables['ZAMOK_SEARCH_DRY_RUN'] = '1'
$psi.EnvironmentVariables['ZAMOK_LANG'] = $Lang
$core = [Diagnostics.Process]::Start($psi)
WaitFor { Test-Path "\\.\pipe\$pipeName" }
New-Item -ItemType Directory $demo | Out-Null
try {
    $doc = "$demo\Documents"
    foreach ($n in 'Documents', 'Projects', 'Photos') { Sample "$demo\$n" }

    # Menyu Provodnika (tolko russkiy snimok, kak ran'she): punkt ustanovlennoy versii
    if ($Lang -eq 'ru' -and -not $Theme -and (Test-Path 'HKCU:\Software\Classes\Directory\shell\ZamokEncrypt')) {
        $form = New-Object Windows.Forms.Form -Property @{ ShowInTaskbar = $false; Opacity = 0; StartPosition = 'Manual'; Location = '300,300'; Size = '10,10' }
        $form.Show()
        "menu: $([Shot]::MenuItem($doc, 'ZamokEncrypt', $form.Handle, 320, 320, "$Out\explorer-menu.png"))"
        $form.Close()
    }

    # Master: shag s parolem i klyuch
    $ui = StartUi "--encrypt `"$doc`""
    $w = Window $ui.Id
    SetValue $w 'Password1' $pw; SetValue $w 'Password2' $pw
    (El $w 'Password2').SetFocus()
    Snap $w 'encrypt-password'
    WaitFor { (El $w 'NextButton').Current.IsEnabled }
    Press $w 'NextButton'
    WaitFor { (El $w 'KeyWords').Current.Name -match '44\.' }
    # Klyuch sluchaynyy: slovo, kotoroe otvlekalo by na snimke, daet novyy klyuch
    $words = @([regex]::Matches((El $w 'KeyWords').Current.Name, '\d+\.\s+(\S+)') | ForEach-Object { $_.Groups[1].Value })
    $odd = @($words | Where-Object { $_ -match '^(lesbian|gay|sex|sexy|sexual|naked|nude|kill|killer|murder|dead|death|die|suicide|drug|drugs|bomb|gun|weapon|war|nazi|slave|abuse|blood|porn|racist|rape|terror|virus|hack|steal|assault|lethal|cancer|hostage|inmate|imprison|prison|violent|crime|criminal|attack|shoot|bullet|toxic|poison|disease|funeral|grave|victim|enemy|danger|shocking)$' })
    if ($odd) { $script:again = $true; throw "key has '$($odd -join ', ')', taking a new one" }
    WaitFor { -not (El $w 'CancelButton').Current.IsEnabled }
    Snap $w 'encrypt-recovery-key'
    $ui.Kill()
    WaitFor { ((Send '{"cmd":"status"}').folders | Where-Object { $_.path -eq $doc }).state -eq 'unlocked' } 60000

    # Eshche dve papki, zapertye
    foreach ($n in 'Projects', 'Photos') {
        # Proshlaya rabota mozhet eshche zavershatsya (job_running): povtor
        WaitFor { (Send "{`"cmd`":`"encrypt`",`"path`":`"$(J "$demo\$n")`",`"password`":`"$pw`"}").ok } 60000
        WaitFor { ((Send '{"cmd":"status"}').folders | Where-Object { $_.path -eq "$demo\$n" }).state -eq 'unlocked' } 60000
        [void](Send "{`"cmd`":`"lock`",`"path`":`"$(J "$demo\$n")`"}")
    }
    # Predlozhenie pro Defender na snimke ne nuzhno: otvechaem srazu
    $offer = @((Send '{"cmd":"status"}').settings.defender_offer)
    if ($offer) { [void](Send ('{"cmd":"defender_offer_done","vaults":[' + (($offer | ForEach-Object { '"' + (J $_) + '"' }) -join ',') + ']}')) }

    $ui = StartUi '--settings'
    $w = Window $ui.Id
    WaitFor { (El $w 'DetailName').Current.Name -eq 'Documents' }
    (El $w 'ToggleButton').SetFocus()
    Snap $w 'settings'
    $ui.Kill()

    # Proverka sledov: zhdem, poka stroki zapolnyatsya
    $ui = StartUi '--traces'
    $w = Window $ui.Id
    WaitFor { (El $w 'ThumbsStatus').Current.Name -and (El $w 'RecheckButton').Current.IsEnabled }
    Snap $w 'traces'
    $ui.Kill()

    # Smena parolya i rasshifrovka: pervyy shag
    $ui = StartUi "--change-password `"$doc`""
    $w = Window $ui.Id
    WaitFor { El $w 'OkButton' }
    Snap $w 'change-password'
    $ui.Kill()
    $ui = StartUi "--decrypt `"$doc`""
    $w = Window $ui.Id
    WaitFor { El $w 'OkButton' }
    Snap $w 'decrypt'
    $ui.Kill()

    # Vopros posle udaleniya papki: core o papke ne sprashivaet, i okno zakrylos by samo, poetomu
    # kanal bez core, okno ostaetsya so strokoy oshibki (zaodno ee cvet na snimke)
    $ui = StartUi "--deleted `"$doc`"" 'zamok-nobody-listens-here'
    $w = Window $ui.Id
    WaitFor { (El $w 'ErrorText').Current.Name }
    Snap $w 'deleted'
    $ui.Kill()

    # Okno parolya na zapertoy papke
    [void](Send "{`"cmd`":`"lock`",`"path`":`"$(J $doc)`"}")
    $ui = StartUi "--unlock `"$doc`""
    $w = Window $ui.Id
    WaitFor { (El $w 'OpenButton').Current.IsEnabled }
    Snap $w 'unlock'
    $ui.Kill()

    # Pokazat klyuch
    $ui = StartUi "--show-key `"$doc`""
    $w = Window $ui.Id
    SetValue $w 'Password' $pw; Start-Sleep -Milliseconds 300; Press $w 'OkButton'
    WaitFor { (El $w 'KeyWords').Current.Name -match '44\.' }
    (El $w 'OkButton').SetFocus()
    Snap $w 'recovery-key'
    $ui.Kill()
}
catch {
    if (-not $script:again) { throw }
    Write-Host $_.Exception.Message
}
finally {
    Get-Process Zamok -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $uiExe } | Stop-Process -Force
    try { [void](Send '{"cmd":"quit"}') } catch {}
    if (-not $core.WaitForExit(20000)) { $core.Kill(); Start-Sleep 2 }
    # Tochki podklyucheniya ushli vmeste s core: udalyaem papku celikom
    Remove-Item $demo -Recurse -Force -ErrorAction SilentlyContinue
    if (Test-Path $demo) { Write-Warning "$demo is left, remove it by hand" }
}
# Novyy klyuch: vsyo snachala (C:\Demo uzhe ubrana)
if ($script:again) {
    & powershell -NoProfile -Sta -ExecutionPolicy Bypass -File $PSCommandPath -Lang $Lang -Out $Out @(if ($Theme) { '-Theme', $Theme })
    exit $LASTEXITCODE
}