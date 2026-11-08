# Udalenie Zamok, ustanovlennogo install-dev.ps1: zaperet i otklyuchit papki, ubrat zapisi
# v reestre i papku programmy. Udalyaetsya tolko to, chto zapisano v install.json.
#
# Zashifrovannye papki ostayutsya zashifrovannymi: dannye v skrytyh papkah .Imya.zamok,
# spisok v %LOCALAPPDATA%\Zamok ne udalyaetsya. Posle povtornoy ustanovki papki snova na meste,
# bez Zamok ih otkryvaet Cryptomator ili zamok-recover. Esli takie papki est, nuzhen -Force.
#
# Zapusk: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\uninstall-dev.ps1 [-Force]
param(
    [string]$InstallDir = "$env:LOCALAPPDATA\Programs\Zamok",
    [switch]$Force
)
$ErrorActionPreference = 'Stop'
$manifest = "$InstallDir\install.json"
if (-not (Test-Path $manifest)) { throw "Zamok is not installed in $InstallDir (no install.json)" }
$info = Get-Content $manifest -Raw | ConvertFrom-Json

$sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$pipe = "zamok-$sid"
function Send([string]$json) {
    $c = New-Object IO.Pipes.NamedPipeClientStream '.', $pipe, 'InOut'; $c.Connect(3000)
    $enc = New-Object Text.UTF8Encoding $false
    $w = New-Object IO.StreamWriter $c, $enc; $w.AutoFlush = $true; $w.WriteLine($json)
    $r = New-Object IO.StreamReader $c, $enc; $line = $r.ReadLine(); $c.Dispose()
    $line | ConvertFrom-Json
}

# Zashifrovannye papki: preduprezhdenie
$folders = @()
if (Test-Path "\\.\pipe\$pipe") { try { $folders = @((Send '{"cmd":"status"}').folders) } catch {} }
elseif (Test-Path "$env:LOCALAPPDATA\Zamok\folders.json") {
    try { $folders = @((Get-Content "$env:LOCALAPPDATA\Zamok\folders.json" -Raw -Encoding UTF8 | ConvertFrom-Json).folders) } catch {}
}
if ($folders.Count -gt 0 -and -not $Force) {
    Write-Host "Encrypted folders stay encrypted and will be unavailable until Zamok is installed again:"
    $folders | ForEach-Object { Write-Host "  $($_.path)" }
    throw 'run again with -Force to uninstall anyway'
}

# Ostanovit core: on zapiraet i otklyuchaet papki
if (Test-Path "\\.\pipe\$pipe") { try { [void](Send '{"cmd":"quit"}') } catch {} }
$deadline = (Get-Date).AddSeconds(20)
while ((Test-Path "\\.\pipe\$pipe") -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 200 }
Get-Process zamok-core, Zamok -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$InstallDir\*" } | Stop-Process -Force
Start-Sleep -Milliseconds 300

# Reestr: tolko nashi klyuchi, .zamok tolko esli on nash
$cls = 'HKCU:\Software\Classes'
foreach ($k in $info.keys) {
    if ($k -eq "$cls\.zamok") {
        $cur = (Get-ItemProperty $k -ErrorAction SilentlyContinue).'(default)'
        if ($cur -and $cur -ne 'Zamok.OpenItem') { Write-Host ".zamok now belongs to $cur, left as is"; continue }
    }
    Remove-Item $k -Recurse -Force -ErrorAction SilentlyContinue
}
$run = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$val = (Get-ItemProperty $run -ErrorAction SilentlyContinue).($info.run)
if ($val -and $val -like "*$InstallDir\zamok-core.exe*") { Remove-ItemProperty $run -Name $info.run }

Add-Type -Namespace ZamokUninstall -Name Shell -MemberDefinition '[DllImport("shell32.dll")] public static extern void SHChangeNotify(int e, uint f, IntPtr a, IntPtr b);'
[ZamokUninstall.Shell]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)

# Papka programmy: tolko esli v ney nash zamok-core.exe
if (Test-Path "$InstallDir\zamok-core.exe") { Remove-Item $InstallDir -Recurse -Force }
Write-Host 'Zamok uninstalled. The folder list in %LOCALAPPDATA%\Zamok is kept.'
