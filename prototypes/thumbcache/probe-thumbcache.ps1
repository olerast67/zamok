# Issledovanie 05: popadayut li miniatyury iz papki Zamok v kesh miniatyur Windows
# (thumbcache_*.db) i ostayutsya li tam posle zapiraniya.
# Skript zapuskaet nastoyashchiy zamok-core so svoey papkoy dannyh i svoim kanalom (kak
# tests\ui\test-ui.ps1), shifruet papku s kartinkoy vnutri tests\tmp\thumbs i prosit miniatyuru
# cherez IThumbnailCache (tot zhe kesh, chto u Provodnika, bez okon Provodnika).
# Kontrol: ta zhe kartinka v obychnoy papke NTFS.
# Pobochnyy effekt: v kesh miniatyur polzovatelya popadayut dve miniatyury sinteticheskoy kartinki
# (cvetnye polosy), kak pri prosmotre papki v Provodnike. Nastroyki Windows ne menyayutsya.
# Zapusk: powershell -NoProfile -ExecutionPolicy Bypass -File prototypes\thumbcache\probe-thumbcache.ps1
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path "$PSScriptRoot/../..").Path
$root = "$repo\tests\tmp\thumbs"
$coreExe = "$repo\core\target\release\zamok-core.exe"
$pipeName = "zamok-thumbs-$PID"
$pw = 'Korova letit na lunu 2026'

Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
[ComImport, Guid("43826d1e-e718-42ee-bc55-a1e261c37bfe"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IShellItemX { }
[ComImport, Guid("F676C15D-596A-4ce2-8234-33996F445DB1"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IThumbnailCacheX {
  [PreserveSig] int GetThumbnail(IShellItemX item, uint size, uint flags, out IntPtr bitmap, out uint outFlags, out Guid id);
  [PreserveSig] int GetThumbnailByID(Guid id, uint size, out IntPtr bitmap, out uint outFlags);
}
public static class Thumbs {
  [DllImport("shell32.dll", CharSet = CharSet.Unicode, PreserveSig = false)]
  static extern void SHCreateItemFromParsingName(string path, IntPtr bc, ref Guid riid, out IShellItemX item);
  static IThumbnailCacheX Cache() {
    return (IThumbnailCacheX)Activator.CreateInstance(Type.GetTypeFromCLSID(new Guid("50EF4544-AC9F-4A8E-B21B-8A26180DB13F")));
  }
  // flags: 0 WTS_EXTRACT (kak Provodnik), 1 WTS_INCACHEONLY. Otvet: "hr=0x.. cached=.. id=.."
  public static string Get(string path, uint flags, out Guid id) {
    var iid = new Guid("43826d1e-e718-42ee-bc55-a1e261c37bfe"); IShellItemX item;
    SHCreateItemFromParsingName(path, IntPtr.Zero, ref iid, out item);
    IntPtr bmp; uint of;
    int hr = Cache().GetThumbnail(item, 96, flags, out bmp, out of, out id);
    if (bmp != IntPtr.Zero) Marshal.Release(bmp);
    return string.Format("hr=0x{0:X8} cached={1}", hr, (of & 2) != 0);
  }
  // Miniatyura po nomeru, bez samogo fayla: S_OK znachit, chto ona lezhit v keshe
  public static int ById(Guid id) {
    IntPtr bmp; uint of;
    int hr = Cache().GetThumbnailByID(id, 96, out bmp, out of);
    if (bmp != IntPtr.Zero) Marshal.Release(bmp);
    return hr;
  }
}
'@

function Picture([string]$path) {
    # Sluchaynye polosy: kazhdyy zapusk novaya kartinka, staryy kesh ne meshaet
    $rnd = New-Object Random
    $bmp = New-Object Drawing.Bitmap 400, 300
    $g = [Drawing.Graphics]::FromImage($bmp)
    for ($x = 0; $x -lt 400; $x += 20) {
        $b = New-Object Drawing.SolidBrush ([Drawing.Color]::FromArgb($rnd.Next(256), $rnd.Next(256), $rnd.Next(256)))
        $g.FillRectangle($b, $x, 0, 20, 300); $b.Dispose()
    }
    $g.Dispose(); $bmp.Save($path, [Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
}
function WaitFor([scriptblock]$c, [int]$ms = 60000) {
    $d = (Get-Date).AddMilliseconds($ms)
    while (-not $(try { & $c } catch { $false })) { if ((Get-Date) -gt $d) { throw "timeout: $c" }; Start-Sleep -Milliseconds 200 }
}
function Send([string]$json) {
    $c = New-Object IO.Pipes.NamedPipeClientStream '.', $pipeName, 'InOut'; $c.Connect(5000)
    $enc = New-Object Text.UTF8Encoding $false
    $w = New-Object IO.StreamWriter $c, $enc; $w.AutoFlush = $true; $r = New-Object IO.StreamReader $c, $enc
    $w.WriteLine($json); $line = $r.ReadLine(); $c.Dispose(); $line | ConvertFrom-Json
}
function J([string]$s) { $s.Replace('\', '\\') }
function Probe([string]$label, [string]$file) {
    $id = [Guid]::Empty
    $before = [Thumbs]::Get($file, 1, [ref]$id)
    $made = [Thumbs]::Get($file, 0, [ref]$id)
    $after = [Thumbs]::Get($file, 1, [ref]$id)
    $attrs = (Get-Item -LiteralPath $file).Attributes
    Write-Host "${label}: attributes [$attrs]"
    Write-Host "${label}: in cache before: $before"
    Write-Host "${label}: extract like Explorer: $made"
    Write-Host "${label}: in cache after: $after"
    $id
}

Remove-Item $root -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$root\plain", "$root\secret" | Out-Null
Picture "$root\plain\photo.png"
Picture "$root\secret\photo.png"

$psi = New-Object Diagnostics.ProcessStartInfo $coreExe
$psi.UseShellExecute = $false
$psi.EnvironmentVariables['ZAMOK_DATA_DIR'] = "$root\data"
$psi.EnvironmentVariables['ZAMOK_PIPE'] = "\\.\pipe\$pipeName"
$psi.EnvironmentVariables['ZAMOK_NO_TRAY'] = '1'
$psi.EnvironmentVariables['ZAMOK_SEARCH_DRY_RUN'] = '1'
$core = [Diagnostics.Process]::Start($psi)
try {
    WaitFor { Test-Path "\\.\pipe\$pipeName" } 15000
    $dir = "$root\secret"
    [void](Send "{`"cmd`":`"encrypt`",`"path`":`"$(J $dir)`",`"password`":`"$pw`"}")
    WaitFor { ((Send '{"cmd":"status"}').folders | Where-Object { $_.path -eq $dir }).state -eq 'unlocked' }

    $plainId = Probe 'NTFS' "$root\plain\photo.png"
    $zamokId = Probe 'Zamok' "$dir\photo.png"
    [void](Send "{`"cmd`":`"lock`",`"path`":`"$(J $dir)`"}")
    WaitFor { ((Send '{"cmd":"status"}').folders | Where-Object { $_.path -eq $dir }).state -eq 'locked' } 15000
    "Zamok: folder locked, photo.png visible: $(Test-Path "$dir\photo.png")"
    "Zamok: thumbnail by id after lock: hr=0x{0:X8} (0 = still in the cache)" -f [Thumbs]::ById($zamokId)
    "NTFS: thumbnail by id: hr=0x{0:X8}" -f [Thumbs]::ById($plainId)
}
finally {
    try { [void](Send '{"cmd":"quit"}') } catch {}
    if (-not $core.WaitForExit(20000)) { $core.Kill() }
}
