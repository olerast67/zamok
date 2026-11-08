# Polnyy cikl na nastoyashchey sisteme: install-dev.ps1, punkt menyu Provodnika, dvoynoy klik
# po znachku .zamok, uninstall-dev.ps1 bez sledov.
#
# Rabotaet s NASTOYASHCHIM reestrom HKCU i %LOCALAPPDATA%\Programs\Zamok. Zapuskaetsya tolko
# esli Zamok eshche ne ustanovlen; v konce vse udaleno i proveryaetsya, chto deystviya papok
# po umolchaniyu v reestre te zhe, chto do testa. Test-papka tolko v tests\tmp\install.
# Zapusk: powershell -NoProfile -ExecutionPolicy Bypass -File tests\install\test-install.ps1
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$repo = (Resolve-Path "$PSScriptRoot/../..").Path
$root = "$repo\tests\tmp\install"
$installDir = "$env:LOCALAPPDATA\Programs\Zamok"
$dataDir = "$env:LOCALAPPDATA\Zamok"
$cls = 'HKCU:\Software\Classes'
$pw = 'Korova letit na lunu 2026'
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
$pipe = "zamok-$([Security.Principal.WindowsIdentity]::GetCurrent().User.Value)"
function Send([string]$json) {
    $c = New-Object IO.Pipes.NamedPipeClientStream '.', $pipe, 'InOut'; $c.Connect(5000)
    $enc = New-Object Text.UTF8Encoding $false
    $w = New-Object IO.StreamWriter $c, $enc; $w.AutoFlush = $true; $w.WriteLine($json)
    $r = New-Object IO.StreamReader $c, $enc; $line = $r.ReadLine(); $c.Dispose()
    $line | ConvertFrom-Json
}
function J([string]$s) { $s.Replace('\', '\\').Replace('"', '\"') }
function Folder([string]$path) { (Send '{"cmd":"status"}').folders | Where-Object { $_.path -eq $path } }

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$AE = [Windows.Automation.AutomationElement]
function UiWindow([int]$ms = 20000) {
    # Okno Zamok.exe iz papki ustanovki (ego zapuskaet Provodnik, PID zaranee ne izvesten)
    $deadline = (Get-Date).AddMilliseconds($ms)
    while ((Get-Date) -lt $deadline) {
        $p = Get-Process Zamok -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq "$installDir\Zamok.exe" } | Select-Object -First 1
        if ($p) {
            $cond = New-Object Windows.Automation.PropertyCondition ($AE::ProcessIdProperty), $p.Id
            $w = $AE::RootElement.FindFirst('Children', $cond)
            if ($w) { return @{ Proc = $p; Win = $w } }
        }
        Start-Sleep -Milliseconds 200
    }
    $null
}
function El($w, [string]$id) { $w.FindFirst('Descendants', (New-Object Windows.Automation.PropertyCondition ($AE::AutomationIdProperty), $id)) }
function Text($w, [string]$id) { $e = El $w $id; if ($e) { $e.Current.Name } }
function SetValue($w, [string]$id, [string]$v) { (El $w $id).GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).SetValue($v) }
function Press($w, [string]$id) { (El $w $id).GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke() }

# Deystviya papok po umolchaniyu i spisok punktov: snimok do testa
function Snapshot([string]$ignore = '') {
    $o = foreach ($k in 'Directory\shell', 'Folder\shell', 'Directory\Background\shell', 'Drive\shell') {
        $def = (Get-ItemProperty "$cls\$k" -ErrorAction SilentlyContinue).'(default)'
        $subs = @(Get-ChildItem "$cls\$k" -ErrorAction SilentlyContinue | ForEach-Object PSChildName | Where-Object { -not $ignore -or $_ -notmatch "^($ignore)$" } | Sort-Object) -join ','
        "$k default=[$def] subkeys=[$subs]"
    }
    $o -join "`n"
}

# Ne zapuskaemsya poverh nastoyashchey ustanovki
if ((Test-Path $installDir) -or (Test-Path "$cls\.zamok") -or (Test-Path "\\.\pipe\$pipe")) {
    throw 'Zamok is already installed or running: this test only runs on a clean system'
}
$dataExisted = Test-Path $dataDir
$before = Snapshot
Remove-Item $root -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$root\$ruName\sub" | Out-Null
[IO.File]::WriteAllText("$root\$ruName\note.txt", 'install test')
$dir = "$root\$ruName"

try {
    & "$repo\scripts\install-dev.ps1" | Out-Null
    Check 'install: files in Programs\Zamok' { (Test-Path "$installDir\Zamok.exe") -and (Test-Path "$installDir\zamok-core.exe") -and (Test-Path "$installDir\zamok.ico") }
    Check 'install: core runs from the install folder' {
        $p = Get-Process zamok-core -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq "$installDir\zamok-core.exe" }
        $p -and (Send '{"cmd":"status"}').ok
    }
    Check 'install: autostart entry points to the core' {
        (Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run').Zamok -eq "`"$installDir\zamok-core.exe`" --watch"
    }
    Check 'install: .zamok opens Zamok.exe --unlock' {
        (Get-ItemProperty "$cls\.zamok").'(default)' -eq 'Zamok.OpenItem' -and
            (Get-ItemProperty "$cls\Zamok.OpenItem\shell\open\command").'(default)' -eq "`"$installDir\Zamok.exe`" --unlock `"%1`""
    }
    Check 'install: only the Zamok items added, default actions of folders unchanged' {
        $now = Snapshot 'ZamokEncrypt|ZamokLock|ZamokUnlock|ZamokMore'
        if ($now -ne $before -or -not (Test-Path "$cls\Directory\shell\ZamokEncrypt")) { throw "before:`n$before`nnow:`n$(Snapshot)" }
        'same plus the Zamok items'
    }

    # Punkt menyu Provodnika "Zashifrovat parolem"
    $shell = New-Object -ComObject Shell.Application
    $item = $shell.Namespace($root).ParseName($ruName)
    Check 'Explorer: folder menu has the Zamok item' {
        $verbs = @($item.Verbs() | ForEach-Object { $_.Name })
        $want = (Get-ItemProperty "$cls\Directory\shell\ZamokEncrypt").MUIVerb
        $verbs -contains $want
    }
    $item.InvokeVerb('ZamokEncrypt')
    $ui = UiWindow
    Check 'Explorer menu item opens the encryption wizard' { $ui -and (Text $ui.Win 'Heading') -like "*$ruName*" }
    if ($ui) {
        $w = $ui.Win
        SetValue $w 'Password1' $pw; SetValue $w 'Password2' $pw; Start-Sleep -Milliseconds 300
        Press $w 'NextButton'
        [void](WaitFor { (Text $w 'KeyWords') -match '44\.' } 20000)
        $words = @([regex]::Matches((Text $w 'KeyWords'), '\d+\.\s+(\S+)') | ForEach-Object { $_.Groups[1].Value })
        Press $w 'NextButton'; Start-Sleep -Milliseconds 500
        foreach ($i in 1..4) {
            $n = [int]([regex]::Match((El $w "Word$i").Current.Name, '\d+').Value)
            SetValue $w "Word$i" $words[$n - 1]
        }
        Press $w 'NextButton'
        Check 'wizard from Explorer encrypts the folder' {
            (WaitFor { (Text $w 'ProgressTitle') -like "*$ruName*" } 60000) -and (Folder $dir).state -eq 'unlocked'
        }
        Press $w 'NextButton'; [void]$ui.Proc.WaitForExit(10000)
    }

    # Punkty menyu po sostoyaniyu papki (uslovie AppliesTo perepisyvaet core)
    $verbName = @{}
    foreach ($v in 'ZamokEncrypt', 'ZamokLock', 'ZamokUnlock') { $verbName[$v] = (Get-ItemProperty "$cls\Directory\shell\$v").MUIVerb }
    function Verbs([string]$name) { @($shell.Namespace($root).ParseName($name).Verbs() | ForEach-Object { $_.Name }) }
    function HasOnly([string]$name, [string]$want) {
        $v = Verbs $name
        $has = @('ZamokEncrypt', 'ZamokLock', 'ZamokUnlock' | Where-Object { $v -contains $verbName[$_] })
        ($has -join ',') -eq $want
    }
    New-Item -ItemType Directory -Force "$root\plain" | Out-Null
    Check 'menu items are in the Windows display language' {
        Add-Type -Namespace ZamokTest -Name Lang -MemberDefinition '[DllImport("kernel32.dll")] public static extern ushort GetUserDefaultUILanguage();'
        $ru = ([ZamokTest.Lang]::GetUserDefaultUILanguage() -band 0x3FF) -eq 0x19
        $t = $verbName['ZamokUnlock']
        if ($ru) { $t -ne 'Open with password' } else { $t -eq 'Open with password' }
    }
    Check 'menu: open Zamok folder has only Lock' { WaitFor { HasOnly $ruName 'ZamokLock' } 8000 }
    # Podmenyu Zamok Shell.Application pokazyvaet bez imeni: proveryaem ego uslovie v reestre
    Check 'menu: Zamok submenu applies to Zamok folders only' {
        $at = (Get-ItemProperty "$cls\Directory\shell\ZamokMore").AppliesTo
        $at.Contains('System.ItemPathDisplay:="' + $dir + '"') -and -not $at.Contains("$root\plain")
    }
    Check 'menu: ordinary folder has only Encrypt' { HasOnly 'plain' 'ZamokEncrypt' }
    Check 'menu: Lock locks the folder without a window' {
        $shell.Namespace($root).ParseName($ruName).InvokeVerb('ZamokLock')
        WaitFor { (Folder $dir).state -eq 'locked' } 15000
    }
    Check 'menu: locked Zamok folder has only Open with password' { WaitFor { HasOnly $ruName 'ZamokUnlock' } 8000 }
    Check 'locked folder icon comes from folder-locked.ico' {
        $ini = [IO.File]::ReadAllText("$dir\desktop.ini", [Text.Encoding]::Unicode)
        $ini -like "*$installDir\folder-locked.ico*"
    }

    # Dvoynoy klik po znachku v zapertoy papke (ShellExecute po sopostavleniyu .zamok)
    [void](Send "{`"cmd`":`"lock`",`"path`":`"$(J $dir)`"}")
    Check 'locked folder shows the open item' { Test-Path "$dir\$openWord $ruName.zamok" }
    $ui = $null
    try { Start-Process "$dir\$openWord $ruName.zamok"; $ui = UiWindow } catch {}
    Check 'double click on the .zamok item opens the password window' { $ui -and (Text $ui.Win 'FolderName') -eq $ruName }
    if ($ui) {
        SetValue $ui.Win 'Password' $pw; Press $ui.Win 'OpenButton'
        Check 'password window opens the folder' { $ui.Proc.WaitForExit(10000) -and (Folder $dir).state -eq 'unlocked' -and (Get-Content "$dir\note.txt") -eq 'install test' }
    }

    # Uninstall bez -Force otkazyvaet, poka est zashifrovannye papki
    Check 'uninstall refuses while encrypted folders exist' {
        try { & "$repo\scripts\uninstall-dev.ps1" 6>$null | Out-Null; $false } catch { $_.Exception.Message -like '*-Force*' }
    }
    # Ubrat test-papku iz spiska (dannye ostayutsya v tests\tmp i udalyayutsya nizhe)
    [void](Send "{`"cmd`":`"remove`",`"path`":`"$(J $dir)`"}")
}
finally {
    Get-Process Zamok -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq "$installDir\Zamok.exe" } | Stop-Process -Force
    if (Test-Path "$installDir\install.json") { & "$repo\scripts\uninstall-dev.ps1" -Force 6>$null | Out-Null }
}

Check 'uninstall: core stopped, files and registry entries gone' {
    -not (Test-Path "\\.\pipe\$pipe") -and -not (Test-Path $installDir) -and -not (Test-Path "$cls\.zamok") -and
        -not (Test-Path "$cls\Zamok.OpenItem") -and -not (Test-Path "$cls\Directory\shell\ZamokEncrypt") -and
        -not (Test-Path "$cls\Directory\shell\ZamokLock") -and -not (Test-Path "$cls\Directory\shell\ZamokUnlock") -and
        -not (Test-Path "$cls\Directory\shell\ZamokMore") -and
        -not (Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run').Zamok
}
Check 'uninstall: registry folder actions exactly as before the test' { $now = Snapshot; if ($now -ne $before) { throw "before:`n$before`nnow:`n$now" }; 'same' }
# Spisok papok sozdal test: udalyaem, esli do testa ego ne bylo
if (-not $dataExisted) { Remove-Item $dataDir -Recurse -Force -ErrorAction SilentlyContinue }
Remove-Item $root -Recurse -Force -ErrorAction SilentlyContinue
$results | Format-Table -AutoSize -Wrap | Out-String -Width 200
"passed: $(@($results | Where-Object Ok).Count) / $($results.Count)"
