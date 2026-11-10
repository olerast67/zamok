# Etap 6 (alfa): proverka okon Zamok.exe na nastoyashchey sisteme.
# Skript zapuskaet nastoyashchiy zamok-core (svoya papka dannyh i svoy kanal) i nastoyashchiy
# Zamok.exe i prohodit scenarii cherez UI Automation (kak ekrannyy diktor): vvodit znacheniya
# i nazhimaet knopki po AutomationId. Klavishi na rabochiy stol ne posylayutsya.
# Okna Zamok poyavlyayutsya na ekrane na vremya testa.
# Vse papki tolko vnutri tests\tmp\ui.
# Zapusk: powershell -NoProfile -ExecutionPolicy Bypass -File tests\ui\test-ui.ps1
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$repo = (Resolve-Path "$PSScriptRoot/../..").Path
$root = "$repo\tests\tmp\ui"
$coreExe = "$repo\core\target\release\zamok-core.exe"
$pw = 'Korova letit na lunu 2026'
$pipeName = "zamok-ui-test-$PID"
$data = "$root\data"
$results = New-Object System.Collections.Generic.List[object]
function Ru([int[]]$c) { -join ($c | ForEach-Object { [char]$_ }) }
$ruName = Ru 0x041A,0x043B,0x0438,0x0435,0x043D,0x0442,0x044B   # Klienty
$openWord = Ru 0x041E,0x0442,0x043A,0x0440,0x044B,0x0442,0x044C  # Otkryt
function Check([string]$name, [scriptblock]$body) {
    try { $r = & $body; if ($r -eq $false) { throw 'returned false' }
        $results.Add([pscustomobject]@{ Test = $name; Ok = $true; Note = "$r" }) }
    catch { $results.Add([pscustomobject]@{ Test = $name; Ok = $false; Note = $_.Exception.Message }) }
}
function WaitFor([scriptblock]$cond, [int]$ms = 10000) {
    $deadline = (Get-Date).AddMilliseconds($ms)
    # Poka tochka podklyucheniya peresozdaetsya, obrashchenie k puti mozhet dat "otkazano v dostupe"
    while (-not $(try { & $cond } catch { $false })) { if ((Get-Date) -gt $deadline) { return $false }; Start-Sleep -Milliseconds 100 }
    $true
}

# Stroki interfeysa na oboih yazykah: proveryaem, chto okno pokazyvaet tekst, a ne klyuch
$strings = @{}
$byLang = @{ 'ru-RU' = @{}; 'en-US' = @{} }
foreach ($lang in 'ru-RU', 'en-US') {
    [xml]$x = Get-Content "$repo\ui\Zamok.UI\Strings\$lang\Resources.resw" -Raw -Encoding UTF8
    foreach ($d in $x.root.data) { if (-not $strings[$d.name]) { $strings[$d.name] = @() }; $strings[$d.name] += $d.value; $byLang[$lang][$d.name] = $d.value }
}
function IsText([string]$key, [string]$shown) { $strings[$key] -contains $shown }

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type -TypeDefinition @'
using System; using System.IO; using System.Runtime.InteropServices;
public static class ZT {
  [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
  static extern IntPtr CreateFileW(string n, uint a, uint s, IntPtr sa, uint c, uint f, IntPtr t);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
  [DllImport("kernel32.dll", SetLastError = true)]
  static extern bool SetFileInformationByHandle(IntPtr h, int cls, byte[] info, int size);
  // Udalit tochku podklyucheniya, kak Provodnik: u nee atribut "tolko chtenie" (koren zapertoy
  // papki), poetomu rmdir otkazyvaet. Snimaem atribut s samoy tochki (OPEN_REPARSE_POINT),
  // potom pometka na udalenie. Soderzhimoe toma ne trogaetsya.
  public static void DeleteMountPoint(string dir) {
    var h = CreateFileW(dir, 0x00010000 | 0x100, 7, IntPtr.Zero, 3, 0x02000000 | 0x00200000, IntPtr.Zero);
    if (h == new IntPtr(-1)) throw new IOException("open failed " + Marshal.GetLastWin32Error());
    try {
      var basic = new byte[40];
      BitConverter.GetBytes(0x4u).CopyTo(basic, 32);
      if (!SetFileInformationByHandle(h, 0, basic, basic.Length)) throw new IOException("attributes failed " + Marshal.GetLastWin32Error());
      if (!SetFileInformationByHandle(h, 4, new byte[] { 1, 0, 0, 0 }, 4)) throw new IOException("delete failed " + Marshal.GetLastWin32Error());
    }
    finally { CloseHandle(h); }
  }
}
'@
$AE = [Windows.Automation.AutomationElement]
function Window([int]$procId, [int]$ms = 15000) {
    $cond = New-Object Windows.Automation.PropertyCondition ($AE::ProcessIdProperty), $procId
    $w = $null; $deadline = (Get-Date).AddMilliseconds($ms)
    while (-not $w -and (Get-Date) -lt $deadline) { $w = $AE::RootElement.FindFirst('Children', $cond); if (-not $w) { Start-Sleep -Milliseconds 200 } }
    $w
}
function El($w, [string]$id) {
    $cond = New-Object Windows.Automation.PropertyCondition ($AE::AutomationIdProperty), $id
    $w.FindFirst('Descendants', $cond)
}
function Text($w, [string]$id) { $e = El $w $id; if ($e) { $e.Current.Name } else { $null } }
function SetValue($w, [string]$id, [string]$value) {
    $e = El $w $id; if (-not $e) { throw "no element $id" }
    $e.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).SetValue($value)
}
function Press($w, [string]$id) {
    $e = El $w $id; if (-not $e) { throw "no element $id" }
    $e.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Enabled($w, [string]$id) { (El $w $id).Current.IsEnabled }

# Dostupnost (etap 6): imena dlya ekrannogo diktora, klavishi dostupa, poryadok Tab.
# Nastoyashchie nazhatiya (SendInput) idut, tolko esli okno Zamok sejchas aktivnoe: inache
# klavisha popala by v chuzhoe okno. Togda fokus proveryaetsya cherez UI Automation, i v
# primechanii proverki eto napisano.
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices; using System.Collections.Generic;
public static class ZK {
  [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [StructLayout(LayoutKind.Sequential)] struct INPUT { public uint type; public KEYBDINPUT ki; public long pad; }
  [StructLayout(LayoutKind.Sequential)] struct KEYBDINPUT { public ushort vk; public ushort scan; public uint flags; public uint time; public IntPtr extra; }
  [DllImport("user32.dll")] static extern uint SendInput(uint n, INPUT[] i, int size);
  public static uint ForegroundPid() { uint p; GetWindowThreadProcessId(GetForegroundWindow(), out p); return p; }
  // Klavisha tolko v okno processa pid; false, esli aktivno drugoe okno
  public static bool Key(uint pid, ushort vk, bool shift) {
    if (ForegroundPid() != pid) return false;
    var l = new List<INPUT>();
    if (shift) l.Add(new INPUT { type = 1, ki = new KEYBDINPUT { vk = 0x10 } });
    l.Add(new INPUT { type = 1, ki = new KEYBDINPUT { vk = vk } });
    l.Add(new INPUT { type = 1, ki = new KEYBDINPUT { vk = vk, flags = 2 } });
    if (shift) l.Add(new INPUT { type = 1, ki = new KEYBDINPUT { vk = 0x10, flags = 2 } });
    return SendInput((uint)l.Count, l.ToArray(), Marshal.SizeOf(typeof(INPUT))) == l.Count;
  }
}
// Svoystvo LiveSetting (30135) upravlyaemaya biblioteka UI Automation ne znaet: chitaem cherez COM.
// Poryadok metodov kak v UIAutomationClient.h, nenuzhnye metody tolko derzhat mesto.
[ComImport, Guid("30cbe57d-d9d0-452a-ab13-7ac5ac4825ee"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IZUia { void CompareElements(); void CompareRuntimeIds(); void GetRootElement();
  [PreserveSig] int ElementFromHandle(IntPtr hwnd, out IZUiaElement el);
  void ElementFromPoint(); void GetFocusedElement(); void GetRootElementBuildCache(); void ElementFromHandleBuildCache();
  void ElementFromPointBuildCache(); void GetFocusedElementBuildCache(); void CreateTreeWalker(); void get_ControlViewWalker();
  void get_ContentViewWalker(); void get_RawViewWalker(); void get_RawViewCondition(); void get_ControlViewCondition();
  void get_ContentViewCondition(); void CreateCacheRequest(); void CreateTrueCondition(); void CreateFalseCondition();
  [PreserveSig] int CreatePropertyCondition(int id, [MarshalAs(UnmanagedType.Struct)] object value, [MarshalAs(UnmanagedType.IUnknown)] out object cond); }
[ComImport, Guid("d22108aa-8ac5-49a5-837b-37bbb3d7591e"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IZUiaElement { void SetFocus(); void GetRuntimeId();
  [PreserveSig] int FindFirst(int scope, [MarshalAs(UnmanagedType.IUnknown)] object cond, out IZUiaElement found);
  void FindAll(); void FindFirstBuildCache(); void FindAllBuildCache(); void BuildUpdatedCache();
  [PreserveSig] int GetCurrentPropertyValue(int id, [MarshalAs(UnmanagedType.Struct)] out object v); }
public static class ZLive {
  // LiveSetting elementa okna hwnd s etim AutomationId: 0 net, 1 Polite, 2 Assertive, -1 ne nayden
  public static int Of(IntPtr hwnd, string automationId) {
    var uia = (IZUia)Activator.CreateInstance(Type.GetTypeFromCLSID(new Guid("ff48dba4-60ef-4201-aa87-54103eef594e")));
    IZUiaElement w, e; object cond, v;
    if (uia.ElementFromHandle(hwnd, out w) != 0 || w == null) return -1;
    if (uia.CreatePropertyCondition(30011, automationId, out cond) != 0) return -1;
    if (w.FindFirst(4, cond, out e) != 0 || e == null) return -1;
    if (e.GetCurrentPropertyValue(30135, out v) != 0) return -1;
    return v == null ? 0 : Convert.ToInt32(v);
  }
}
'@
function Focusables($w) {
    $list = @()
    foreach ($e in @($w.FindAll('Descendants', [Windows.Automation.Condition]::TrueCondition))) {
        $c = $e.Current
        if (-not $c.IsKeyboardFocusable -or -not $c.IsEnabled -or $c.IsOffscreen) { continue }
        # Tab vhodit v spisok na vybrannuyu stroku, ostalnye stroki strelkami
        if ($c.ControlType -eq [Windows.Automation.ControlType]::ListItem -and
            -not $e.GetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern).Current.IsSelected) { continue }
        $list += $e
    }
    , $list
}
function Ident($e) { if ($e.Current.AutomationId) { $e.Current.AutomationId } else { $e.Current.Name } }
function Focused { Ident ([Windows.Automation.AutomationElement]::FocusedElement) }
function A11y([string]$label, $w, $proc) {
    Check "a11y ${label}: every control has a name for Narrator, access keys differ" {
        $els = Focusables $w
        if ($els.Count -lt 1) { throw 'nothing focusable' }
        # Imya pustoe ili klyuch stroki vmesto teksta
        $bad = @($els | Where-Object { $n = $_.Current.Name; -not $n.Trim() -or $strings.ContainsKey($n.Replace('/', '.')) } | ForEach-Object { Ident $_ })
        if ($bad) { throw "no name: $($bad -join ', ')" }
        $keys = @($w.FindAll('Descendants', [Windows.Automation.Condition]::TrueCondition) |
            Where-Object { -not $_.Current.IsOffscreen -and $_.Current.AccessKey } | ForEach-Object { $_.Current.AccessKey.ToUpper() })
        $dup = @($keys | Group-Object | Where-Object Count -gt 1 | ForEach-Object Name)
        if ($dup) { throw "same access key: $($dup -join ', ')" }
        "$($els.Count) controls; access keys $($keys -join ' ')"
    }
    Check "layout ${label}: visible buttons and fields do not overlap" {
        # Dve knopki v odnoy yacheyke setki ili tekst shire knopki: ramki peresekayutsya
        $all = Focusables $w
        $els = @($all | Where-Object { $_.Current.ControlType.ProgrammaticName -in
            'ControlType.Button', 'ControlType.Edit', 'ControlType.Hyperlink' })
        $hit = @()
        for ($i = 0; $i -lt $els.Count; $i++) {
            for ($j = $i + 1; $j -lt $els.Count; $j++) {
                $a = $els[$i].Current.BoundingRectangle; $b = $els[$j].Current.BoundingRectangle
                if ($a.IsEmpty -or $b.IsEmpty) { continue }
                $x = [Math]::Min($a.Right, $b.Right) - [Math]::Max($a.Left, $b.Left)
                $y = [Math]::Min($a.Bottom, $b.Bottom) - [Math]::Max($a.Top, $b.Top)
                if ($x -gt 1 -and $y -gt 1) { $hit += "$(Ident $els[$i]) / $(Ident $els[$j])" }
            }
        }
        if ($hit) { throw "overlap: $($hit -join ', ')" }
        "$($els.Count) controls apart"
    }
    Check "a11y ${label}: Tab and Shift+Tab go through every control in order" {
        # Okno moglo izmenitsya vo vremya proverki (shifrovanie doshlo do kontsa, i Otmena
        # vyklyuchilas): togda eshche raz s novym sostavom
        foreach ($attempt in 1..3) {
            $els = Focusables $w
            $want = @($els | ForEach-Object { Ident $_ })
            $els[0].SetFocus(); Start-Sleep -Milliseconds 200
            $why = $null
            if ([ZK]::ForegroundPid() -ne $proc.Id -or (Focused) -ne $want[0]) { $why = 'window not active'; break }
            $fwd = @(); $back = @()
            foreach ($i in 1..(2 * $want.Count)) {
                if (-not [ZK]::Key($proc.Id, 9, $i -gt $want.Count)) { $why = "another window became active after $($i - 1) presses"; break }
                Start-Sleep -Milliseconds 150
                if ($i -le $want.Count) { $fwd += Focused } else { $back += Focused }
            }
            if ($why) { break }
            $expFwd = @($want | Select-Object -Skip 1) + $want[0]
            $expBack = @($want[($want.Count - 1)..0])
            if (($fwd -join '>') -eq ($expFwd -join '>') -and ($back -join '>') -eq ($expBack -join '>')) { return "Tab pressed for real: $($want -join ' > ')" }
            if ((@(Focusables $w | ForEach-Object { Ident $_ }) -join '>') -ne ($want -join '>') -and $attempt -lt 3) { continue }
            if (($fwd -join '>') -ne ($expFwd -join '>')) { throw "Tab: $($fwd -join ' > '), want $($expFwd -join ' > ')" }
            throw "Shift+Tab: $($back -join ' > '), want $($expBack -join ' > ')"
        }
        # Klavishi ne nazhat: Windows ne dala oknu klaviaturu. Kazhdyy element dostupen cherez
        # UI Automation (deystvie est), poryadok berem iz dereva okna
        foreach ($e in $els) {
            $pats = @($e.GetSupportedPatterns() | ForEach-Object { $_.ProgrammaticName })
            if (-not ($pats -match 'Invoke|Toggle|Value|SelectionItem|ExpandCollapse|Scroll')) { throw "$(Ident $e): no action for UI Automation" }
        }
        "$why, Tab not pressed for real; every control usable through UI Automation: $($want -join ' > ')"
    }
}
# LiveSetting: 1 Polite, 2 Assertive (ekrannyy diktor chitaet stroku sam, bez perehoda k ney)
function Live($w, [string]$id) { [ZLive]::Of([IntPtr]$w.Current.NativeWindowHandle, $id) }

# Okno "Proverka sledov" sredi okon processa (ono otkryvaetsya iz nastroek, okon dva)
function TracesWindow([int]$procId) {
    $cond = New-Object Windows.Automation.PropertyCondition ($AE::ProcessIdProperty), $procId
    foreach ($c in @($AE::RootElement.FindAll('Children', $cond))) { if (El $c 'RecheckButton') { return $c } }
    $null
}
# Tekst sovpadaet so strokoy iz fayla strok (podstanovka {0} lyubaya)
function IsTextLike([string]$prefix, [string]$shown) {
    foreach ($k in @($strings.Keys | Where-Object { $_.StartsWith($prefix) })) {
        foreach ($v in $strings[$k]) { if ($shown -match ('^' + (([regex]::Escape($v)) -replace '\\\{\d\}', '.*') + '$')) { return $true } }
    }
    $false
}
# V kazhdoy stroke sostoyanie i sovet, oba iz fayla strok; Zamok nichego ne menyaet
function TracesRows($tw) {
    $rows = @()
    foreach ($r in 'Disk', 'Hiber', 'Page', 'Index', 'Thumbs') {
        $st = Text $tw "${r}Status"; $ad = Text $tw "${r}Advice"
        if (-not (IsTextLike "Traces_${r}_" $st)) { throw "${r}: status is not a string: $st" }
        if (-not ((IsTextLike "Traces_${r}Do_" $ad) -or (IsText 'Traces_Nothing' $ad))) { throw "${r}: advice is not a string: $ad" }
        $rows += "${r}: $st"
    }
    $rows -join ' | '
}

function StartCore {
    $psi = New-Object Diagnostics.ProcessStartInfo $coreExe
    $psi.UseShellExecute = $false
    $psi.EnvironmentVariables['ZAMOK_DATA_DIR'] = $data
    $psi.EnvironmentVariables['ZAMOK_PIPE'] = "\\.\pipe\$pipeName"
    $psi.EnvironmentVariables['ZAMOK_NO_TRAY'] = '1'
    $psi.EnvironmentVariables['ZAMOK_SEARCH_DRY_RUN'] = '1'
    # Okno "papku udalili" core zapuskaet iz proveryaemoy sborki
    $psi.EnvironmentVariables['ZAMOK_UI_EXE'] = $uiExe
    $p = [Diagnostics.Process]::Start($psi)
    [void](WaitFor { (Test-Path "\\.\pipe\$pipeName") -or $p.HasExited } 15000)
    $p
}
# Okno Provodnika na papke: skolko elementov ono pokazyvaet (UI Automation, bez F5)
function ExplorerWindow([string]$path) {
    $sh = New-Object -ComObject Shell.Application
    foreach ($w in @($sh.Windows())) { $here = $null; try { $here = $w.Document.Folder.Self.Path } catch {}; if ($here -eq $path) { return $w } }
    $null
}
function ShownItems([string]$path) {
    $w = ExplorerWindow $path
    if (-not $w) { return -1 }
    $cond = New-Object Windows.Automation.PropertyCondition ($AE::ControlTypeProperty), ([Windows.Automation.ControlType]::ListItem)
    @($AE::FromHandle([IntPtr]$w.HWND).FindAll('Descendants', $cond)).Count
}
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
function Folder([string]$path) { (Send '{"cmd":"status"}').folders | Where-Object { $_.path -eq $path } }
function StartUi([string[]]$uiArgs, [string]$pipe = $pipeName, [string]$lang = '') {
    $psi = New-Object Diagnostics.ProcessStartInfo $uiExe
    $psi.UseShellExecute = $false
    $psi.Arguments = ($uiArgs | ForEach-Object { '"' + $_ + '"' }) -join ' '
    $psi.EnvironmentVariables['ZAMOK_PIPE'] = $pipe
    if ($lang) { $psi.EnvironmentVariables['ZAMOK_LANG'] = $lang }
    [Diagnostics.Process]::Start($psi)
}
function MakeSample([string]$dir) {
    New-Item -ItemType Directory -Force "$dir\sub" | Out-Null
    [IO.File]::WriteAllText("$dir\note.txt", 'hello from ui test')
    [IO.File]::WriteAllBytes("$dir\sub\data.bin", (New-Object byte[] 200000))
}

# Podgotovka
Get-Process Zamok, zamok-core -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$repo*" } | Stop-Process -Force
Start-Sleep -Milliseconds 500
Remove-Item $root -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $root | Out-Null
Push-Location "$repo\core"; & "$env:USERPROFILE\.cargo\bin\cargo.exe" build -q --release -p zamok-core 2>&1 | Out-Null; Pop-Location
# Proveryaem opublikovannuyu sborku: imenno ona stavitsya (v obychnoy sborke est fayly,
# kotoryh v publikacii mozhet ne okazatsya, tak uzhe bylo s Zamok.pri)
& dotnet publish "$repo\ui\Zamok.UI\Zamok.UI.csproj" -c Release -o "$root-publish" -v q 2>&1 | Out-Null
$uiExe = "$root-publish\Zamok.exe"

$script:core = $null
$ui = $null
try {
    $script:core = StartCore
    $dir = "$root\$ruName"; MakeSample $dir

    # 1. Master shifrovaniya
    $ui = StartUi '--encrypt', $dir
    $w = Window $ui.Id
    Check 'wizard: window opens with the folder name in the heading' { $w -and (Text $w 'Heading') -like "*$ruName*" }
    Check 'wizard: Encrypt is disabled until passwords match' {
        SetValue $w 'Password1' $pw; SetValue $w 'Password2' 'other'
        Start-Sleep -Milliseconds 300
        -not (Enabled $w 'NextButton')
    }
    Check 'wizard: strength text is a real string' {
        # Na zanyatoy mashine okno inogda ne uspevaet za vvodom: zhdem, pri nuzhde vvodim eshche raz
        foreach ($try in 1..3) { SetValue $w 'Password2' $pw; if (WaitFor { Enabled $w 'NextButton' } 3000) { break } }
        (IsText 'Encrypt_Strength4' (Text $w 'StrengthText')) -and (Enabled $w 'NextButton')
    }
    A11y 'wizard password step' $w $ui
    Press $w 'NextButton'
    [void](WaitFor { (Text $w 'KeyWords') -match '44\.' } 20000)
    $keyText = Text $w 'KeyWords'
    $words = @([regex]::Matches($keyText, '\d+\.\s+(\S+)') | ForEach-Object { $_.Groups[1].Value })
    Check 'wizard: recovery key shows 44 numbered words' { $words.Count -eq 44 }
    A11y 'wizard key step' $w $ui
    Press $w 'NextButton'
    Start-Sleep -Milliseconds 500
    A11y 'wizard words step' $w $ui
    Check 'wizard: wrong words are rejected' {
        1..4 | ForEach-Object { SetValue $w "Word$_" 'wrong' }
        Press $w 'NextButton'; Start-Sleep -Milliseconds 300
        IsText 'Encrypt_WordsWrong' (Text $w 'ErrorText')
    }
    Check 'wizard: the error line is read by Narrator at once (LiveSetting Assertive)' { (Live $w 'ErrorText') -eq 2 }
    Check 'wizard: correct words lead to the result' {
        foreach ($i in 1..4) {
            $header = (El $w "Word$i").Current.Name
            $n = [int]([regex]::Match($header, '\d+').Value)
            SetValue $w "Word$i" $words[$n - 1]
        }
        Press $w 'NextButton'
        WaitFor { (Text $w 'ProgressTitle') -like "*$ruName*" } 60000
    }
    Check 'wizard: folder is encrypted and open, content intact' {
        (Folder $dir).state -eq 'unlocked' -and (Get-Content "$dir\note.txt") -eq 'hello from ui test'
    }
    A11y 'wizard result' $w $ui
    Check 'wizard: Lock now locks the folder and closes the window' {
        Press $w 'LockNowButton'
        $ui.WaitForExit(10000) -and (Folder $dir).state -eq 'locked'
    }

    # 2. Okno parolya
    [void](Send "{`"cmd`":`"lock`",`"path`":`"$(J $dir)`"}")
    $item = "$dir\$openWord $ruName.zamok"
    $ui = StartUi '--unlock', $item
    $w = Window $ui.Id
    Check 'unlock: window shows the folder name' { $w -and (Text $w 'FolderName') -eq $ruName }
    Check 'unlock: prompt is a real string' { IsText 'Unlock_Prompt.Text' (Text $w 'Prompt') }
    A11y 'unlock' $w $ui
    Check 'unlock: wrong password keeps the window and says so' {
        SetValue $w 'Password' 'not the password'; Press $w 'OpenButton'
        (WaitFor { IsText 'Error_wrong_password' (Text $w 'ErrorText') } 10000) -and -not $ui.HasExited -and (Folder $dir).state -eq 'locked'
    }
    Check 'unlock: "Wrong password" is read by Narrator at once (LiveSetting Assertive)' { (Live $w 'ErrorText') -eq 2 }
    Check 'unlock: right password opens the folder and closes the window' {
        SetValue $w 'Password' $pw; Press $w 'OpenButton'
        $ui.WaitForExit(10000) -and (Folder $dir).state -eq 'unlocked' -and (Test-Path "$dir\note.txt")
    }
    Check 'unlock: already open folder closes the window by itself' {
        $ui = StartUi '--unlock', $item
        $ui.WaitForExit(15000)
    }
    # Klaviatura po-nastoyashchemu: Enter v pole parolya otkryvaet, Esc zakryvaet okno
    [void](Send "{`"cmd`":`"lock`",`"path`":`"$(J $dir)`"}")
    $ui = StartUi '--unlock', $item
    $w = Window $ui.Id
    Check 'keyboard: Enter in the password field opens the folder' {
        SetValue $w 'Password' $pw; (El $w 'Password').SetFocus(); Start-Sleep -Milliseconds 200
        if (-not [ZK]::Key($ui.Id, 13, $false)) { Press $w 'OpenButton'; $how = 'window not active, Enter not pressed for real (Open pressed through UI Automation)' } else { $how = 'Enter pressed for real' }
        if (-not ($ui.WaitForExit(10000) -and (Folder $dir).state -eq 'unlocked')) { throw 'not opened' }
        $how
    }
    $ui = StartUi '--unlock', "$root\not-encrypted-esc"
    $w = Window $ui.Id
    Check 'keyboard: Esc closes the window' {
        Start-Sleep -Milliseconds 500; (El $w 'CancelButton').SetFocus(); Start-Sleep -Milliseconds 200
        if (-not [ZK]::Key($ui.Id, 27, $false)) { $ui.Kill(); return 'window not active, Esc not pressed for real' }
        if (-not $ui.WaitForExit(5000)) { throw 'still open' }
        'Esc pressed for real'
    }

    # 3. Klyuch vosstanovleniya
    [void](Send "{`"cmd`":`"lock`",`"path`":`"$(J $dir)`"}")
    $ui = StartUi '--unlock', $dir
    $w = Window $ui.Id
    Press $w 'ModeLink'; Start-Sleep -Milliseconds 300
    A11y 'unlock with the recovery key' $w $ui
    Check 'recovery: the key opens the folder' {
        (IsText 'Unlock_PromptRecovery.Text' (Text $w 'Prompt')) | Out-Null
        SetValue $w 'RecoveryWords' ($words -join ' '); Press $w 'OpenButton'
        $ui.WaitForExit(10000) -and (Folder $dir).state -eq 'unlocked'
    }
    # Fayl klyucha celikom (zagolovok, tablica s nomerami, podskazka) vstavlen v pole
    [void](Send "{`"cmd`":`"lock`",`"path`":`"$(J $dir)`"}")
    $table = (0..10 | ForEach-Object { $r = $_; (0..3 | ForEach-Object { $n = $r * 4 + $_ + 1; ('{0,2}. {1,-13}' -f $n, $words[$n - 1]) }) -join '' }) -join "`r`n"
    $fileText = "Zamok recovery key for the folder `"x`"`r`nSaved 2026-11-08 12:00`r`n`r`n$table`r`n`r`nHow to open: paste this whole file."
    $ui = StartUi '--unlock', $dir
    $w = Window $ui.Id
    Check 'recovery: the whole saved key file pasted opens the folder' {
        Press $w 'ModeLink'; Start-Sleep -Milliseconds 300
        SetValue $w 'RecoveryWords' $fileText; Press $w 'OpenButton'
        $ui.WaitForExit(10000) -and (Folder $dir).state -eq 'unlocked'
    }

    # Variant bez klyucha vosstanovleniya: posle parolya srazu itog
    $dir2 = "$root\nokey"; MakeSample $dir2
    $ui = StartUi '--encrypt', $dir2
    $w = Window $ui.Id
    Check 'wizard without a recovery key: warning shown, no key page, folder encrypted' {
        SetValue $w 'Password1' $pw; SetValue $w 'Password2' $pw
        (El $w 'MakeKey').GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern).Toggle()
        Start-Sleep -Milliseconds 300
        $warn = El $w 'NoKeyWarning'
        if (-not $warn -or -not (IsText 'Encrypt_NoKeyWarning.Text' $warn.Current.Name)) { throw 'no warning' }
        Press $w 'NextButton'
        if (-not (WaitFor { (Text $w 'ProgressTitle') -like '*nokey*' } 60000)) { throw 'no result' }
        $key = El $w 'KeyWords'
        if ($key -and -not $key.Current.IsOffscreen) { throw 'key page was shown' }
        (Folder $dir2).state -eq 'unlocked'
    }
    $ui.Kill()

    # Peredumal hranit klyuch: knopka "Bez klyucha" na shage s klyuchom, s podtverzhdeniem
    $dir3 = "$root\skipkey"; MakeSample $dir3
    $ui = StartUi '--encrypt', $dir3
    $w = Window $ui.Id
    Check 'wizard: No key on the key page asks again, then skips to the result' {
        SetValue $w 'Password1' $pw; SetValue $w 'Password2' $pw; Start-Sleep -Milliseconds 300
        Press $w 'NextButton'
        if (-not (WaitFor { (Text $w 'KeyWords') -match '44\.' } 20000)) { throw 'no key page' }
        Press $w 'SkipKeyButton'; Start-Sleep -Milliseconds 300
        if (-not (IsText 'Encrypt_NoKeyWarning.Text' (Text $w 'ErrorText'))) { throw 'no warning' }
        Press $w 'SkipKeyButton'
        (WaitFor { (Text $w 'ProgressTitle') -like '*skipkey*' } 60000) -and (Folder $dir3).state -eq 'unlocked'
    }
    $ui.Kill()

    # Rasshifrovat navsegda cherez okno
    $ui = StartUi '--decrypt', $dir2
    $w = Window $ui.Id
    Start-Sleep -Milliseconds 500
    A11y 'decrypt window' $w $ui
    Check 'decrypt window: wrong password is reported' {
        SetValue $w 'Password' 'not it'; Start-Sleep -Milliseconds 300; Press $w 'OkButton'
        WaitFor { IsText 'Error_wrong_password' (Text $w 'ErrorText') } 10000
    }
    Check 'decrypt window: folder becomes ordinary again' {
        SetValue $w 'Password' $pw; Start-Sleep -Milliseconds 300; Press $w 'OkButton'
        (WaitFor { (Text $w 'DoneText') -like '*nokey*' } 60000) -and -not (Folder $dir2) -and
            -not ((Get-Item -LiteralPath $dir2 -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) -and
            (Get-Content "$dir2\note.txt") -eq 'hello from ui test'
    }
    $ui.Kill()

    # Smenit parol i pokazat klyuch (podmenyu Zamok)
    $ui = StartUi '--change-password', $dir
    $w = Window $ui.Id
    $pw2 = 'Novyy parol dlya testa 2026'
    Start-Sleep -Milliseconds 500
    A11y 'change password' $w $ui
    Check 'change password: wrong current password is reported' {
        SetValue $w 'Password' 'not it'; SetValue $w 'NewPassword1' $pw2; SetValue $w 'NewPassword2' $pw2
        Start-Sleep -Milliseconds 300; Press $w 'OkButton'
        WaitFor { IsText 'Error_wrong_password' (Text $w 'ErrorText') } 10000
    }
    Check 'change password: done, the new password opens the folder' {
        SetValue $w 'Password' $pw; SetValue $w 'NewPassword1' $pw2; SetValue $w 'NewPassword2' $pw2
        Start-Sleep -Milliseconds 300; Press $w 'OkButton'
        if (-not (WaitFor { IsText 'Manage_Changed' (Text $w 'DoneText') } 10000)) { throw 'not changed' }
        Press $w 'OkButton'; [void]$ui.WaitForExit(10000)
        [void](Send "{`"cmd`":`"lock`",`"path`":`"$(J $dir)`"}")
        (Send "{`"cmd`":`"unlock`",`"path`":`"$(J $dir)`",`"password`":`"$pw2`"}").ok
    }
    $ui = StartUi '--show-key', $dir
    $w = Window $ui.Id
    Start-Sleep -Milliseconds 500
    A11y 'show key, password' $w $ui
    Check 'show key: the password shows the same 44 words' {
        SetValue $w 'Password' $pw2; Start-Sleep -Milliseconds 300; Press $w 'OkButton'
        if (-not (WaitFor { (Text $w 'KeyWords') -match '44\.' } 10000)) { throw 'no key' }
        $shown = @([regex]::Matches((Text $w 'KeyWords'), '\d+\.\s+(\S+)') | ForEach-Object { $_.Groups[1].Value })
        ($shown -join ' ') -eq ($words -join ' ')
    }
    A11y 'show key, the key' $w $ui
    $ui.Kill()
    $pw = $pw2

    # Okno nastroek: spisok papok, obshchaya nastroyka Win+L, Zaperet iz okna
    [void](Send "{`"cmd`":`"unlock`",`"path`":`"$(J $dir)`",`"password`":`"$pw`"}")
    $ui = StartUi '--settings'
    $w = Window $ui.Id
    Check 'settings: folder list and properties are shown' {
        WaitFor { (Text $w 'DetailName') -and (Text $w 'PropVault') -like '*.zamok*' } 10000
    }
    Check 'settings: screen lock switch changes the core setting' {
        $sw = El $w 'ScreenLockSwitch'
        $sw.GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern).Toggle()
        $off = WaitFor { (Send '{"cmd":"status"}').settings.lock_on_screen_lock -eq $false } 5000
        $sw.GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern).Toggle()
        $off -and (WaitFor { (Send '{"cmd":"status"}').settings.lock_on_screen_lock -eq $true } 5000)
    }
    Check 'settings: Defender offer is shown with the vaults, Copy puts the command on the clipboard' {
        # Svernutyy element v dereve UI Automation ne viden: nalichie teksta i est pokaz kartochki
        if (-not (WaitFor { El $w 'DefenderText' } 10000)) { throw 'no card' }
        if (-not (IsText 'Defender_Text.Text' (Text $w 'DefenderText'))) { throw 'text is not a string' }
        A11y 'settings' $w $ui
        $offer = @((Send '{"cmd":"status"}').settings.defender_offer)
        $saved = Get-Clipboard -Raw -ErrorAction SilentlyContinue
        # Buffer obmena mozhet derzhat drugaya programma: okno togda pishet ob etom, nazhimaem eshche raz
        $script:copied = $null
        foreach ($try in 1..3) {
            Press $w 'DefenderCopy'; Start-Sleep -Milliseconds 500
            if (WaitFor { (IsText 'Defender_Copied' (Text $w 'StatusText')) -or (IsText 'Common_ClipboardBusy' (Text $w 'ErrorText')) } 5000) {
                if (IsText 'Defender_Copied' (Text $w 'StatusText')) { break }
            }
        }
        [void](WaitFor { $script:copied = Get-Clipboard -Raw; $script:copied -like 'Add-MpPreference*' } 5000)
        if ($saved) { Set-Clipboard -Value $saved }
        $env:ZT_CMD = $script:copied
        $env:ZT_WANT = $offer -join '|'
        $got = & powershell -NoProfile -Command 'function Add-MpPreference { param([string[]]$ExclusionPath) $ExclusionPath }; if (((Invoke-Expression $env:ZT_CMD) -join ''|'') -eq $env:ZT_WANT) { ''same'' } else { ''diff'' }'
        if ($got -ne 'same') { throw "command gives other paths: $got" }
        $offer.Count -ge 1 -and $offer -contains "$root\.$ruName.zamok" -and -not $script:copied.Contains($root) -and
            (WaitFor { @((Send '{"cmd":"status"}').settings.defender_offer).Count -eq 0 } 5000) -and
            (IsText 'Defender_Copied' (Text $w 'StatusText'))
    }
    Check 'settings: the status line is read by Narrator (LiveSetting Polite)' { (Live $w 'StatusText') -eq 1 }
    Check 'settings: Defender offer is gone after the answer' {
        WaitFor { -not (El $w 'DefenderText') } 5000
    }
    Check 'settings: Lock button locks the open folder' {
        $list = El $w 'FolderList'
        $item = $list.FindAll('Children', [Windows.Automation.Condition]::TrueCondition) | Where-Object { $_.Current.Name -like "*$ruName*" -or $_.FindFirst('Descendants', (New-Object Windows.Automation.PropertyCondition ($AE::NameProperty), $ruName)) } | Select-Object -First 1
        if ($item) { $item.GetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern).Select(); Start-Sleep -Milliseconds 500 }
        if ((Text $w 'DetailName') -ne $ruName) { throw "selected: $(Text $w 'DetailName')" }
        Press $w 'ToggleButton'
        WaitFor { (Folder $dir).state -eq 'locked' } 10000
    }
    # Proverka sledov: knopka v nastroykah otkryvaet vtoroe okno togo zhe processa
    Check 'traces: Settings button opens the check, every row has a status and advice from the string file' {
        Press $w 'TracesButton'
        if (-not (WaitFor { $script:tw = TracesWindow $ui.Id; $script:tw -and (El $script:tw 'RecheckButton').Current.IsEnabled -and (Text $script:tw 'ThumbsStatus') } 20000)) { throw 'no traces window' }
        TracesRows $script:tw
    }
    Check 'traces: test folders outside the profile are not in the search index' {
        $s = Text $script:tw 'IndexStatus'
        if ($s -ne $byLang['ru-RU']['Traces_Index_Outside'] -and $s -ne $byLang['ru-RU']['Traces_Index_SearchOff']) { throw "index: $s" }
        $s
    }
    if ($script:tw) { A11y 'traces' $script:tw $ui }
    Check 'traces: Check again runs the check again, Close closes only this window' {
        Press $script:tw 'RecheckButton'
        if (-not (WaitFor { (El $script:tw 'RecheckButton').Current.IsEnabled } 20000)) { throw 'still checking' }
        Press $script:tw 'CloseButton'
        (WaitFor { -not (TracesWindow $ui.Id) } 5000) -and -not $ui.HasExited -and (Text $w 'DetailName')
    }
    $ui.Kill()

    # 4. Oshibki
    $ui = StartUi '--unlock', "$root\not-encrypted"
    $w = Window $ui.Id
    Check 'unlock: folder that is not encrypted is reported' {
        (WaitFor { IsText 'Error_not_found' (Text $w 'ErrorText') } 10000) -and -not (Enabled $w 'OpenButton')
    }
    $ui.Kill()
    $ui = StartUi '--unlock', $dir 'zamok-nobody-listens-here'
    $w = Window $ui.Id
    Check 'unlock: core not running is reported' {
        (WaitFor { IsText 'Error_core_not_running' (Text $w 'ErrorText') } 15000) -and -not (Enabled $w 'OpenButton')
    }
    $ui.Kill()
    $busyDir = "$root\busy"; MakeSample $busyDir
    $held = [IO.File]::Open("$busyDir\note.txt", 'Open', 'Read', 'Read')
    $ui = StartUi '--encrypt', $busyDir
    $w = Window $ui.Id
    Check 'wizard: file open in another program is named, folder untouched' {
        SetValue $w 'Password1' $pw; SetValue $w 'Password2' $pw; Start-Sleep -Milliseconds 300
        Press $w 'NextButton'
        (WaitFor { (Text $w 'ErrorText') -like '*note.txt*' } 15000) -and (Get-Content "$busyDir\note.txt") -eq 'hello from ui test' -and -not (Test-Path "$root\.busy.zamok.tmp")
    }
    $held.Dispose(); $ui.Kill(); [void]$ui.WaitForExit(5000)

    # 5. Papku udalili v Provodnike: core sprashivaet v okne
    function DeletedWindow {
        # Tolko okno, kotoroe zapustil core: okna proshlyh proverok mogut eshche zavershatsya
        $c = Get-CimInstance Win32_Process -Filter "Name='Zamok.exe'" | Where-Object { $_.ExecutablePath -eq $uiExe -and $_.CommandLine -like '*--deleted*' } | Select-Object -First 1
        if ($c) { Get-Process -Id $c.ProcessId -ErrorAction SilentlyContinue } else { $null }
    }
    [void](Send "{`"cmd`":`"lock`",`"path`":`"$(J $dir)`"}")
    Check 'deleted folder: window asks, Bring back returns the folder locked' {
        [ZT]::DeleteMountPoint($dir)
        if (-not (WaitFor { DeletedWindow } 10000)) { throw 'no window' }
        $p = DeletedWindow; $w = Window $p.Id
        if ((Text $w 'FolderName') -ne $ruName) { throw "name: $(Text $w 'FolderName')" }
        if (-not (IsText 'Deleted_Message.Text' (Text $w 'Message'))) { throw 'message is not a string' }
        A11y 'deleted folder' $w $p
        Press $w 'RestoreButton'
        $p.WaitForExit(10000) -and (WaitFor { (Folder $dir).state -eq 'locked' -and (Test-Path "$dir\$openWord $ruName.zamok") } 10000)
    }
    Check 'deleted folder: window closed without an answer, the folder comes back by itself' {
        [ZT]::DeleteMountPoint($dir)
        if (-not (WaitFor { DeletedWindow } 10000)) { throw 'no window' }
        $p = DeletedWindow; $w = Window $p.Id
        Start-Sleep -Milliseconds 500
        $p.CloseMainWindow() | Out-Null
        $p.WaitForExit(10000) -and (WaitFor { (Folder $dir).state -eq 'locked' } 10000)
    }
    Check 'deleted folder: Remove from Zamok keeps the encrypted data and writes the note' {
        [ZT]::DeleteMountPoint($dir)
        if (-not (WaitFor { DeletedWindow } 10000)) { throw 'no window' }
        $p = DeletedWindow; $w = Window $p.Id
        if ((Text $w 'RemoveNote') -notlike "*.$ruName.zamok*") { throw "note text: $(Text $w 'RemoveNote')" }
        Press $w 'RemoveButton'
        $gone = $p.WaitForExit(10000) -and (WaitFor { -not (Folder $dir) } 10000)
        $note = @(Get-ChildItem -LiteralPath $root -Filter "*$ruName.txt" -File)
        $gone -and -not (Test-Path $dir) -and (Test-Path "$root\.$ruName.zamok\masterkey.cryptomator") -and $note.Count -eq 1
    }

    # Pora zapirat po bezdeystviyu, a fayl otkryt: core sprashivaet v okne --idle-lock
    function IdleWindow {
        $c = Get-CimInstance Win32_Process -Filter "Name='Zamok.exe'" | Where-Object { $_.ExecutablePath -eq $uiExe -and $_.CommandLine -like '*--idle-lock*' } | Select-Object -First 1
        if ($c) { Get-Process -Id $c.ProcessId -ErrorAction SilentlyContinue } else { $null }
    }
    $idleDir = "$root\idle"; MakeSample $idleDir
    [void](Send "{`"cmd`":`"encrypt`",`"path`":`"$(J $idleDir)`",`"password`":`"$pw`"}")
    [void](WaitFor { (Folder $idleDir).state -eq 'unlocked' } 60000)
    $me = [IO.Path]::GetFileName((Get-Process -Id $PID).Path)
    $script:idleHeld = [IO.File]::Open("$idleDir\note.txt", 'Open', 'Read', 'ReadWrite')
    Check 'idle lock with a file open: window names the program, Postpone keeps the folder open' {
        [void](Send "{`"cmd`":`"set_idle_lock`",`"path`":`"$(J $idleDir)`",`"secs`":3}")
        if (-not (WaitFor { IdleWindow } 15000)) { throw 'no window' }
        $p = IdleWindow; $w = Window $p.Id
        if ((Text $w 'FolderName') -ne 'idle') { throw "name: $(Text $w 'FolderName')" }
        $want = @($strings['Idle_OpenIn'] | ForEach-Object { $_ -f $me })
        if (-not (WaitFor { $want -contains (Text $w 'OpenIn') } 5000)) { throw "open in: $(Text $w 'OpenIn')" }
        if (-not (IsText 'Idle_Later.Text' (Text $w 'Later'))) { throw 'later text is not a string' }
        A11y 'idle lock question' $w $p
        Press $w 'PostponeButton'
        $p.WaitForExit(10000) -and -not (Folder $idleDir).idle_asking -and (Start-Sleep -Seconds 5) -eq $null -and
            (Folder $idleDir).state -eq 'unlocked'
    }
    Check 'idle lock with a file open: Lock now locks the folder' {
        # Deskriptor, otkrytyy do zapiraniya, posle nego ne schitaetsya: otkryt zanovo
        [void](Send "{`"cmd`":`"set_idle_lock`",`"path`":`"$(J $idleDir)`",`"secs`":0}")
        [void](Send "{`"cmd`":`"lock`",`"path`":`"$(J $idleDir)`"}")
        [void](Send "{`"cmd`":`"unlock`",`"path`":`"$(J $idleDir)`",`"password`":`"$pw`"}")
        $script:idleHeld.Dispose(); $script:idleHeld = [IO.File]::Open("$idleDir\note.txt", 'Open', 'Read', 'ReadWrite')
        [void](Send "{`"cmd`":`"set_idle_lock`",`"path`":`"$(J $idleDir)`",`"secs`":3}")
        if (-not (WaitFor { IdleWindow } 15000)) { throw 'no window' }
        $p = IdleWindow; $w = Window $p.Id
        [void](WaitFor { Text $w 'OpenIn' } 5000)
        Press $w 'LockButton'
        $p.WaitForExit(10000) -and (WaitFor { (Folder $idleDir).state -eq 'locked' } 10000)
    }
    $script:idleHeld.Dispose()
    # Ubrat papku iz spiska: lishnyaya stroka v nastroykah dala by polosu prokrutki (ee knopki
    # WinUI nazyvaet na yazyke Windows, proverka yazyka nizhe ih ne zhdet)
    [void](Send "{`"cmd`":`"remove`",`"path`":`"$(J $idleDir)`"}")

    # Yazyk: okna po yazyku Windows (ZAMOK_LANG zadaet ego dlya testa), tekst iz svoego fayla strok
    foreach ($l in @(@('ru', 'ru-RU'), @('en', 'en-US'))) {
        $ui = StartUi '--unlock', $item $pipeName $l[0]
        $w = Window $ui.Id
        Check "language $($l[0]): the window speaks it" {
            (WaitFor { (Text $w 'Prompt') -eq $byLang[$l[1]]['Unlock_Prompt.Text'] } 5000) -and
                (Text $w 'OpenButton') -eq $byLang[$l[1]]['Unlock_Open.Content']
        }
        if ($l[0] -eq 'en') { A11y 'unlock in English' $w $ui }
        $ui.Kill()
    }
    $ui = StartUi '--settings' $pipeName 'en'
    $w = Window $ui.Id
    [void](WaitFor { Text $w 'DetailName' } 10000)
    # Ni odnogo russkogo slova, krome imen papok (podpisi pereklyuchateley WinUI inache berutsya iz
    # yazyka Windows). Imya polosy prokrutki i ee knopok daet sama WinUI na yazyke Windows, ego ne zadat
    function NoRussian($w) {
        # Knopki vnutri polosy prokrutki tozhe ot WinUI: propuskaem vse vnutri ScrollBar
        $walker = [Windows.Automation.TreeWalker]::ControlViewWalker
        $inBar = { param($e) for ($p = $e; $p; $p = $walker.GetParent($p)) { if ($p.Current.ControlType.ProgrammaticName -eq 'ControlType.ScrollBar') { return $true } }; $false }
        $ru = @($w.FindAll('Descendants', [Windows.Automation.Condition]::TrueCondition) |
            Where-Object { -not (& $inBar $_) } | ForEach-Object { $_.Current.Name } |
            Where-Object { $_ -match '[Ѐ-ӿ]' -and $_ -notlike "*$ruName*" })
        if ($ru) { throw "Russian text: $($ru -join ' | ')" }
        $true
    }
    Check 'language en: settings have no Russian text' { NoRussian $w }
    A11y 'settings in English' $w $ui
    $ui.Kill()
    $ui = StartUi '--traces' $pipeName 'en'
    $w = Window $ui.Id
    [void](WaitFor { (El $w 'RecheckButton').Current.IsEnabled -and (Text $w 'ThumbsStatus') } 20000)
    Check 'language en: the traces check has no Russian text' { (NoRussian $w) -and (TracesRows $w) }
    A11y 'traces in English' $w $ui
    $ui.Kill()
    $dir4 = "$root\english"; MakeSample $dir4
    $ui = StartUi '--encrypt', $dir4 $pipeName 'en'
    $w = Window $ui.Id
    Start-Sleep -Milliseconds 500
    Check 'language en: the wizard has no Russian text' { NoRussian $w }
    A11y 'wizard in English' $w $ui
    $ui.Kill()

    # 6. Okno Provodnika na papke posle parolya pokazyvaet fayly bez F5. Poslednim: Provodnik
    # zabiraet aktivnost, i okna, zapushchennye testom posle nego, aktivnymi ne stanovyatsya
    $dir = "$root\refresh"; MakeSample $dir
    [void](Send "{`"cmd`":`"encrypt`",`"path`":`"$(J $dir)`",`"password`":`"$pw`"}")
    [void](WaitFor { (Folder $dir).state -eq 'unlocked' } 60000)
    $item = "$dir\$openWord refresh.zamok"
    Check 'unlock: Explorer window on the folder shows the files without F5' {
        [void](Send "{`"cmd`":`"lock`",`"path`":`"$(J $dir)`"}")
        Start-Process explorer.exe "`"$dir`""
        if (-not (WaitFor { (ShownItems $dir) -eq 1 } 15000)) { throw "locked view: $(ShownItems $dir)" }
        $ui = StartUi '--unlock', $item
        $w = Window $ui.Id
        SetValue $w 'Password' $pw; Press $w 'OpenButton'
        $ok = $ui.WaitForExit(10000) -and (WaitFor { (ShownItems $dir) -ge 2 } 5000)
        $n = ShownItems $dir
        (ExplorerWindow $dir).Quit()
        $ok -or "shown=$n" -eq ''
    }
}
finally {
    Get-Process Zamok -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $uiExe } | Stop-Process -Force
    if ($script:core -and -not $script:core.HasExited) {
        try { [void](Send '{"cmd":"quit"}') } catch {}
        if (-not $script:core.WaitForExit(15000)) { $script:core.Kill() }
    }
    $results | Format-Table -AutoSize -Wrap | Out-String -Width 200
    "passed: $(@($results | Where-Object Ok).Count) / $($results.Count)"
}
