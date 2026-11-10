# Sobrat ustanovshchik Zamok (MSI): zamok-core (Rust release), Zamok.exe (C# publish) i WiX 5
# (lokalnyy instrument repozitoriya, dotnet-tools.json). Rezultat: installer\out\Zamok-<versiya>.msi
# Zapusk: powershell -NoProfile -ExecutionPolicy Bypass -File installer\build-msi.ps1
param([string]$Version = '0.1.0')
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path "$PSScriptRoot\..").Path
$out = "$repo\installer\out"
$app = "$out\app"

# Rust: zakreplennyy kompilyator, --locked, bez lokalnykh putey (RELEASING.md, vosproizvodimost)
& "$repo\scripts\build-core-release.ps1" -Packages zamok-core, zamok-shell

Remove-Item $app -Recurse -Force -ErrorAction SilentlyContinue
# Deterministic: bez vremeni sborki v fayle; ContinuousIntegrationBuild: puti iskhodnikov
# zamenyayutsya na /_/, v Zamok.exe ne popadaet papka repozitoriya
& dotnet publish "$repo\ui\Zamok.UI\Zamok.UI.csproj" -c Release -o $app -v q `
    -p:Deterministic=true -p:ContinuousIntegrationBuild=true | Out-Null
if ($LASTEXITCODE) { throw 'dotnet publish failed' }
Copy-Item "$repo\core\target\release\zamok-core.exe" $app
Copy-Item "$repo\core\target\release\zamok_shell.dll" "$app\zamok-shell.dll"
# Zapasnye znachki; nastoyashchie Zamok.exe sobiraet pri ustanovke iz znachkov Windows
Copy-Item "$repo\assets\zamok.ico", "$repo\assets\zamok-open.ico" $app
Get-ChildItem $app -Filter *.pdb | Remove-Item
# Nashi fayly ne dolzhny soderzhat lichnykh putey sborochnoy mashiny
$own = @('zamok-core.exe', 'zamok-shell.dll', 'Zamok.exe', 'Zamok.dll', 'Zamok.Client.dll') | ForEach-Object { "$app\$_" }
& "$repo\scripts\check-paths.ps1" @own
if ($LASTEXITCODE) { throw 'local paths found in release binaries' }

Push-Location $repo
& dotnet tool restore | Out-Null
& dotnet wix build "$repo\installer\Zamok.wxs" -arch x64 -d "AppDir=$app" -d "Version=$Version" -o "$out\Zamok-$Version.msi"
$code = $LASTEXITCODE
Pop-Location
if ($code) { throw "wix build failed ($code)" }
Write-Host "Built $out\Zamok-$Version.msi"

# Obshchiy ustanovshchik: neizmenennyy MSI WinFsp + Zamok.msi. Fayl WinFsp ne v git: skachivaetsya
# po tochnomu adresu i prinimaetsya tolko s etim otpechatkom i deystvitelnoy podpisyu izdatelya.
$winfspName = 'winfsp-2.1.25156.msi'
$winfspUrl = "https://github.com/winfsp/winfsp/releases/download/v2.1/$winfspName"
$winfspSha = '073A70E00F77423E34BED98B86E600DEF93393BA5822204FAC57A29324DB9F7A'
$deps = "$out\deps"
$winfsp = "$deps\$winfspName"
New-Item -ItemType Directory -Force $deps | Out-Null
if (-not (Test-Path $winfsp)) {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    Invoke-WebRequest -UseBasicParsing -Uri $winfspUrl -OutFile $winfsp
}
if ((Get-FileHash $winfsp -Algorithm SHA256).Hash -ne $winfspSha) { throw "$winfspName has a wrong SHA-256" }
$sig = Get-AuthenticodeSignature $winfsp
if ($sig.Status -ne 'Valid' -or $sig.SignerCertificate.Subject -notlike 'CN=NAVIMATICS LLC,*') { throw "$winfspName has a wrong signature" }

# Logotip okna ustanovshchika: 64 tochki, iz znachka programmy
Add-Type -AssemblyName System.Drawing
$logo = "$deps\logo-64.png"
$src = [Drawing.Image]::FromFile("$repo\assets\zamok-256.png")
$bmp = New-Object Drawing.Bitmap 64, 64
$g = [Drawing.Graphics]::FromImage($bmp)
$g.InterpolationMode = 'HighQualityBicubic'
$g.DrawImage($src, 0, 0, 64, 64)
$bmp.Save($logo, [Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose(); $src.Dispose()

Push-Location $repo
& dotnet wix extension add -g WixToolset.Util.wixext/5.0.2 | Out-Null
if ($LASTEXITCODE) { throw 'wix extension failed' }
& dotnet wix extension add -g WixToolset.BootstrapperApplications.wixext/5.0.2 | Out-Null
if ($LASTEXITCODE) { throw 'wix extension failed' }
& dotnet wix build "$repo\installer\Zamok.bundle.wxs" -arch x64 `
    -ext WixToolset.Util.wixext -ext WixToolset.BootstrapperApplications.wixext `
    -d "AppDir=$app" -d "Version=$Version" -d "WinFspMsi=$winfsp" -d "ZamokMsi=$out\Zamok-$Version.msi" `
    -d "Logo=$logo" -d "InstallerDir=$repo\installer" `
    -o "$out\Zamok-Setup-$Version.exe"
$code = $LASTEXITCODE
Pop-Location
if ($code) { throw "wix bundle build failed ($code)" }
Write-Host "Built $out\Zamok-Setup-$Version.exe"

# Otpechatki vsekh faylov reliza v odnom fayle (format sha256sum)
& "$repo\scripts\sha256sums.ps1" -Dir $out -Files "Zamok-Setup-$Version.exe", "Zamok-$Version.msi"
