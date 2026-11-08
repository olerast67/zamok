# Ustanovka Zamok dlya razrabotki i alfy: sobrat, skopirovat v %LOCALAPPDATA%\Programs\Zamok,
# zaregistrirovat v HKCU (bez prav administratora) i zapustit zamok-core.
#
# Chto pishetsya v reestr (tolko HKCU, vse udalyaet uninstall-dev.ps1):
#   Software\Classes\Directory\shell\ZamokEncrypt   punkt "Zashifrovat parolem" (deystvie po umolchaniyu papok ne menyaetsya)
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
$isRu = (Get-UICulture).TwoLetterISOLanguageName -eq 'ru'
# "Zashifrovat parolem", "Papka Zamok"
$verbText = if ($isRu) { Ru 0x0417,0x0430,0x0448,0x0438,0x0444,0x0440,0x043E,0x0432,0x0430,0x0442,0x044C,0x20,0x043F,0x0430,0x0440,0x043E,0x043B,0x0435,0x043C } else { 'Encrypt with password' }
$typeText = if ($isRu) { (Ru 0x041F,0x0430,0x043F,0x043A,0x0430) + ' Zamok' } else { 'Zamok folder' }

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
    & "$env:USERPROFILE\.cargo\bin\cargo.exe" build -q --release -p zamok-core
    if ($LASTEXITCODE) { throw 'cargo build failed' }
    Pop-Location
    Write-Host 'Publishing Zamok.exe (C#, self-contained)...'
    Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
    & dotnet publish "$repo\ui\Zamok.UI\Zamok.UI.csproj" -c Release -o $stage -v q | Out-Null
    if ($LASTEXITCODE) { throw 'dotnet publish failed' }
    Copy-Item "$repo\core\target\release\zamok-core.exe" $stage
    Copy-Item "$repo\assets\zamok.ico", "$repo\assets\zamok-open.ico" $stage
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
SetKey "$cls\Directory\shell\ZamokEncrypt" @{ 'MUIVerb' = $verbText; 'Icon' = "$exe,0" }
SetKey "$cls\Directory\shell\ZamokEncrypt\command" @{ '(default)' = "`"$exe`" --encrypt `"%1`"" }
SetKey "$cls\.zamok" @{ '(default)' = 'Zamok.OpenItem' }
SetKey "$cls\Zamok.OpenItem" @{ '(default)' = $typeText; 'FriendlyTypeName' = $typeText }
SetKey "$cls\Zamok.OpenItem\DefaultIcon" @{ '(default)' = "$exe,0" }
SetKey "$cls\Zamok.OpenItem\shell\open\command" @{ '(default)' = "`"$exe`" --unlock `"%1`"" }
Set-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name 'Zamok' -Value "`"$core`""

# Zapis ob ustanovke: uninstall-dev.ps1 udalyaet tolko eto
@{
    version = 1
    installed = (Get-Date).ToString('s')
    commit = (& git -C $repo rev-parse --short HEAD 2>$null)
    dir = $InstallDir
    keys = @("$cls\Directory\shell\ZamokEncrypt", "$cls\.zamok", "$cls\Zamok.OpenItem")
    run = 'Zamok'
} | ConvertTo-Json | Set-Content "$InstallDir\install.json" -Encoding UTF8

# Provodnik dolzhen perechitat sopostavleniya i znachki
Add-Type -Namespace ZamokInstall -Name Shell -MemberDefinition '[DllImport("shell32.dll")] public static extern void SHChangeNotify(int e, uint f, IntPtr a, IntPtr b);'
[ZamokInstall.Shell]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)

if (-not $NoStart) {
    Start-Process $core
    $deadline = (Get-Date).AddSeconds(15)
    while (-not (Test-Path "\\.\pipe\$pipe") -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 200 }
    if (-not (Test-Path "\\.\pipe\$pipe")) { throw 'zamok-core did not start: see %LOCALAPPDATA%\Zamok\core.log' }
}
Write-Host "Zamok installed to $InstallDir"
