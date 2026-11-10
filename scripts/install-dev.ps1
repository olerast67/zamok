# Ustanovka Zamok dlya razrabotki i alfy: sobrat, skopirovat v %LOCALAPPDATA%\Programs\Zamok,
# zaregistrirovat v HKCU (bez prav administratora) i zapustit zamok-core.
#
# Chto pishetsya v reestr (tolko HKCU, vse udalyaet uninstall-dev.ps1):
#   Software\Classes\Directory\shell\ZamokEncrypt   punkt "Zashifrovat parolem" (deystvie po umolchaniyu papok ne menyaetsya)
#   Software\Classes\Directory\shell\ZamokLock, ZamokUnlock   "Zaperet" i "Otkryt parolem" tolko na papkah Zamok
#     (uslovie AppliesTo perepisyvaet zamok-core, kogda papki otkryvayutsya i zapirayutsya)
#   Software\Classes\.zamok, Software\Classes\Zamok.OpenItem   dvoynoy klik po znachku "Otkryt <Imya>.zamok"
#   Software\Microsoft\Windows\CurrentVersion\Run\Zamok   zapusk zamok-core pri vhode
#
# Zapusk: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\install-dev.ps1
# Skript v ASCII: russkie stroki sobirayutsya iz kodov simvolov.
param(
    [string]$InstallDir = "$env:LOCALAPPDATA\Programs\Zamok",
    [switch]$NoBuild,
    [switch]$NoStart
)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path "$PSScriptRoot\..").Path
function Ru([int[]]$c) { -join ($c | ForEach-Object { [char]$_ }) }
# Yazyk interfeysa Windows napryamuyu: Get-UICulture v processe, zapushchennom iz drugoy
# obolochki, mozhet vernut angliyskiy
Add-Type -Namespace ZamokInstall -Name Lang -MemberDefinition '[DllImport("kernel32.dll")] public static extern ushort GetUserDefaultUILanguage();'
$isRu = ([ZamokInstall.Lang]::GetUserDefaultUILanguage() -band 0x3FF) -eq 0x19
# "Zashifrovat parolem", "Papka Zamok"
$verbText = if ($isRu) { Ru 0x0417,0x0430,0x0448,0x0438,0x0444,0x0440,0x043E,0x0432,0x0430,0x0442,0x044C,0x20,0x043F,0x0430,0x0440,0x043E,0x043B,0x0435,0x043C } else { 'Encrypt with password' }
$typeText = if ($isRu) { (Ru 0x041F,0x0430,0x043F,0x043A,0x0430) + ' Zamok' } else { 'Zamok folder' }
# "Zaperet", "Otkryt parolem"
$lockText = if ($isRu) { Ru 0x0417,0x0430,0x043F,0x0435,0x0440,0x0435,0x0442,0x044C } else { 'Lock' }
# "Smenit parol", "Pokazat klyuch vosstanovleniya"
$changeText = if ($isRu) { Ru 0x0421,0x043C,0x0435,0x043D,0x0438,0x0442,0x044C,0x20,0x043F,0x0430,0x0440,0x043E,0x043B,0x044C } else { 'Change password' }
$showKeyText = if ($isRu) { Ru 0x041F,0x043E,0x043A,0x0430,0x0437,0x0430,0x0442,0x044C,0x20,0x043A,0x043B,0x044E,0x0447,0x20,0x0432,0x043E,0x0441,0x0441,0x0442,0x0430,0x043D,0x043E,0x0432,0x043B,0x0435,0x043D,0x0438,0x044F } else { 'Show recovery key' }
# "Rasshifrovat navsegda"
$decryptText = if ($isRu) { Ru 0x0420,0x0430,0x0441,0x0448,0x0438,0x0444,0x0440,0x043E,0x0432,0x0430,0x0442,0x044C,0x20,0x043D,0x0430,0x0432,0x0441,0x0435,0x0433,0x0434,0x0430 } else { 'Decrypt permanently' }
$unlockText = if ($isRu) { Ru 0x041E,0x0442,0x043A,0x0440,0x044B,0x0442,0x044C,0x20,0x043F,0x0430,0x0440,0x043E,0x043B,0x0435,0x043C } else { 'Open with password' }

if ((Split-Path $InstallDir -Leaf) -ne 'Zamok') { throw "InstallDir must end with \Zamok: $InstallDir" }

# WinFsp nuzhen dlya podklyucheniya papok
$winfsp = Get-ItemProperty 'HKLM:\SOFTWARE\WOW6432Node\WinFsp' -ErrorAction SilentlyContinue
if (-not $winfsp) { throw 'WinFsp is not installed: https://winfsp.dev/rel/' }

# .zamok uzhe prinadlezhit drugoy programme: nichego ne trogaem
$cls = 'HKCU:\Software\Classes'
$cur = (Get-ItemProperty "$cls\.zamok" -ErrorAction SilentlyContinue).'(default)'
if ($cur -and $cur -ne 'Zamok.OpenItem') { throw ".zamok is already registered by another program ($cur)" }

# Sborka
$stage = "$repo\tests\tmp\install-stage"
if (-not $NoBuild) {
    Write-Host 'Building zamok-core (Rust, release)...'
    Push-Location "$repo\core"
    & "$env:USERPROFILE\.cargo\bin\cargo.exe" build -q --release -p zamok-core -p zamok-shell
    if ($LASTEXITCODE) { throw 'cargo build failed' }
    Pop-Location
    Write-Host 'Publishing Zamok.exe (C#, self-contained)...'
    Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
    & dotnet publish "$repo\ui\Zamok.UI\Zamok.UI.csproj" -c Release -o $stage -v q | Out-Null
    if ($LASTEXITCODE) { throw 'dotnet publish failed' }
    Copy-Item "$repo\core\target\release\zamok-core.exe" $stage
    # Rasshirenie Provodnika: core kopiruet ego v svoyu papku i registriruet sam
    Copy-Item "$repo\core\target\release\zamok_shell.dll" "$stage\zamok-shell.dll"
    Copy-Item "$repo\assets\zamok.ico", "$repo\assets\zamok-open.ico" $stage
    # Znachki iz znachkov etoy Windows (papka i zamok iz imageres.dll) sobiraet sam Zamok.exe
    $mk = Start-Process "$stage\Zamok.exe" -ArgumentList '--make-icons', "`"$stage`"" -PassThru -Wait
    if (-not (Test-Path "$stage\folder-locked.ico")) {
        Write-Host "Could not build the icons (exit $($mk.ExitCode)): using the built-in ones"
        Copy-Item "$repo\assets\zamok.ico" "$stage\folder-locked.ico"
        Copy-Item "$repo\assets\zamok.ico" "$stage\open-item.ico"
    }
}
if (-not (Test-Path "$stage\Zamok.exe") -or -not (Test-Path "$stage\zamok-core.exe")) { throw "nothing to install in $stage" }

# Ostanovit ustanovlennyy core (on zapret papki) pered zamenoy faylov
$sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$pipe = "zamok-$sid"
function Quit-Core {
    if (-not (Test-Path "\\.\pipe\$pipe")) { return }
    try {
        $c = New-Object IO.Pipes.NamedPipeClientStream '.', $pipe, 'InOut'; $c.Connect(3000)
        $w = New-Object IO.StreamWriter $c; $w.AutoFlush = $true; $w.WriteLine('{"cmd":"quit"}')
        $r = New-Object IO.StreamReader $c; [void]$r.ReadLine(); $c.Dispose()
    } catch {}
    $deadline = (Get-Date).AddSeconds(20)
    while ((Test-Path "\\.\pipe\$pipe") -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 200 }
}
Quit-Core
Get-Process zamok-core, Zamok -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$InstallDir\*" } | Stop-Process -Force
Start-Sleep -Milliseconds 300

# Fayly
New-Item -ItemType Directory -Force $InstallDir | Out-Null
& robocopy $stage $InstallDir /MIR /NFL /NDL /NJH /NJS /NP | Out-Null
if ($LASTEXITCODE -ge 8) { throw "robocopy failed ($LASTEXITCODE)" }
$exe = "$InstallDir\Zamok.exe"
$core = "$InstallDir\zamok-core.exe"

# Reestr (HKCU)
function SetKey([string]$path, [hashtable]$values) {
    New-Item -Path $path -Force | Out-Null
    foreach ($k in $values.Keys) { Set-ItemProperty -Path $path -Name $k -Value $values[$k] }
}
$nothing = 'System.ItemPathDisplay:="::zamok-none::"'
SetKey "$cls\Directory\shell\ZamokEncrypt" @{ 'MUIVerb' = $verbText; 'Icon' = "$InstallDir\zamok.ico" }
SetKey "$cls\Directory\shell\ZamokEncrypt\command" @{ '(default)' = "`"$exe`" --encrypt `"%1`"" }
SetKey "$cls\Directory\shell\ZamokLock" @{ 'MUIVerb' = $lockText; 'Icon' = "$InstallDir\zamok.ico"; 'AppliesTo' = $nothing }
SetKey "$cls\Directory\shell\ZamokLock\command" @{ '(default)' = "`"$exe`" --lock `"%1`"" }
SetKey "$cls\Directory\shell\ZamokUnlock" @{ 'MUIVerb' = $unlockText; 'Icon' = "$InstallDir\zamok.ico"; 'AppliesTo' = $nothing }
SetKey "$cls\Directory\shell\ZamokUnlock\command" @{ '(default)' = "`"$exe`" --unlock `"%1`"" }
# Podmenyu "Zamok" na papkah Zamok: redkie deystviya, chtoby ne zagromozhdat menyu
SetKey "$cls\Directory\shell\ZamokMore" @{ 'MUIVerb' = 'Zamok'; 'Icon' = "$InstallDir\zamok.ico"; 'SubCommands' = ''; 'AppliesTo' = $nothing }
SetKey "$cls\Directory\shell\ZamokMore\shell\1ChangePassword" @{ 'MUIVerb' = $changeText }
SetKey "$cls\Directory\shell\ZamokMore\shell\1ChangePassword\command" @{ '(default)' = "`"$exe`" --change-password `"%1`"" }
SetKey "$cls\Directory\shell\ZamokMore\shell\2ShowKey" @{ 'MUIVerb' = $showKeyText }
SetKey "$cls\Directory\shell\ZamokMore\shell\2ShowKey\command" @{ '(default)' = "`"$exe`" --show-key `"%1`"" }
SetKey "$cls\Directory\shell\ZamokMore\shell\3Decrypt" @{ 'MUIVerb' = $decryptText }
SetKey "$cls\Directory\shell\ZamokMore\shell\3Decrypt\command" @{ '(default)' = "`"$exe`" --decrypt `"%1`"" }
SetKey "$cls\.zamok" @{ '(default)' = 'Zamok.OpenItem' }
SetKey "$cls\Zamok.OpenItem" @{ '(default)' = $typeText; 'FriendlyTypeName' = $typeText }
SetKey "$cls\Zamok.OpenItem\DefaultIcon" @{ '(default)' = "$InstallDir\zamok.ico" }
SetKey "$cls\Zamok.OpenItem\shell\open\command" @{ '(default)' = "`"$exe`" --unlock `"%1`"" }
# Avtozapusk cherez storozha: on perezapuskaet core, esli tot upal
Set-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name 'Zamok' -Value "`"$core`" --watch"

# Zapis ob ustanovke: uninstall-dev.ps1 udalyaet tolko eto
@{
    version = 1
    installed = (Get-Date).ToString('s')
    commit = (& git -C $repo rev-parse --short HEAD 2>$null)
    dir = $InstallDir
    keys = @("$cls\Directory\shell\ZamokEncrypt", "$cls\Directory\shell\ZamokLock", "$cls\Directory\shell\ZamokUnlock", "$cls\Directory\shell\ZamokMore", "$cls\.zamok", "$cls\Zamok.OpenItem", "$cls\CLSID\{3F7A9C12-8E4B-4D61-A5C7-19E2B60D4F83}", "$cls\Directory\shellex\CopyHookHandlers\Zamok")
    run = 'Zamok'
} | ConvertTo-Json | Set-Content "$InstallDir\install.json" -Encoding UTF8

# Provodnik dolzhen perechitat sopostavleniya i znachki
Add-Type -Namespace ZamokInstall -Name Shell -MemberDefinition '[DllImport("shell32.dll")] public static extern void SHChangeNotify(int e, uint f, IntPtr a, IntPtr b);'
[ZamokInstall.Shell]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)

if (-not $NoStart) {
    Start-Process $core -ArgumentList '--watch'
    $deadline = (Get-Date).AddSeconds(15)
    while (-not (Test-Path "\\.\pipe\$pipe") -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 200 }
    if (-not (Test-Path "\\.\pipe\$pipe")) { throw 'zamok-core did not start: see %LOCALAPPDATA%\Zamok\core.log' }
}
Write-Host "Zamok installed to $InstallDir"
