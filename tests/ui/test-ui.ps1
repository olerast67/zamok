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
foreach ($lang in 'ru-RU', 'en-US') {
    [xml]$x = Get-Content "$repo\ui\Zamok.UI\Strings\$lang\Resources.resw" -Raw -Encoding UTF8
    foreach ($d in $x.root.data) { if (-not $strings[$d.name]) { $strings[$d.name] = @() }; $strings[$d.name] += $d.value }
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
function StartUi([string[]]$uiArgs, [string]$pipe = $pipeName) {
    $psi = New-Object Diagnostics.ProcessStartInfo $uiExe
    $psi.UseShellExecute = $false
    $psi.Arguments = ($uiArgs | ForEach-Object { '"' + $_ + '"' }) -join ' '
    $psi.EnvironmentVariables['ZAMOK_PIPE'] = $pipe
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
        SetValue $w 'Password2' $pw; Start-Sleep -Milliseconds 300
        (IsText 'Encrypt_Strength4' (Text $w 'StrengthText')) -and (Enabled $w 'NextButton')
    }
    Press $w 'NextButton'
    [void](WaitFor { (Text $w 'KeyWords') -match '44\.' } 20000)
    $keyText = Text $w 'KeyWords'
    $words = @([regex]::Matches($keyText, '\d+\.\s+(\S+)') | ForEach-Object { $_.Groups[1].Value })
    Check 'wizard: recovery key shows 44 numbered words' { $words.Count -eq 44 }
    Press $w 'NextButton'
    Start-Sleep -Milliseconds 500
    Check 'wizard: wrong words are rejected' {
        1..4 | ForEach-Object { SetValue $w "Word$_" 'wrong' }
        Press $w 'NextButton'; Start-Sleep -Milliseconds 300
        IsText 'Encrypt_WordsWrong' (Text $w 'ErrorText')
    }
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
    Check 'unlock: wrong password keeps the window and says so' {
        SetValue $w 'Password' 'not the password'; Press $w 'OpenButton'
        (WaitFor { IsText 'Error_wrong_password' (Text $w 'ErrorText') } 10000) -and -not $ui.HasExited -and (Folder $dir).state -eq 'locked'
    }
    Check 'unlock: right password opens the folder and closes the window' {
        SetValue $w 'Password' $pw; Press $w 'OpenButton'
        $ui.WaitForExit(10000) -and (Folder $dir).state -eq 'unlocked' -and (Test-Path "$dir\note.txt")
    }
    Check 'unlock: already open folder closes the window by itself' {
        $ui = StartUi '--unlock', $item
        $ui.WaitForExit(15000)
    }

    # 3. Klyuch vosstanovleniya
    [void](Send "{`"cmd`":`"lock`",`"path`":`"$(J $dir)`"}")
    $ui = StartUi '--unlock', $dir
    $w = Window $ui.Id
    Check 'recovery: the key opens the folder' {
        Press $w 'ModeLink'; Start-Sleep -Milliseconds 300
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
    Check 'show key: the password shows the same 44 words' {
        SetValue $w 'Password' $pw2; Start-Sleep -Milliseconds 300; Press $w 'OkButton'
        if (-not (WaitFor { (Text $w 'KeyWords') -match '44\.' } 10000)) { throw 'no key' }
        $shown = @([regex]::Matches((Text $w 'KeyWords'), '\d+\.\s+(\S+)') | ForEach-Object { $_.Groups[1].Value })
        ($shown -join ' ') -eq ($words -join ' ')
    }
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
        $offer = @((Send '{"cmd":"status"}').settings.defender_offer)
        $saved = Get-Clipboard -Raw -ErrorAction SilentlyContinue
        Press $w 'DefenderCopy'
        $copied = $null; [void](WaitFor { $script:copied = Get-Clipboard -Raw; $script:copied -like 'Add-MpPreference*' } 5000)
        if ($saved) { Set-Clipboard -Value $saved }
        $env:ZT_CMD = $script:copied
        $env:ZT_WANT = $offer -join '|'
        $got = & powershell -NoProfile -Command 'function Add-MpPreference { param([string[]]$ExclusionPath) $ExclusionPath }; if (((Invoke-Expression $env:ZT_CMD) -join ''|'') -eq $env:ZT_WANT) { ''same'' } else { ''diff'' }'
        if ($got -ne 'same') { throw "command gives other paths: $got" }
        $offer.Count -ge 1 -and $offer -contains "$root\.$ruName.zamok" -and -not $script:copied.Contains($root) -and
            (WaitFor { @((Send '{"cmd":"status"}').settings.defender_offer).Count -eq 0 } 5000) -and
            (IsText 'Defender_Copied' (Text $w 'StatusText'))
    }
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
