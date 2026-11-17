# Papki na fleshkah exFAT i FAT32: podklyuchenie s bukvoy diska (DESIGN.md, razdel 3.6).
# Bez parametrov: vse papki vnutri tests\tmp\letter na NTFS, core zapuskaetsya s
# ZAMOK_FORCE_LETTER_MODE=1 i schitaet lyuboy tom fleshkoy exFAT (na mashine razrabotki exFAT net).
# S -Drive G: tot zhe prohod na nastoyashchem diske exFAT: vse dannye tolko v G:\zamok-test,
# v konce papka udalyaetsya. -Big dobavlyaet shifrovanie fayla bolshe 4 GiB (dolgo: pishet 4,5 GB).
# Core vsegda so svoey papkoy dannyh i svoim kanalom, nastoyashchiy Zamok ne zatragivaetsya.
# Uhod fleshki i smena bukvy proveryayutsya cherez subst (bukva diska bez prav administratora).
# Zapusk: powershell -NoProfile -ExecutionPolicy Bypass -File tests\core\test-letter.ps1 [-Drive G:] [-Big]
# Skript v ASCII: russkie imena sobirayutsya iz kodov simvolov.
param([string]$Drive = '', [switch]$Big)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path "$PSScriptRoot/../..").Path
$force = -not $Drive
$root = if ($force) { "$repo\tests\tmp\letter" } else { "$($Drive.TrimEnd('\'))\zamok-test" }
$data = "$repo\tests\tmp\letter-data"
$data2 = "$repo\tests\tmp\letter-data2"
$coreExe = "$repo\core\target\release\zamok-core.exe"
$pw = 'zamok-letter-test'
$pipeName = "zamok-letter-test-$PID"
$pipe2 = "zamok-letter-test2-$PID"
$results = New-Object System.Collections.Generic.List[object]
function Ru([int[]]$c) { -join ($c | ForEach-Object { [char]$_ }) }
$ruName = Ru 0x0424,0x043E,0x0442,0x043E                         # Foto
$ruPapka = Ru 0x041F,0x0430,0x043F,0x043A,0x0430                 # Papka
$ruPrivet = Ru 0x043F,0x0440,0x0438,0x0432,0x0435,0x0442         # privet
$ruOpen = Ru 0x041E,0x0442,0x043A,0x0440,0x044B,0x0442,0x044C    # Otkryt
function Check([string]$name, [scriptblock]$body) {
    try { $r = & $body; if ($r -eq $false) { throw 'returned false' }
        $results.Add([pscustomobject]@{ Test = $name; Ok = $true; Note = "$r" }) }
    catch { $results.Add([pscustomobject]@{ Test = $name; Ok = $false; Note = $_.Exception.Message }) }
}
function WaitFor([scriptblock]$cond, [int]$ms = 10000) {
    $deadline = (Get-Date).AddMilliseconds($ms)
    while (-not $(try { & $cond } catch { $false })) { if ((Get-Date) -gt $deadline) { return $false }; Start-Sleep -Milliseconds 100 }
    $true
}
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class ZL {
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern IntPtr FindWindowExW(IntPtr parent, IntPtr after, string cls, string title);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint msg, IntPtr w, IntPtr l);
  [DllImport("kernel32.dll")] public static extern uint GetLogicalDrives();
  public static IntPtr CoreWindow(int pid) {
    IntPtr h = IntPtr.Zero;
    while ((h = FindWindowExW(IntPtr.Zero, h, "ZamokCoreWindow", null)) != IntPtr.Zero) {
      uint p; GetWindowThreadProcessId(h, out p); if (p == pid) return h;
    }
    return IntPtr.Zero;
  }
}
'@
function StartCore([string]$crash = '', [string]$dir = $data, [string]$pipe = $pipeName, [string]$mode = '1') {
    $psi = New-Object Diagnostics.ProcessStartInfo $coreExe
    $psi.UseShellExecute = $false
    $psi.EnvironmentVariables['ZAMOK_DATA_DIR'] = $dir
    $psi.EnvironmentVariables['ZAMOK_PIPE'] = "\\.\pipe\$pipe"
    $psi.EnvironmentVariables['ZAMOK_NO_TRAY'] = '1'
    $psi.EnvironmentVariables['ZAMOK_SEARCH_DRY_RUN'] = '1'
    if ($force -or $mode -ne '1') { $psi.EnvironmentVariables['ZAMOK_FORCE_LETTER_MODE'] = $mode }
    if ($crash) { $psi.EnvironmentVariables['ZAMOK_TEST_CRASH'] = $crash }
    $p = [Diagnostics.Process]::Start($psi)
    [void](WaitFor { (Test-Path "\\.\pipe\$pipe") -or $p.HasExited } 60000)
    if (-not $p.HasExited) { try { [void](SendTo $pipe '{"cmd":"status"}') } catch {} }
    $p
}
function SendTo([string]$pipe, [string]$json) {
    $c = New-Object IO.Pipes.NamedPipeClientStream '.', $pipe, 'InOut'
    $c.Connect(5000)
    $enc = New-Object Text.UTF8Encoding $false
    $w = New-Object IO.StreamWriter $c, $enc; $w.AutoFlush = $true
    $r = New-Object IO.StreamReader $c, $enc
    $w.WriteLine($json)
    $line = $r.ReadLine()
    $c.Dispose()
    $line | ConvertFrom-Json
}
function Send([string]$json) { SendTo $pipeName $json }
function J([string]$s) { $s.Replace('\', '\\').Replace('"', '\"') }
function Folder([string]$path, [string]$pipe = $pipeName) { (SendTo $pipe '{"cmd":"status"}').folders | Where-Object { $_.path -eq $path } }
function EncryptReq([string]$path, [string]$p = $pw) { "{`"cmd`":`"encrypt`",`"path`":`"$(J $path)`",`"password`":`"$p`"}" }
function Unlock([string]$path, [string]$p = $pw) { Send "{`"cmd`":`"unlock`",`"path`":`"$(J $path)`",`"password`":`"$p`"}" }
function Lock([string]$path) { Send "{`"cmd`":`"lock`",`"path`":`"$(J $path)`"}" }
function WaitJob([string]$id, [int]$ms = 600000) {
    $deadline = (Get-Date).AddMilliseconds($ms)
    while ($true) {
        $j = (Send "{`"cmd`":`"job_status`",`"id`":`"$id`"}").job
        if ($j.state -ne 'running') { return $j }
        if ((Get-Date) -gt $deadline) { throw "job still running: $($j | ConvertTo-Json -Compress)" }
        Start-Sleep -Milliseconds 200
    }
}
function EncryptWait([string]$path) { $r = Send (EncryptReq $path); if ($r.ok) { WaitJob $r.job.id } else { $r } }
function PostToCore([uint32]$msg, [int]$w) { [void][ZL]::PostMessageW([ZL]::CoreWindow($script:proc.Id), $msg, [IntPtr]$w, [IntPtr]::Zero) }
function MakeSample([string]$dir, [int]$many = 20) {
    New-Item -ItemType Directory -Force "$dir\$ruPapka\vlozh", "$dir\pustaya", "$dir\many" | Out-Null
    [IO.File]::WriteAllText("$dir\$ruPrivet.txt", $ruPrivet, (New-Object Text.UTF8Encoding $false))
    [IO.File]::WriteAllBytes("$dir\empty.bin", [byte[]]@())
    $rnd = New-Object Random 42; $b = New-Object byte[] (3MB); $rnd.NextBytes($b)
    [IO.File]::WriteAllBytes("$dir\$ruPapka\big.bin", $b)
    [IO.File]::WriteAllText("$dir\$ruPapka\vlozh\deep.txt", 'deep')
    1..$many | ForEach-Object { [IO.File]::WriteAllText("$dir\many\f$_.txt", "file $_") }
}
function Manifest([string]$dir) {
    $base = (Get-Item -LiteralPath $dir -Force).FullName.TrimEnd('\')
    $sha = [Security.Cryptography.SHA256]::Create()
    @(Get-ChildItem -LiteralPath $dir -Recurse -Force | ForEach-Object {
        $rel = $_.FullName.Substring($base.Length + 1).ToLower()
        if ($_.PSIsContainer) { "D $rel" } else {
            "F $rel " + [BitConverter]::ToString($sha.ComputeHash([IO.File]::ReadAllBytes($_.FullName)))
        }
    } | Sort-Object) -join "`n"
}
function Sib([string]$path) {
    $parent = Split-Path $path -Parent; $name = Split-Path $path -Leaf
    @{ Tmp = "$parent\.$name.zamok.tmp"; Vault = "$parent\.$name.zamok"; Aside = "$parent\.$name.zamok-original" }
}
function Journals([string]$dir = $data) { @(Get-ChildItem "$dir\jobs" -Filter *.json -ErrorAction SilentlyContinue).Count }
function DriveOf([string]$path, [string]$pipe = $pipeName) { (Folder $path $pipe).drive }
function DriveUp([string]$d) { $d -and (Test-Path "$d\") }
# Na puti papki obychnaya papka: tolko fayl otkrytiya i skrytyy desktop.ini so znachkom
function IsPlaceholder([string]$path) {
    $i = Get-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue
    if (-not $i -or -not $i.PSIsContainer -or ($i.Attributes -band [IO.FileAttributes]::ReparsePoint)) { return $false }
    $name = Split-Path $path -Leaf
    $items = @(Get-ChildItem -LiteralPath $path -Force | ForEach-Object Name | Sort-Object)
    $want = @('desktop.ini', "$ruOpen $name.zamok") | Sort-Object
    $ini = Get-Item -LiteralPath "$path\desktop.ini" -Force -ErrorAction SilentlyContinue
    ($items -join '|') -eq ($want -join '|') -and ($i.Attributes -band [IO.FileAttributes]::System) -and
        ($ini.Attributes -band [IO.FileAttributes]::Hidden) -and ($ini.Attributes -band [IO.FileAttributes]::System) -and
        ([IO.File]::ReadAllText("$path\desktop.ini") -like '*IconResource=*')
}
function FreeLetters {
    $used = [ZL]::GetLogicalDrives()
    @(80..87 | Where-Object { ($used -band (1 -shl ($_ - 65))) -eq 0 } | ForEach-Object { [string][char]$_ })
}
$javaDir = 'C:\dev\tools\jdk-26.0.2.1+1\bin'; $toolDir = "$repo\tests\fixtures-tool"
$haveCryptomator = (Test-Path "$javaDir\java.exe") -and (Test-Path "$toolDir\cp.txt")
function CryptomatorFiles([string]$vault, [string]$out) {
    $cp = (Get-Content "$toolDir\cp.txt").Substring(4).Trim([char]34)
    & "$javaDir\java.exe" "-Dfile.encoding=UTF-8" "-Dsun.jnu.encoding=UTF-8" -cp "$toolDir/out;$cp" ReadVault $vault $out $pw 2>&1 | Out-Null
    (Manifest $out) -split "`n" | Where-Object { $_ -like 'F *' }
}
function FilesOf([string]$manifest) { $manifest -split "`n" | Where-Object { $_ -like 'F *' } }

# Podgotovka
if (-not $force) {
    $fs = (Get-Volume -DriveLetter $Drive.Substring(0, 1)).FileSystem
    if ($fs -notin 'exFAT', 'FAT32') { throw "$Drive is $fs, not exFAT or FAT32" }
}
Get-Process zamok-core -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $coreExe } | Stop-Process -Force
Start-Sleep -Milliseconds 500
Remove-Item $root, $data, $data2 -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $root | Out-Null
Push-Location "$repo\core"
& "$env:USERPROFILE\.cargo\bin\cargo.exe" build -q --release -p zamok-core 2>&1 | Out-Null
Pop-Location
MakeSample "$root\ref"
$ref = Manifest "$root\ref"
$substs = New-Object System.Collections.Generic.List[string]

$script:proc = $null
try {
    $script:proc = StartCore

    # A. Shifrovanie na meste: papka s faylom otkrytiya, otkrytaya papka na bukve diska
    $a = "$root\a\$ruName"; MakeSample $a; $sa = Sib $a
    $resp = Send (EncryptReq $a)
    Check 'encrypt on this volume is accepted (preflight), recovery key has 44 words' {
        $resp.ok -and @($resp.recovery_key -split ' ').Count -eq 44 -or $(throw "resp=$($resp | ConvertTo-Json -Compress)")
    }
    $job = WaitJob $resp.job.id
    Check 'encrypt: job done' { $job.state -eq 'done' -or $(throw "job=$($job | ConvertTo-Json -Compress)") }
    $fa = Folder $a; $da = $fa.drive
    Check 'after encryption: letter mode, open at a drive letter' { $fa.letter -and $fa.state -eq 'unlocked' -and $da -match '^[D-Z]:$' -and (DriveUp $da) }
    Check 'drive content is identical to the original' { (Manifest "$da\") -eq $ref }
    Check 'volume label is the folder name' { ([IO.DriveInfo]::new($da)).VolumeLabel -eq $ruName }
    Check 'folder path: plain folder with the opener file and desktop.ini (lock icon)' { IsPlaceholder $a }
    Check 'opener file names the hidden folder' { [IO.File]::ReadAllText("$a\$ruOpen $ruName.zamok") -like "*.$ruName.zamok*" }
    Check 'vault next to it is hidden and system, original and tmp are gone' {
        $at = (Get-Item -LiteralPath $sa.Vault -Force).Attributes
        ($at -band [IO.FileAttributes]::Hidden) -and ($at -band [IO.FileAttributes]::System) -and
            -not (Test-Path $sa.Aside) -and -not (Test-Path $sa.Tmp) -and (Journals) -eq 0
    }

    # B. Chtenie i zapis, zapiranie, otkrytie
    [IO.File]::WriteAllText("$da\new.txt", 'written at the drive letter')
    [IO.File]::AppendAllText("$da\$ruPapka\vlozh\deep.txt", ' and more')
    Check 'read back what was written at the drive' { [IO.File]::ReadAllText("$da\new.txt") -eq 'written at the drive letter' }
    $refA = Manifest "$da\"
    $r = Lock $a
    Check 'lock: drive letter is gone, folder locked, opener still there' {
        $r.ok -and $r.folder.state -eq 'locked' -and -not $r.folder.drive -and -not (DriveUp $da) -and (IsPlaceholder $a)
    }
    $r = Unlock $a 'wrong'
    Check 'wrong password: refused, no drive letter appears' { $r.error -eq 'wrong_password' -and -not (DriveUp $da) -and (Folder $a).state -eq 'locked' }
    $r = Unlock "$a\$ruOpen $ruName.zamok"
    $da = $r.folder.drive
    Check 'unlock by the opener file path: open again, writes kept' { $r.ok -and (DriveUp $da) -and (Manifest "$da\") -eq $refA }
    PostToCore 0x02B1 7
    Check 'screen lock (WTS_SESSION_LOCK) locks and unmounts the drive' { WaitFor { (Folder $a).state -eq 'locked' -and -not (DriveUp $da) } 5000 }
    $r = Unlock $a; $da = $r.folder.drive
    [void](Send "{`"cmd`":`"set_idle_lock`",`"path`":`"$(J $a)`",`"secs`":3}")
    Check 'idle lock locks and unmounts the drive' { WaitFor { (Folder $a).state -eq 'locked' -and -not (DriveUp $da) } 15000 }
    [void](Send "{`"cmd`":`"set_idle_lock`",`"path`":`"$(J $a)`",`"secs`":1800}")
    $r = Unlock $a; $da = $r.folder.drive
    PostToCore 0x0219 0x8004
    Check 'device message while the drive is present: folder stays open' { Start-Sleep -Milliseconds 1500; (Folder $a).state -eq 'unlocked' -and (DriveUp $da) }
    [void](Lock $a)
    if ($haveCryptomator) {
        Check 'Cryptomator reads the vault: same content' {
            ((CryptomatorFiles $sa.Vault "$root\a-cryptomator") -join "`n") -eq ((FilesOf $refA) -join "`n")
        }
    }

    # C. Fleshku vynuli i vstavili pod drugoy bukvoy (subst vmesto fleshki)
    $letters = FreeLetters
    $s1 = $letters[0] + ':'; $s2 = $letters[1] + ':'
    New-Item -ItemType Directory -Force "$root\stick" | Out-Null
    & subst $s1 "$root\stick"; $substs.Add($s1)
    $c = "$s1\$ruName"; MakeSample $c
    $jc = EncryptWait $c
    $fc = Folder $c; $dc = $fc.drive
    Check 'stick: encrypted and open' {
        $jc.state -eq 'done' -and $fc.state -eq 'unlocked' -and (Manifest "$dc\") -eq $ref -or $(throw "job=$($jc | ConvertTo-Json -Compress) folder=$($fc | ConvertTo-Json -Compress)")
    }
    & subst $s1 /d; [void]$substs.Remove($s1)
    PostToCore 0x0219 0x8004
    Check 'stick removed (DBT_DEVICEREMOVECOMPLETE): locked at once, drive letter gone, state disconnected' {
        WaitFor { (Folder $c).state -eq 'disconnected' -and -not (DriveUp $dc) } 3000
    }
    Start-Sleep -Seconds 8
    Check 'stick absent for 8 s: still in the list, not treated as deleted' { (Folder $c).state -eq 'disconnected' }
    Check 'unlock while the stick is absent: not_connected' { (Unlock $c).error -eq 'not_connected' }
    & subst $s2 "$root\stick"; $substs.Add($s2)
    $c2 = "$s2\$ruName"
    PostToCore 0x0219 0x8000
    Check 'stick back under another letter (DBT_DEVICEARRIVAL): found by volume serial, path updated, locked' {
        WaitFor { (Folder $c2).state -eq 'locked' } 8000
    }
    $r = Unlock $c2; $dc = $r.folder.drive
    Check 'open after the letter change: same content' { $r.ok -and (Manifest "$dc\") -eq $ref }
    & subst $s2 /d; [void]$substs.Remove($s2)
    Check 'stick removed without a device message: the 2 s check locks it (fallback)' {
        WaitFor { (Folder $c2).state -eq 'disconnected' -and -not (DriveUp $dc) } 6000
    }
    [void](Send "{`"cmd`":`"remove`",`"path`":`"$(J $c2)`"}")
    [void](Send '{"cmd":"quit"}'); [void]$script:proc.WaitForExit(20000)

    # D. Avariya na kazhdom shage, potom perezapusk
    $points = @(
        @{ P = 'copying'; Back = $true }, @{ P = 'copy:3'; Back = $true }, @{ P = 'copy_written:3'; Back = $true },
        @{ P = 'flush_done'; Back = $true }, @{ P = 'verified_before_journal'; Back = $true },
        @{ P = 'verified'; Back = $false }, @{ P = 'vault_placed_before_journal'; Back = $false }, @{ P = 'vault_placed'; Back = $false },
        @{ P = 'original_aside_before_journal'; Back = $false }, @{ P = 'original_aside'; Back = $false }, @{ P = 'aside_checked'; Back = $false },
        @{ P = 'registered_before_journal'; Back = $false }, @{ P = 'registered'; Back = $false }, @{ P = 'aside_deleted'; Back = $false }
    )
    $i = 0
    foreach ($pt in $points) {
        $i++
        $e = "$root\e$i\$ruName"; MakeSample $e; $se = Sib $e
        $script:proc = StartCore $pt.P
        try { [void](Send (EncryptReq $e)) } catch {}
        $died = $script:proc.WaitForExit(120000)
        $code = if ($died) { $script:proc.ExitCode } else { $script:proc.Kill(); 'alive' }
        $script:proc = StartCore
        if ($pt.Back) {
            Check "crash at $($pt.P) (exit $code): original untouched, copy waits to be continued" {
                -not (IsPlaceholder $e) -and (Manifest $e) -eq $ref -and (Test-Path $se.Tmp) -and -not (Test-Path $se.Vault) -and
                    -not (Test-Path $se.Aside) -and (Journals) -eq 1 -and -not (Folder $e)
            }
            $je = EncryptWait $e
            Check "crash at $($pt.P), encrypt again: continued, same content at the drive, nothing left" {
                $f = Folder $e
                $je.state -eq 'done' -and $f.state -eq 'unlocked' -and (Manifest "$($f.drive)\") -eq $ref -and (IsPlaceholder $e) -and
                    -not (Test-Path $se.Tmp) -and -not (Test-Path $se.Aside) -and (Journals) -eq 0 -or $(throw "job=$($je | ConvertTo-Json -Compress)")
            }
        } else {
            Check "crash at $($pt.P) (exit $code): encrypted, opener in place, same content, nothing left" {
                $f = Folder $e
                $u = Unlock $e
                $f.letter -and $u.ok -and (Manifest "$($u.folder.drive)\") -eq $ref -and (IsPlaceholder $e) -and
                    -not (Test-Path $se.Tmp) -and -not (Test-Path $se.Aside) -and (Journals) -eq 0
            }
        }
        [void](Send "{`"cmd`":`"remove`",`"path`":`"$(J $e)`"}")
        if (-not $script:proc.HasExited) { [void](Send '{"cmd":"quit"}'); [void]$script:proc.WaitForExit(20000) }
    }

    # E. Drugoy kompyuter: chistaya papka dannyh, dvoynoy shchelchok po faylu otkrytiya
    $script:proc = StartCore '' $data2 $pipe2
    $opener = "$a\$ruOpen $ruName.zamok"
    Check 'another computer: the folder is not in its list' { -not (Folder $a $pipe2) }
    Check 'adopt_check: the opener with a real vault next to it is accepted' { (SendTo $pipe2 "{`"cmd`":`"adopt_check`",`"path`":`"$(J $opener)`"}").ok }
    New-Item -ItemType Directory -Force "$root\fake\$ruName" | Out-Null
    New-Item -ItemType Directory -Force "$root\fake\.$ruName.zamok" | Out-Null
    Check 'adopt_check: a hidden folder without vault files is refused' {
        (SendTo $pipe2 "{`"cmd`":`"adopt_check`",`"path`":`"$(J "$root\fake\$ruName")`"}").error -eq 'bad_vault'
    }
    $r = SendTo $pipe2 "{`"cmd`":`"adopt`",`"path`":`"$(J $opener)`",`"password`":`"wrong`"}"
    Check 'adopt with a wrong password: refused, not added' { $r.error -eq 'wrong_password' -and -not (Folder $a $pipe2) }
    $r = SendTo $pipe2 "{`"cmd`":`"adopt`",`"path`":`"$(J $opener)`",`"password`":`"$pw`"}"
    Check 'adopt with the password: added in letter mode and opened, same content' {
        $r.ok -and $r.folder.letter -and $r.folder.state -eq 'unlocked' -and $r.folder.path -eq $a -and (Manifest "$($r.folder.drive)\") -eq $refA
    }
    Check 'adopted folder is saved in the list of the other computer' {
        $cfg = Get-Content "$data2\folders.json" -Raw -Encoding UTF8 | ConvertFrom-Json
        $cfg.folders.Count -eq 1 -and $cfg.folders[0].letter -and $cfg.folders[0].rel_vault -like "*.$ruName.zamok"
    }
    [void](SendTo $pipe2 '{"cmd":"quit"}'); [void]$script:proc.WaitForExit(20000)

    # F. Predel FAT32: fayl, kotoryy v zashifrovannom vide bolshe 4 GiB, otklonyaetsya do kopirovaniya
    $script:proc = StartCore '' $data $pipeName 'fat32'
    $f = "$root\f\$ruName"; MakeSample $f 2
    $fs = [IO.File]::Create("$f\video.bin"); $fs.SetLength(4GB - 1); $fs.Dispose()
    $r = Send (EncryptReq $f)
    Check 'FAT32: a 4 GiB - 1 file is refused (its ciphertext is larger), with its name' {
        $r.error -eq 'file_too_large' -and $r.examples[0] -eq 'video.bin' -and -not (Test-Path (Sib $f).Tmp) -and (Journals) -eq 0
    }
    Remove-Item "$f\video.bin" -Force
    [void](Send '{"cmd":"quit"}'); [void]$script:proc.WaitForExit(20000)
    $script:proc = StartCore
    if (-not $force) {
        # Na exFAT predela net: fayl bolshe 4 GiB prinimaetsya proverkoy
        $g = "$root\g\$ruName"; New-Item -ItemType Directory -Force $g | Out-Null
        # Melkie fayly idut ranshe bolshogo (po alfavitu): otmena uspevaet do nego
        if (-not $Big) { 1..300 | ForEach-Object { [IO.File]::WriteAllText("$g\a$($_.ToString('000')).txt", "$_") } }
        $fs = [IO.File]::Create("$g\huge.bin"); $fs.SetLength(4GB + 512MB); $fs.Dispose()
        if ($Big) {
            $jg = EncryptWait $g
            $fg = Folder $g
            Check 'exFAT: a 4.5 GiB file is encrypted (no FAT32 limit), size is right at the drive' {
                $jg.state -eq 'done' -and (Get-Item "$($fg.drive)\huge.bin").Length -eq (4GB + 512MB) -or $(throw "job=$($jg | ConvertTo-Json -Compress)")
            }
            [void](Lock $g)
            Check 'exFAT: the ciphertext file is larger than 4 GiB' {
                @(Get-ChildItem -LiteralPath (Sib $g).Vault -Recurse -Force -File | Where-Object Length -gt 4GB).Count -eq 1
            }
            [void](Send "{`"cmd`":`"remove`",`"path`":`"$(J $g)`"}")
        } else {
            $r = Send (EncryptReq $g)
            if ($r.ok) { [void](Send "{`"cmd`":`"cancel_job`",`"id`":`"$($r.job.id)`"}"); $jg = WaitJob $r.job.id }
            Check 'exFAT: a 4.5 GiB file is accepted by the checks (cancelled before it is copied, original untouched)' {
                $r.ok -and $jg.error -eq 'cancelled' -and (Get-Item "$g\huge.bin").Length -eq (4GB + 512MB) -and -not (Test-Path (Sib $g).Tmp)
            }
        }
    }

    # G. Rasshifrovat navsegda: zaglushka uhodit, na meste obychnaya papka
    $r = Send "{`"cmd`":`"decrypt`",`"path`":`"$(J $a)`",`"password`":`"$pw`"}"
    $jd = if ($r.ok) { WaitJob $r.job.id } else { $r }
    Check 'decrypt permanently: plain folder with the content, no opener, no vault' {
        $jd.state -eq 'done' -and -not (Folder $a) -and (Manifest $a) -eq $refA -and -not (Test-Path $sa.Vault) -or $(throw "job=$($jd | ConvertTo-Json -Compress)")
    }
    [void](Send '{"cmd":"quit"}'); [void]$script:proc.WaitForExit(20000)

    Check 'log has no folder names, paths or passwords' {
        $log = (Get-Content "$data\core.log" -Raw -Encoding UTF8) + (Get-Content "$data2\core.log" -Raw -Encoding UTF8)
        $bad = @($ruName, $ruPapka, $root, $pw) | Where-Object { $log.Contains($_) }
        if ($bad) { "found in log: $($bad -join ', ')" -eq '' } else { 'clean' }
    }
}
finally {
    foreach ($p in @($script:proc)) {
        if ($p -and -not $p.HasExited) {
            try { [void](Send '{"cmd":"quit"}') } catch {}
            try { [void](SendTo $pipe2 '{"cmd":"quit"}') } catch {}
            if (-not $p.WaitForExit(20000)) { $p.Kill() }
        }
    }
    foreach ($s in @($substs)) { & subst $s /d 2>&1 | Out-Null }
    if (-not $force) { Remove-Item $root -Recurse -Force -ErrorAction SilentlyContinue }
    $results | Format-Table -AutoSize -Wrap | Out-String -Width 220
    "passed: $(@($results | Where-Object Ok).Count) / $($results.Count)"
}
