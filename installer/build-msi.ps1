# Sobrat ustanovshchik Zamok (MSI): zamok-core (Rust release), Zamok.exe (C# publish) i WiX 5
# (lokalnyy instrument repozitoriya, dotnet-tools.json). Rezultat: installer\out\Zamok-<versiya>.msi
# Zapusk: powershell -NoProfile -ExecutionPolicy Bypass -File installer\build-msi.ps1
param([string]$Version = '0.1.0')
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path "$PSScriptRoot\..").Path
$out = "$repo\installer\out"
$app = "$out\app"

Push-Location "$repo\core"
& "$env:USERPROFILE\.cargo\bin\cargo.exe" build -q --release -p zamok-core
if ($LASTEXITCODE) { throw 'cargo build failed' }
Pop-Location

Remove-Item $app -Recurse -Force -ErrorAction SilentlyContinue
& dotnet publish "$repo\ui\Zamok.UI\Zamok.UI.csproj" -c Release -o $app -v q | Out-Null
if ($LASTEXITCODE) { throw 'dotnet publish failed' }
Copy-Item "$repo\core\target\release\zamok-core.exe" $app
# Zapasnye znachki; nastoyashchie Zamok.exe sobiraet pri ustanovke iz znachkov Windows
Copy-Item "$repo\assets\zamok.ico", "$repo\assets\zamok-open.ico" $app
Get-ChildItem $app -Filter *.pdb | Remove-Item

Push-Location $repo
& dotnet tool restore | Out-Null
& dotnet wix build "$repo\installer\Zamok.wxs" -arch x64 -d "AppDir=$app" -d "Version=$Version" -o "$out\Zamok-$Version.msi"
$code = $LASTEXITCODE
Pop-Location
if ($code) { throw "wix build failed ($code)" }
Write-Host "Built $out\Zamok-$Version.msi"
