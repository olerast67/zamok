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
    while (-not (& $cond)) { if ((Get-Date) -gt $deadline) { return $false }; Start-Sleep -Milliseconds 100 }
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
    $p = [Diagnostics.Process]::Start($psi)
    [void](WaitFor { (Test-Path "\\.\pipe\$pipeName") -or $p.HasExited } 15000)
    $p
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
    Check 'wizard: Close closes the window' { Press $w 'NextButton'; $ui.WaitForExit(10000) }

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
    $held.Dispose(); $ui.Kill()
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
