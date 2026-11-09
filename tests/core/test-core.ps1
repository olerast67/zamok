# Etap 4: proverka fonovogo processa zamok-core na nastoyashchey sisteme.
# Core zapuskaetsya kak programma so svoey papkoy dannyh i svoim kanalom, skript govorit s nim
# po kanalu, pishet v papku, shlyot oknu core te zhe soobshcheniya, chto Windows pri Win+L i sne,
# ubivaet process i podsovyvaet mertvuyu tochku podklyucheniya.
# Zapusk: powershell -NoProfile -ExecutionPolicy Bypass -File tests\core\test-core.ps1
# Skript v ASCII: russkie imena sobirayutsya iz kodov simvolov.
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path "$PSScriptRoot/../..").Path
$root = "$repo\tests\tmp\core"
$coreExe = "$repo\core\target\release\zamok-core.exe"
$pw = 'zamok-core-test'
$pipeName = "zamok-test-$PID"
$results = New-Object System.Collections.Generic.List[object]
function Ru([int[]]$c) { -join ($c | ForEach-Object { [char]$_ }) }
$ruName = Ru 0x041A,0x043B,0x0438,0x0435,0x043D,0x0442,0x044B   # Klienty
$openWord = Ru 0x041E,0x0442,0x043A,0x0440,0x044B,0x0442,0x044C  # Otkryt
$data = "$root\data"
$mnt = "$root\$ruName"
$vault = "$root\.$ruName.zamok"
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

Add-Type -TypeDefinition @'
using System; using System.IO; using System.Runtime.InteropServices; using System.Text;
public static class ZT {
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern IntPtr FindWindowExW(IntPtr parent, IntPtr after, string cls, string title);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint msg, IntPtr w, IntPtr l);
  public static IntPtr CoreWindow(int pid) {
    IntPtr h = IntPtr.Zero;
    while ((h = FindWindowExW(IntPtr.Zero, h, "ZamokCoreWindow", null)) != IntPtr.Zero) {
      uint p; GetWindowThreadProcessId(h, out p); if (p == pid) return h;
    }
    return IntPtr.Zero;
  }
  [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
  static extern IntPtr CreateFileW(string n, uint a, uint s, IntPtr sa, uint c, uint f, IntPtr t);
  [DllImport("kernel32.dll", SetLastError = true)]
  static extern bool DeviceIoControl(IntPtr h, uint code, byte[] inb, int inl, byte[] outb, int outl, out int ret, IntPtr ov);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
  // Mertvaya tochka podklyucheniya, kak posle vyklyucheniya pitaniya: tom WinFsp, kotorogo net
  public static void SetMountPoint(string dir, string target) {
    byte[] t = Encoding.Unicode.GetBytes(target);
    int pathLen = t.Length * 2 + 4;
    var buf = new byte[16 + pathLen];
    BitConverter.GetBytes(0xA0000003u).CopyTo(buf, 0);
    BitConverter.GetBytes((ushort)(8 + pathLen)).CopyTo(buf, 4);
    BitConverter.GetBytes((ushort)0).CopyTo(buf, 8);
    BitConverter.GetBytes((ushort)t.Length).CopyTo(buf, 10);
    BitConverter.GetBytes((ushort)(t.Length + 2)).CopyTo(buf, 12);
    BitConverter.GetBytes((ushort)t.Length).CopyTo(buf, 14);
    t.CopyTo(buf, 16); t.CopyTo(buf, 16 + t.Length + 2);
    var h = CreateFileW(dir, 0xC0000000, 7, IntPtr.Zero, 3, 0x02000000 | 0x00200000, IntPtr.Zero);
    if (h == new IntPtr(-1)) throw new IOException("open failed " + Marshal.GetLastWin32Error());
    int r;
    try { if (!DeviceIoControl(h, 0x900A4, buf, buf.Length, null, 0, out r, IntPtr.Zero)) throw new IOException("set failed " + Marshal.GetLastWin32Error()); }
    finally { CloseHandle(h); }
  }
}
'@

$script:proc = $null
function StartCore([string]$crash = '') {
    $psi = New-Object Diagnostics.ProcessStartInfo $coreExe
    if ($crash) { $psi.EnvironmentVariables['ZAMOK_TEST_CRASH'] = $crash }
    $psi.UseShellExecute = $false
    $psi.RedirectStandardError = $true
    $psi.EnvironmentVariables['ZAMOK_DATA_DIR'] = $data
    $psi.EnvironmentVariables['ZAMOK_PIPE'] = "\\.\pipe\$pipeName"
    $psi.EnvironmentVariables['ZAMOK_NO_TRAY'] = '1'
    $psi.EnvironmentVariables['ZAMOK_RECENT_DIR'] = "$root\recent"
    $p = [Diagnostics.Process]::Start($psi)
    [void](WaitFor { (Test-Path "\\.\pipe\$pipeName") -or $p.HasExited } 15000)
    [void](WaitFor { $p.HasExited -or [ZT]::CoreWindow($p.Id) -ne [IntPtr]::Zero } 5000)
    $p
}
function Send([string]$json) {
    $c = New-Object IO.Pipes.NamedPipeClientStream '.', $pipeName, 'InOut'
    $c.Connect(5000)
    $enc = New-Object Text.UTF8Encoding $false
    $w = New-Object IO.StreamWriter $c, $enc; $w.AutoFlush = $true
    $r = New-Object IO.StreamReader $c, $enc
    $w.WriteLine($json)
    $line = $r.ReadLine()
    $c.Dispose()
    $line | ConvertFrom-Json
}
function J([string]$s) { $s.Replace('\', '\\').Replace('"', '\"') }
function State { (Send '{"cmd":"status"}').folders | Where-Object { $_.path -eq $mnt } | Select-Object -ExpandProperty state }
function PostToCore([uint32]$msg, [int]$w) { [void][ZT]::PostMessageW([ZT]::CoreWindow($script:proc.Id), $msg, [IntPtr]$w, [IntPtr]::Zero) }
function SrcCount { @(Get-ChildItem "$repo\tests\fixtures-src\vault1" -Recurse -File -Force).Count }

# Podgotovka
Get-Process zamok-core -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $coreExe } | Stop-Process -Force
Start-Sleep -Milliseconds 500
Remove-Item $root -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $root | Out-Null
Push-Location "$repo\core"
& "$env:USERPROFILE\.cargo\bin\cargo.exe" build -q --release -p zamok-core 2>&1 | Out-Null
& "$env:USERPROFILE\.cargo\bin\cargo.exe" run -q --release --example create_vault -- "$repo\tests\fixtures-src\vault1" $vault $pw | Out-Null
Pop-Location
$recovery = (Get-Content "$vault-recovery.txt" -Raw).Trim()

try {
    $script:proc = StartCore
    Check 'core starts, pipe and window exist' { -not $script:proc.HasExited -and [ZT]::CoreWindow($script:proc.Id) -ne [IntPtr]::Zero }
    Check 'pipe: only the current user has access' {
        # Get-Acl ne chitaet prava kanalov: berem ih u podklyuchennogo klienta
        $c = New-Object IO.Pipes.NamedPipeClientStream '.', $pipeName, 'InOut'
        $c.Connect(5000)
        $rules = $c.GetAccessControl().GetAccessRules($true, $true, [Security.Principal.NTAccount])
        $c.Dispose()
        $ids = @($rules | ForEach-Object { $_.IdentityReference.Value })
        $me = [Security.Principal.WindowsIdentity]::GetCurrent().Name
        if ($ids.Count -ne 1 -or $ids[0] -ne $me) { "access: $($ids -join ', ')" -eq '' } else { $ids[0] }
    }
    Check 'second core with the same pipe refuses to start' {
        $p2 = StartCore
        $ok = $p2.WaitForExit(10000) -and $p2.ExitCode -eq 3
        if (-not $p2.HasExited) { $p2.Kill() }
        $ok -and -not $script:proc.HasExited
    }
    Check 'status: empty list' { @((Send '{"cmd":"status"}').folders).Count -eq 0 }
    Check 'garbage request: bad_request' { (Send 'hello').error -eq 'bad_request' }

    # Dobavit papku
    Check 'add: bad vault path' { (Send "{`"cmd`":`"add`",`"path`":`"$(J $mnt)`",`"vault`":`"$(J "$root\nothing")`"}").error -eq 'bad_vault' }
    Check 'add: vault inside the folder is refused' { (Send "{`"cmd`":`"add`",`"path`":`"$(J $root)`",`"vault`":`"$(J $vault)`"}").error -eq 'bad_request' }
    New-Item -ItemType Directory "$root\busy" | Out-Null; Set-Content "$root\busy\keep.txt" 'keep'
    Check 'add: plain folder on the path is not touched' {
        $r = Send "{`"cmd`":`"add`",`"path`":`"$(J "$root\busy")`",`"vault`":`"$(J $vault)`"}"
        $r.error -eq 'path_occupied' -and (Get-Content "$root\busy\keep.txt") -eq 'keep'
    }
    Check 'add: mounted locked' {
        $r = Send "{`"cmd`":`"add`",`"path`":`"$(J $mnt)`",`"vault`":`"$(J $vault)`"}"
        $r.ok -and $r.folder.state -eq 'locked' -and $r.folder.name -eq $ruName
    }
    Check 'add again: already_added' { (Send "{`"cmd`":`"add`",`"path`":`"$(J $mnt)`",`"vault`":`"$(J $vault)`"}").error -eq 'already_added' }
    Check 'locked view: only the open item' {
        $names = @(Get-ChildItem $mnt -Name)
        ($names.Count -eq 1 -and $names[0] -eq "$openWord $ruName.zamok") -or ("names: $($names -join ', ')" -eq '')
    }
    Check 'mount point has the system attribute (icon)' { ((Get-Item $mnt -Force).Attributes -band [IO.FileAttributes]::System) -ne 0 }
    Check 'folders.json lists the folder' { (Get-Content "$data\folders.json" -Raw -Encoding UTF8 | ConvertFrom-Json).folders[0].path -eq $mnt }

    # Otkryt i zaperet
    Check 'wrong password: stays locked' { (Send "{`"cmd`":`"unlock`",`"path`":`"$(J $mnt)`",`"password`":`"nope`"}").error -eq 'wrong_password' -and (State) -eq 'locked' }
    Check 'unlock by the open item path' {
        $r = Send "{`"cmd`":`"unlock`",`"path`":`"$(J "$mnt\$openWord $ruName.zamok")`",`"password`":`"$pw`"}"
        $r.ok -and $r.folder.state -eq 'unlocked'
    }
    Check 'unlocked: all files visible' { $n = @(Get-ChildItem $mnt -Recurse -File -Force).Count; $n -eq (SrcCount) -or "files=$n" -eq '' }
    Set-Content "$mnt\new.txt" 'written through core'
    Check 'lock: files hidden again' {
        $r = Send "{`"cmd`":`"lock`",`"path`":`"$(J $mnt)`"}"
        $r.folder.state -eq 'locked' -and -not (Test-Path "$mnt\new.txt")
    }
    Check 'wrong recovery key' { (Send "{`"cmd`":`"unlock_recovery`",`"path`":`"$(J $mnt)`",`"words`":`"one two three`"}").error -eq 'wrong_recovery_key' }
    Check 'unlock with the recovery key: written file is there' {
        $r = Send "{`"cmd`":`"unlock_recovery`",`"path`":`"$(J $mnt)`",`"words`":`"$recovery`"}"
        $r.ok -and (Get-Content "$mnt\new.txt") -eq 'written through core'
    }
    Check 'lock_all' { (Send '{"cmd":"lock_all"}').ok -and (State) -eq 'locked' }

    # Avtozapiranie po soobshcheniyam Windows
    [void](Send "{`"cmd`":`"unlock`",`"path`":`"$(J $mnt)`",`"password`":`"$pw`"}")
    Check 'screen lock (WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK) locks' { PostToCore 0x02B1 7; WaitFor { (State) -eq 'locked' } 5000 }
    [void](Send "{`"cmd`":`"unlock`",`"path`":`"$(J $mnt)`",`"password`":`"$pw`"}")
    Check 'sleep (WM_POWERBROADCAST, PBT_APMSUSPEND) locks' { PostToCore 0x0218 4; WaitFor { (State) -eq 'locked' } 5000 }
    [void](Send "{`"cmd`":`"unlock`",`"path`":`"$(J $mnt)`",`"password`":`"$pw`"}")
    Check 'user switch (WTS_CONSOLE_DISCONNECT) locks' { PostToCore 0x02B1 2; WaitFor { (State) -eq 'locked' } 5000 }
    Check 'lock_all during the password check: folder ends locked, unlock cancelled' {
        # Zapros otkrytiya ushel, scrypt schitaet; v eto vremya prihodit lock_all.
        # Pri lyubom raskladke papka v konce zaperta; otmenu dostatochno poymat hotya by raz.
        $cancelled = 0
        foreach ($delay in 0, 10, 20, 30, 40) {
            $c = New-Object IO.Pipes.NamedPipeClientStream '.', $pipeName, 'InOut'
            $c.Connect(5000)
            $enc = New-Object Text.UTF8Encoding $false
            $w = New-Object IO.StreamWriter $c, $enc; $w.AutoFlush = $true
            $r = New-Object IO.StreamReader $c, $enc
            $w.WriteLine("{`"cmd`":`"unlock`",`"path`":`"$(J $mnt)`",`"password`":`"$pw`"}")
            Start-Sleep -Milliseconds $delay
            [void](Send '{"cmd":"lock_all"}')
            $resp = $r.ReadLine() | ConvertFrom-Json
            $c.Dispose()
            $st = State
            if ($st -ne 'locked') { throw "delay=$delay state=$st resp=$($resp | ConvertTo-Json -Compress)" }
            if ($resp.error -eq 'unlock_cancelled') { $cancelled++ }
        }
        if ($cancelled -eq 0) { throw 'never caught the unlock in progress' }
        "cancelled $cancelled of 5"
    }
    [void](Send "{`"cmd`":`"unlock`",`"path`":`"$(J $mnt)`",`"password`":`"$pw`"}")
    Check 'session unlock message does not lock' { PostToCore 0x02B1 8; Start-Sleep 1; (State) -eq 'unlocked' }

    # Avtozapiranie po bezdeystviyu
    Check 'set_idle_lock 5 s' { (Send "{`"cmd`":`"set_idle_lock`",`"path`":`"$(J $mnt)`",`"secs`":5}").folder.idle_lock_secs -eq 5 }
    [void](Send "{`"cmd`":`"lock`",`"path`":`"$(J $mnt)`"}")
    [void](Send "{`"cmd`":`"unlock`",`"path`":`"$(J $mnt)`",`"password`":`"$pw`"}")
    Check 'reading the folder keeps it open' {
        1..9 | ForEach-Object { [void](Get-Content "$mnt\new.txt"); Start-Sleep -Milliseconds 1000 }
        (State) -eq 'unlocked'
    }
    Check 'idle folder locks by itself' { WaitFor { (State) -eq 'locked' } 12000 }
    # Otkrytyy v programme fayl: po bezdeystviyu ne zapiraem, inache dokument ne sohranit
    [void](Send "{`"cmd`":`"unlock`",`"path`":`"$(J $mnt)`",`"password`":`"$pw`"}")
    $held = [IO.File]::Open("$mnt\new.txt", 'Open', 'Read', 'ReadWrite')
    Check 'file held open: idle lock waits' { Start-Sleep 10; (State) -eq 'unlocked' }
    $held.Dispose()
    Check 'file closed: idle lock happens' { WaitFor { (State) -eq 'locked' } 12000 }
    [void](Send "{`"cmd`":`"set_idle_lock`",`"path`":`"$(J $mnt)`",`"secs`":0}")

    # Perezapusk: spisok sohranyaetsya, papka podklyuchaetsya zapertoy
    Check 'quit: process exits, mount path gone' {
        [void](Send '{"cmd":"quit"}')
        $script:proc.WaitForExit(15000) -and -not (Test-Path $mnt)
    }
    $script:proc = StartCore
    Check 'restart: folder mounted locked from folders.json' { (State) -eq 'locked' -and (Test-Path "$mnt\$openWord $ruName.zamok") }
    Check 'restart: idle setting kept' { ((Send '{"cmd":"status"}').folders[0].idle_lock_secs) -eq 0 }

    # Padenie processa: WinFsp sam ubiraet tochku podklyucheniya
    [void](Send "{`"cmd`":`"unlock`",`"path`":`"$(J $mnt)`",`"password`":`"$pw`"}")
    Check 'core killed: mount point removed, nothing decrypted left' {
        $script:proc.Kill(); [void]$script:proc.WaitForExit(10000)
        WaitFor { -not (Test-Path $mnt) } 10000
    }
    $script:proc = StartCore
    Check 'after crash: mounted again, locked' { (State) -eq 'locked' }

    # Mertvaya tochka podklyucheniya posle vyklyucheniya pitaniya
    [void](Send '{"cmd":"quit"}'); [void]$script:proc.WaitForExit(15000)
    New-Item -ItemType Directory $mnt | Out-Null
    [ZT]::SetMountPoint($mnt, '\Device\Volume{00000000-0000-0000-0000-00000000dead}\')
    Check 'stale mount point is in place' { try { [void](Get-ChildItem $mnt); $false } catch { $true } }
    $script:proc = StartCore
    Check 'start removes the stale mount point and mounts' { (State) -eq 'locked' -and (Test-Path "$mnt\$openWord $ruName.zamok") }

    # Na meste papki obychnaya papka: core ee ne trogaet
    [void](Send '{"cmd":"quit"}'); [void]$script:proc.WaitForExit(15000)
    New-Item -ItemType Directory $mnt | Out-Null; Set-Content "$mnt\mine.txt" 'mine'
    $script:proc = StartCore
    Check 'occupied path at start: unavailable, user file untouched' {
        $f = (Send '{"cmd":"status"}').folders[0]
        $f.state -eq 'unavailable' -and $f.error -eq 'path_occupied' -and (Get-Content "$mnt\mine.txt") -eq 'mine'
    }
    Remove-Item $mnt -Recurse -Force
    Check 'unlock mounts a folder that was unavailable' {
        $r = Send "{`"cmd`":`"unlock`",`"path`":`"$(J $mnt)`",`"password`":`"$pw`"}"
        $r.ok -and (Get-Content "$mnt\new.txt") -eq 'written through core'
    }

    # Ubrat iz spiska: dannye cely
    Check 'remove: unmounted, vault intact, list empty' {
        $r = Send "{`"cmd`":`"remove`",`"path`":`"$(J $mnt)`"}"
        $r.ok -and -not (Test-Path $mnt) -and (Test-Path "$vault\masterkey.cryptomator") -and
            @((Get-Content "$data\folders.json" -Raw -Encoding UTF8 | ConvertFrom-Json).folders).Count -eq 0
    }
    Check 'unknown folder: not_found' { (Send "{`"cmd`":`"lock`",`"path`":`"$(J $mnt)`"}").error -eq 'not_found' }

    # Chelovek udalil papku v Provodnike (DESIGN.md, razdel 5.1)
    [void](Send "{`"cmd`":`"add`",`"path`":`"$(J $mnt)`",`"vault`":`"$(J $vault)`"}")
    Check 'mount point deleted, vault intact: folder comes back locked' {
        cmd /c rmdir "$mnt" | Out-Null
        (WaitFor { Test-Path "$mnt\$openWord $ruName.zamok" } 10000) -and (State) -eq 'locked' -and (Test-Path "$vault\masterkey.cryptomator")
    }
    Check 'folder renamed in Explorer: core keeps it at the new path' {
        $new = "$root\renamed"
        # Tom viden chut ranshe, chem core zapomnil nomer tochki podklyucheniya
        Start-Sleep -Seconds 1
        Move-Item -LiteralPath $mnt -Destination $new
        $ok = (WaitFor { @((Send '{"cmd":"status"}').folders | Where-Object { $_.path -eq $new }).Count -eq 1 } 10000) -and
            (Test-Path "$new\$openWord $ruName.zamok") -and -not (Test-Path $mnt) -and
            ((Get-Content "$data\folders.json" -Raw -Encoding UTF8 | ConvertFrom-Json).folders[0].path -eq $new)
        Move-Item -LiteralPath $new -Destination $mnt
        $ok -and (WaitFor { (State) -eq 'locked' } 10000)
    }
    # Perenos v druguyu papku togo zhe diska: hranilishche idet sledom, kogda papka zaperta
    $other = "$root\other"; New-Item -ItemType Directory -Force $other | Out-Null
    $mntB = "$other\$ruName"; $vaultB = "$other\.$ruName.zamok"
    function VaultIs([string]$v, [string]$old) { WaitFor { (Test-Path -LiteralPath "$v\masterkey.cryptomator") -and -not (Test-Path -LiteralPath $old) } 12000 }
    function FirstState { @((Send '{"cmd":"status"}').folders)[0].state }
    Check 'locked folder moved to another folder: the vault follows, list updated' {
        Move-Item -LiteralPath $mnt -Destination $mntB
        (VaultIs $vaultB $vault) -and (WaitFor { (FirstState) -eq 'locked' } 10000) -and
            ((Get-Content "$data\folders.json" -Raw -Encoding UTF8 | ConvertFrom-Json).folders[0].path -eq $mntB)
    }
    Check 'moved folder opens with the password' {
        $r = Send "{`"cmd`":`"unlock`",`"path`":`"$(J $mntB)`",`"password`":`"$pw`"}"
        $r.ok -and (Get-Content -LiteralPath "$mntB\new.txt") -eq 'written through core'
    }
    Check 'open folder moved: stays open, the vault waits for the lock' {
        Move-Item -LiteralPath $mntB -Destination $mnt
        $seen = WaitFor { @((Send '{"cmd":"status"}').folders | Where-Object { $_.path -eq $mnt }).Count -eq 1 } 10000
        Start-Sleep -Seconds 3
        $seen -and (Test-Path -LiteralPath "$vaultB\masterkey.cryptomator") -and (Get-Content -LiteralPath "$mnt\new.txt") -eq 'written through core'
    }
    Check 'after the lock the vault follows' {
        [void](Send "{`"cmd`":`"lock`",`"path`":`"$(J $mnt)`"}")
        (VaultIs $vault $vaultB) -and (WaitFor { (State) -eq 'locked' } 10000)
    }
    Check 'core killed in the middle of a vault move: found after restart, move finished' {
        [void](Send '{"cmd":"quit"}'); [void]$script:proc.WaitForExit(15000)
        $script:proc = StartCore 'vault_move'
        Move-Item -LiteralPath $mnt -Destination $mntB
        [void]$script:proc.WaitForExit(15000)
        $stillOld = Test-Path -LiteralPath "$vault\masterkey.cryptomator"
        $script:proc = StartCore
        $stillOld -and (VaultIs $vaultB $vault) -and (WaitFor { (FirstState) -eq 'locked' } 10000)
    }
    Check 'moved back: the vault returns too' {
        Move-Item -LiteralPath $mntB -Destination $mnt
        (VaultIs $vault $vaultB) -and (WaitFor { (State) -eq 'locked' } 10000)
    }
    Check 'parent renamed while core was off, old name taken again: entry follows the vault' {
        Move-Item -LiteralPath $mnt -Destination $mntB
        $moved = (VaultIs $vaultB $vault) -and (WaitFor { (FirstState) -eq 'locked' } 10000)
        Start-Sleep -Seconds 3
        [void](Send '{"cmd":"quit"}'); [void]$script:proc.WaitForExit(15000)
        $renamed = "$root\other-renamed"
        Rename-Item -LiteralPath $other -NewName 'other-renamed'
        New-Item -ItemType Directory -Force $other | Out-Null
        $script:proc = StartCore
        $folder = "$renamed\$ruName"
        $ok = (WaitFor { @((Send '{"cmd":"status"}').folders | Where-Object { $_.path -eq $folder -and $_.state -eq 'locked' }).Count -eq 1 } 15000) -and
            ((Get-Content "$data\folders.json" -Raw -Encoding UTF8 | ConvertFrom-Json).folders[0].vault -eq "$renamed\.$ruName.zamok")
        [void](Send '{"cmd":"quit"}'); [void]$script:proc.WaitForExit(15000)
        Remove-Item -LiteralPath $other -Force
        Rename-Item -LiteralPath $renamed -NewName 'other'
        $script:proc = StartCore
        $back = WaitFor { @((Send '{"cmd":"status"}').folders | Where-Object { $_.path -eq $mntB -and $_.state -eq 'locked' }).Count -eq 1 } 15000
        Move-Item -LiteralPath $mntB -Destination $mnt
        $moved -and $ok -and $back -and (VaultIs $vault $vaultB) -and (WaitFor { (State) -eq 'locked' } 10000)
    }
    # Vyrezat v Provodnike: papka v bufere obmena s pometkoy "perenos" (2) ili "kopirovanie" (5)
    function PutOnClipboard([string]$path, [int]$effect) {
        $env:ZC_PATH = $path; $env:ZC_EFFECT = "$effect"; $env:ZC_FORMAT = 'Preferred DropEffect'
        & powershell -NoProfile -Sta -Command 'Add-Type -AssemblyName System.Windows.Forms; $d = New-Object Windows.Forms.DataObject; $c = New-Object Collections.Specialized.StringCollection; [void]$c.Add($env:ZC_PATH); $d.SetFileDropList($c); $m = New-Object IO.MemoryStream(,[BitConverter]::GetBytes([int]$env:ZC_EFFECT)); $d.SetData($env:ZC_FORMAT, $m); [Windows.Forms.Clipboard]::SetDataObject($d, $true, 20, 200)'
    }
    function ClipHasFiles { (& powershell -NoProfile -Sta -Command 'Add-Type -AssemblyName System.Windows.Forms; [Windows.Forms.Clipboard]::ContainsFileDropList()') -eq 'True' }
    $savedClip = Get-Clipboard -Raw -ErrorAction SilentlyContinue
    # Snachala kopirovanie: esli pomoshchnik slomaetsya, eta proverka upadet, a ne proydet vpustuyu
    Check 'copy of a zamok folder stays on the clipboard' { PutOnClipboard $mnt 5; Start-Sleep -Seconds 2; ClipHasFiles }
    Check 'cut of a zamok folder is cancelled, the clipboard is emptied' { PutOnClipboard $mnt 2; WaitFor { -not (ClipHasFiles) } 8000 }
    Check 'cut of another folder stays on the clipboard' { PutOnClipboard $data 2; Start-Sleep -Seconds 2; ClipHasFiles }
    if ($savedClip) { Set-Clipboard -Value $savedClip } else { & powershell -NoProfile -Sta -Command 'Add-Type -AssemblyName System.Windows.Forms; [Windows.Forms.Clipboard]::Clear()' }
    # Nedavnie fayly: posle zapiraniya uhodyat tolko yarlyki v etu papku
    function Lnk([string]$name, [string]$target) { $s = (New-Object -ComObject WScript.Shell).CreateShortcut("$root\recent\$name.lnk"); $s.TargetPath = $target; $s.Save() }
    Check 'after the lock recent shortcuts into the folder are gone, others stay' {
        New-Item -ItemType Directory -Force "$root\recent" | Out-Null
        $u = Send "{`"cmd`":`"unlock`",`"path`":`"$(J $mnt)`",`"password`":`"$pw`"}"
        Lnk 'inside' "$mnt\new.txt"; Lnk 'folder' $mnt; Lnk 'other' "$data\x.txt"; Lnk 'similar' "$mnt-2\a.txt"
        Start-Sleep -Seconds 3
        [void](Send "{`"cmd`":`"lock`",`"path`":`"$(J $mnt)`"}")
        $u.ok -and (WaitFor { -not (Test-Path "$root\recent\inside.lnk") -and -not (Test-Path "$root\recent\folder.lnk") } 8000) -and
            (Test-Path "$root\recent\other.lnk") -and (Test-Path "$root\recent\similar.lnk")
    }
    Check 'hotkey locks everything' {
        $u = Send "{`"cmd`":`"unlock`",`"path`":`"$(J $mnt)`",`"password`":`"$pw`"}"
        PostToCore 0x0312 1
        $u.ok -and (WaitFor { (State) -eq 'locked' } 8000)
    }
    Check 'vault deleted: folder leaves the list, the name is free again' {
        Remove-Item $vault -Recurse -Force
        (WaitFor { -not (State) } 10000) -and (WaitFor { -not (Test-Path $mnt) } 5000) -and
            @((Get-Content "$data\folders.json" -Raw -Encoding UTF8 | ConvertFrom-Json).folders).Count -eq 0
    }
    Check 'log has no folder names, paths or passwords' {
        $log = Get-Content "$data\core.log" -Raw -Encoding UTF8
        $bad = @($ruName, $root, $pw, 'busy') | Where-Object { $log.Contains($_) }
        if ($bad) { "found in log: $($bad -join ', ')" -eq '' } else { "lines=$(($log -split "`n").Count)" }
    }

    # Storozh (--watch): podnimaet core posle padeniya, uvazhaet obychnyy vyhod
    [void](Send '{"cmd":"quit"}'); [void]$script:proc.WaitForExit(15000)
    $psi = New-Object Diagnostics.ProcessStartInfo $coreExe
    $psi.UseShellExecute = $false
    $psi.Arguments = '--watch'
    $psi.EnvironmentVariables['ZAMOK_DATA_DIR'] = $data
    $psi.EnvironmentVariables['ZAMOK_PIPE'] = "\\.\pipe\$pipeName"
    $psi.EnvironmentVariables['ZAMOK_NO_TRAY'] = '1'
    $psi.EnvironmentVariables['ZAMOK_RECENT_DIR'] = "$root\recent"
    $watch = [Diagnostics.Process]::Start($psi)
    function Child { Get-CimInstance Win32_Process -Filter "ParentProcessId=$($watch.Id)" | Where-Object Name -eq 'zamok-core.exe' | Select-Object -First 1 }
    [void](WaitFor { Test-Path "\\.\pipe\$pipeName" } 15000)
    Check 'watchdog: core restarted after a crash' {
        $first = Child
        Stop-Process -Id $first.ProcessId -Force
        (WaitFor { $c = Child; $c -and $c.ProcessId -ne $first.ProcessId -and (Test-Path "\\.\pipe\$pipeName") } 15000) -and (Send '{"cmd":"status"}').ok
    }
    Check 'watchdog: quit stops core and the watchdog' {
        [void](Send '{"cmd":"quit"}')
        $watch.WaitForExit(20000)
    }
    if (-not $watch.HasExited) { $watch.Kill() }
}
finally {
    if ($script:proc -and -not $script:proc.HasExited) {
        try { [void](Send '{"cmd":"quit"}') } catch {}
        if (-not $script:proc.WaitForExit(15000)) { $script:proc.Kill() }
    }
    $results | Format-Table -AutoSize -Wrap | Out-String -Width 220
    "passed: $(@($results | Where-Object Ok).Count) / $($results.Count)"
}

