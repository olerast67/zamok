# Etap 4: shifrovanie sushchestvuyushchey papki na meste (DESIGN.md, razdel 5).
# Vse papki tolko vnutri tests\tmp\encrypt. Core zapuskaetsya so svoey papkoy dannyh i svoim
# kanalom, nastoyashchiy %LOCALAPPDATA%\Zamok ne zatragivaetsya.
# Glavnoe: process snimaetsya avariyno na kazhdom shage (ZAMOK_TEST_CRASH), posle perezapuska
# libo original na meste i ne tronut, libo papka zashifrovana i soderzhimoe to zhe.
# Avariya vo vremya kopirovaniya ostavlyaet kopiyu i zhurnal: povtornoe shifrovanie toy zhe papki
# prodolzhaet s mesta ostanovki (razdel G).
# Zapusk: powershell -NoProfile -ExecutionPolicy Bypass -File tests\core\test-encrypt.ps1
# Skript v ASCII: russkie imena sobirayutsya iz kodov simvolov.
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path "$PSScriptRoot/../..").Path
$root = "$repo\tests\tmp\encrypt"
$coreExe = "$repo\core\target\release\zamok-core.exe"
$pw = 'zamok-encrypt-test'
$pipeName = "zamok-enc-test-$PID"
$data = "$root\data"
$results = New-Object System.Collections.Generic.List[object]
function Ru([int[]]$c) { -join ($c | ForEach-Object { [char]$_ }) }
$ruName = Ru 0x041A,0x043B,0x0438,0x0435,0x043D,0x0442,0x044B   # Klienty
$ruPapka = Ru 0x041F,0x0430,0x043F,0x043A,0x0430                 # Papka
$ruPrivet = Ru 0x043F,0x0440,0x0438,0x0432,0x0435,0x0442         # privet
$ruLong = (Ru 0x0434) * 120                                      # dddd... (120)
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
function StartCore([string]$crash = '', [string]$pause = '') {
    $psi = New-Object Diagnostics.ProcessStartInfo $coreExe
    $psi.UseShellExecute = $false
    $psi.EnvironmentVariables['ZAMOK_DATA_DIR'] = $data
    $psi.EnvironmentVariables['ZAMOK_PIPE'] = "\\.\pipe\$pipeName"
    $psi.EnvironmentVariables['ZAMOK_NO_TRAY'] = '1'
    $psi.EnvironmentVariables['ZAMOK_SEARCH_DRY_RUN'] = '1'
    if ($crash) { $psi.EnvironmentVariables['ZAMOK_TEST_CRASH'] = $crash }
    if ($pause) { $psi.EnvironmentVariables['ZAMOK_TEST_PAUSE'] = $pause }
    $p = [Diagnostics.Process]::Start($psi)
    [void](WaitFor { (Test-Path "\\.\pipe\$pipeName") -or $p.HasExited } 60000)
    # Kanal sozdaetsya do vosstanovleniya posle sboya, a otvechat core nachinaet tolko posle nego:
    # pervyy otvet znachit, chto vosstanovlenie zakoncheno
    if (-not $p.HasExited) { try { [void](Send '{"cmd":"status"}') } catch {} }
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
function Folder([string]$path) { (Send '{"cmd":"status"}').folders | Where-Object { $_.path -eq $path } }
function EncryptReq([string]$path, [string]$p = $pw) { "{`"cmd`":`"encrypt`",`"path`":`"$(J $path)`",`"password`":`"$p`"}" }
function WaitJob([string]$id, [int]$ms = 180000) {
    $deadline = (Get-Date).AddMilliseconds($ms)
    while ($true) {
        $j = (Send "{`"cmd`":`"job_status`",`"id`":`"$id`"}").job
        if ($j.state -ne 'running') { return $j }
        if ((Get-Date) -gt $deadline) { throw "job still running: $($j | ConvertTo-Json -Compress)" }
        Start-Sleep -Milliseconds 200
    }
}

# Primer papki: russkie imena, vlozhennost, pustaya papka, pustoy fayl, fayl 3 MB, dlinnoe imya,
# mnogo melkih faylov. Soderzhimoe vsegda odinakovoe (seed), chtoby sravnivat s etalonom.
function MakeSample([string]$dir, [int]$many = 30) {
    New-Item -ItemType Directory -Force "$dir\$ruPapka\vlozh", "$dir\pustaya", "$dir\many" | Out-Null
    [IO.File]::WriteAllText("$dir\$ruPrivet.txt", $ruPrivet, (New-Object Text.UTF8Encoding $false))
    [IO.File]::WriteAllBytes("$dir\empty.bin", [byte[]]@())
    $rnd = New-Object Random 42; $big = New-Object byte[] (3MB); $rnd.NextBytes($big)
    [IO.File]::WriteAllBytes("$dir\$ruPapka\big.bin", $big)
    [IO.File]::WriteAllText("$dir\$ruPapka\vlozh\$ruLong.txt", 'long name')
    1..$many | ForEach-Object { [IO.File]::WriteAllText("$dir\many\f$_.txt", "file $_") }
}
# Opis: papki i fayly s SHA-256, otnositelnye puti, po alfavitu
function Manifest([string]$dir) {
    $base = (Get-Item -LiteralPath $dir -Force).FullName.TrimEnd('\')
    $sha = [Security.Cryptography.SHA256]::Create()
    @(Get-ChildItem -LiteralPath $dir -Recurse -Force | ForEach-Object {
        $rel = $_.FullName.Substring($base.Length + 1).ToLower()
        if ($_.PSIsContainer) { "D $rel" } else {
            $b = [IO.File]::ReadAllBytes($_.FullName)
            "F $rel " + [BitConverter]::ToString($sha.ComputeHash($b))
        }
    } | Sort-Object) -join "`n"
}
function Sha([string]$file) {
    $sha = [Security.Cryptography.SHA256]::Create()
    [BitConverter]::ToString($sha.ComputeHash([IO.File]::ReadAllBytes($file)))
}
function Sib([string]$path) {
    $parent = Split-Path $path -Parent; $name = Split-Path $path -Leaf
    @{ Tmp = "$parent\.$name.zamok.tmp"; Vault = "$parent\.$name.zamok"; Aside = "$parent\.$name.zamok-original" }
}
function Journals { @(Get-ChildItem "$data\jobs" -Filter *.json -ErrorAction SilentlyContinue).Count }
# Otpechatki faylov shifroteksta (bez dir.c9r i dirid.c9r): shifrovanie sluchaynoe, poetomu
# nepodmenennyy fayl = tot zhe otpechatok
function CtHashes([string]$vault) {
    @(Get-ChildItem -LiteralPath $vault -Recurse -Force -File | Where-Object { $_.Name -like '*.c9r' -and $_.Name -ne 'dir.c9r' -and $_.Name -ne 'dirid.c9r' } |
        ForEach-Object { Sha $_.FullName })
}
$javaDir = 'C:\dev\tools\jdk-26.0.2.1+1\bin'; $toolDir = "$repo\tests\fixtures-tool"
$haveCryptomator = (Test-Path "$javaDir\java.exe") -and (Test-Path "$toolDir\cp.txt")
# Cryptomator chitaet hranilishche (papka dolzhna byt zaperta); sravnivayutsya fayly
function CryptomatorFiles([string]$vault, [string]$out) {
    $cp = (Get-Content "$toolDir\cp.txt").Substring(4).Trim([char]34)
    & "$javaDir\java.exe" "-Dfile.encoding=UTF-8" "-Dsun.jnu.encoding=UTF-8" -cp "$toolDir/out;$cp" ReadVault $vault $out $pw 2>&1 | Out-Null
    (Manifest $out) -split "`n" | Where-Object { $_ -like 'F *' }
}
function FilesOf([string]$manifest) { $manifest -split "`n" | Where-Object { $_ -like 'F *' } }
function IsMount([string]$p) { $i = Get-Item -LiteralPath $p -Force -ErrorAction SilentlyContinue; $i -and ($i.Attributes -band [IO.FileAttributes]::ReparsePoint) }

# Podgotovka
Get-Process zamok-core -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $coreExe } | Stop-Process -Force
Start-Sleep -Milliseconds 500
Remove-Item $root -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $root | Out-Null
Push-Location "$repo\core"
& "$env:USERPROFILE\.cargo\bin\cargo.exe" build -q --release -p zamok-core 2>&1 | Out-Null
Pop-Location
MakeSample "$root\ref"
$ref = Manifest "$root\ref"

$script:proc = $null
try {
    $script:proc = StartCore

    # A. Obychnoe shifrovanie
    $a = "$root\a\$ruName"; MakeSample $a; $sa = Sib $a
    # Daty i atributy: odin fayl tolko dlya chteniya i skrytyy, odna papka s davney datoy
    Set-ItemProperty -LiteralPath "$a\$ruPrivet.txt" -Name Attributes -Value 'ReadOnly, Hidden'
    (Get-Item -LiteralPath "$a\pustaya").LastWriteTimeUtc = [datetime]'2020-01-02T03:04:05Z'
    $times = @{}
    Get-ChildItem -LiteralPath $a -Recurse -Force | ForEach-Object { $times[$_.FullName.Substring($a.Length)] = "$($_.LastWriteTimeUtc.Ticks) $($_.CreationTimeUtc.Ticks)" }
    $resp = Send (EncryptReq $a)
    Check 'encrypt: job started, recovery key has 44 words' {
        $resp.ok -and $resp.job.id -and @($resp.recovery_key -split ' ').Count -eq 44
    }
    $job = WaitJob $resp.job.id
    Check 'encrypt: job done' { $job.state -eq 'done' -or "job=$($job | ConvertTo-Json -Compress)" -eq '' }
    Check 'encrypt: progress counted every file' { $job.files_done -eq $job.files_total -and $job.files_total -eq 34 }
    Check 'after encryption the folder is open on the same path' { (Folder $a).state -eq 'unlocked' -and (IsMount $a) }
    Check 'content is identical to the original' { (Manifest $a) -eq $ref }
    Check 'dates of files and folders are kept' {
        # Fayly i papka s yavno zadannoy datoy. Vremya tolko chto zapolnennyh papok NTFS
        # dopisyvaet s zaderzhkoy, ego v snimke skripta i v opisi core mozhet ne byt.
        $diff = @(Get-ChildItem -LiteralPath $a -Recurse -Force | Where-Object { -not $_.PSIsContainer -or $_.Name -eq 'pustaya' } |
            Where-Object { $times[$_.FullName.Substring($a.Length)] -ne "$($_.LastWriteTimeUtc.Ticks) $($_.CreationTimeUtc.Ticks)" })
        if ($diff.Count) { throw "changed: $($diff[0].FullName.Substring($a.Length)) of $($diff.Count)" }
        "items=$($times.Count)"
    }
    Check 'read-only and hidden attributes are kept' {
        $at = (Get-Item -LiteralPath "$a\$ruPrivet.txt" -Force).Attributes
        ($at -band [IO.FileAttributes]::ReadOnly) -ne 0 -and ($at -band [IO.FileAttributes]::Hidden) -ne 0
    }
    Check 'original, tmp copy and journal are gone' { -not (Test-Path $sa.Aside) -and -not (Test-Path $sa.Tmp) -and (Journals) -eq 0 }
    Check 'vault is next to the folder, hidden and system' {
        $at = (Get-Item -LiteralPath $sa.Vault -Force).Attributes
        ($at -band [IO.FileAttributes]::Hidden) -ne 0 -and ($at -band [IO.FileAttributes]::System) -ne 0
    }
    Check 'lock, then recovery key opens it' {
        [void](Send "{`"cmd`":`"lock`",`"path`":`"$(J $a)`"}")
        $r = Send "{`"cmd`":`"unlock_recovery`",`"path`":`"$(J $a)`",`"words`":`"$($resp.recovery_key)`"}"
        $r.ok -and (Manifest $a) -eq $ref
    }
    Check 'encrypting it again is refused' { (Send (EncryptReq $a)).error -eq 'not_a_plain_folder' }
    if ($haveCryptomator) {
        [void](Send "{`"cmd`":`"lock`",`"path`":`"$(J $a)`"}")
        Check 'Cryptomator reads the encrypted folder: same content' {
            # Cryptomator ne sozdaet pustye papki v vygruzke inache, sravnivaem fayly
            ((CryptomatorFiles $sa.Vault "$root\a-cryptomator") -join "`n") -eq ((FilesOf $ref) -join "`n")
        }
    }

    # B. Otkazy proverok: papka ne menyaetsya
    $b = "$root\b\$ruName"; MakeSample $b
    Check 'empty password is refused' { (Send (EncryptReq $b '')).error -eq 'bad_request' }
    $held = [IO.File]::Open("$b\$ruPapka\big.bin", 'Open', 'Read', 'Read')
    Check 'file open in another program: refused with its name' {
        $r = Send (EncryptReq $b)
        $r.error -eq 'files_in_use' -and $r.examples[0] -eq "$ruPapka\big.bin"
    }
    $held.Dispose()
    New-Item -ItemType Directory (Sib $b).Vault | Out-Null
    Check 'existing .zamok next to it: refused' { (Send (EncryptReq $b)).error -eq 'vault_exists' }
    Remove-Item (Sib $b).Vault
    $sys = "$env:LOCALAPPDATA\Temp\zamok-system-test-$PID"; New-Item -ItemType Directory -Force $sys | Out-Null
    Check 'folder inside AppData: refused as system' { (Send (EncryptReq $sys)).error -eq 'system_folder' }
    Remove-Item $sys -Force
    Check 'refused folder is untouched, nothing left next to it' {
        (Manifest $b) -eq $ref -and -not (Test-Path (Sib $b).Tmp) -and (Journals) -eq 0
    }

    # C. Otmena vo vremya kopirovaniya
    $c = "$root\c\$ruName"; MakeSample $c 1500; $refC = Manifest $c
    $r = Send (EncryptReq $c)
    [void](Send "{`"cmd`":`"cancel_job`",`"id`":`"$($r.job.id)`"}")
    $jc = WaitJob $r.job.id
    Check 'cancel: job failed with cancelled' { $jc.error -eq 'cancelled' -or "job=$($jc | ConvertTo-Json -Compress)" -eq '' }
    Check 'cancel: original untouched, tmp copy deleted' { (Manifest $c) -eq $refC -and -not (Test-Path (Sib $c).Tmp) -and (Journals) -eq 0 }

    # D. Papku derzhit programma (tekushchaya papka processa): pereimenovat nelzya, otkat
    $d = "$root\d\$ruName"; MakeSample $d
    $holder = Start-Process powershell -ArgumentList '-NoProfile', '-Command', 'Start-Sleep 120' -WorkingDirectory "$d\$ruPapka" -PassThru -WindowStyle Hidden
    Start-Sleep -Milliseconds 800
    $r = Send (EncryptReq $d)
    $jd = WaitJob $r.job.id
    Check 'folder held by a program: folder_in_use, rolled back' {
        $jd.error -eq 'folder_in_use' -and (Manifest $d) -eq $ref -and -not (Test-Path (Sib $d).Vault) -and -not (Test-Path (Sib $d).Tmp) -and (Journals) -eq 0
    }
    Stop-Process $holder.Id -Force -ErrorAction SilentlyContinue

    [void](Send '{"cmd":"quit"}'); [void]$script:proc.WaitForExit(20000)

    # E. Avariya na kazhdom shage, potom perezapusk
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
            # Kopirovanie prervano: original ne tronut, kopiya i zhurnal zhdut prodolzheniya
            Check "crash at $($pt.P) (exit $code): original untouched, copy waits to be continued" {
                -not (IsMount $e) -and (Manifest $e) -eq $ref -and (Test-Path $se.Tmp) -and -not (Test-Path $se.Vault) -and
                    -not (Test-Path $se.Aside) -and (Journals) -eq 1 -and -not (Folder $e)
            }
            $r = Send (EncryptReq $e)
            $je = if ($r.ok) { WaitJob $r.job.id } else { $r }
            Check "crash at $($pt.P), encrypt again: continued, same content, nothing left" {
                $je.state -eq 'done' -and (Folder $e).state -eq 'unlocked' -and (Manifest $e) -eq $ref -and -not (Test-Path $se.Tmp) -and -not (Test-Path $se.Aside) -and
                    (Journals) -eq 0 -or $(throw "job=$($je | ConvertTo-Json -Compress)")
            }
        } else {
            Check "crash at $($pt.P) (exit $code): encrypted, same content, nothing left" {
                $f = Folder $e
                $u = Send "{`"cmd`":`"unlock`",`"path`":`"$(J $e)`",`"password`":`"$pw`"}"
                $f -and $u.ok -and (Manifest $e) -eq $ref -and -not (Test-Path $se.Tmp) -and -not (Test-Path $se.Aside) -and (Journals) -eq 0
            }
        }
        if (-not $script:proc.HasExited) { [void](Send '{"cmd":"quit"}'); [void]$script:proc.WaitForExit(20000) }
    }

    # F. Fayl poyavilsya posle posledney proverki, poka shel sbros na disk: on ne dolzhen propast
    $added = 'added-during-flush.txt'
    function NothingLeft($p) { -not (Test-Path $p.Tmp) -and -not (Test-Path $p.Vault) -and -not (Test-Path $p.Aside) -and (Journals) -eq 0 }
    function FlushJob([string]$dir, [string]$gate, [scriptblock]$during) {
        $resp = Send (EncryptReq $dir)
        [void](WaitFor { Test-Path "$gate.reached" } 60000)
        & $during
        New-Item -ItemType File $gate | Out-Null
        $resp
    }
    $g1 = "$root\gate1"; $f1 = "$root\f1\$ruName"; MakeSample $f1; $s1 = Sib $f1
    $script:proc = StartCore '' "flush_window|$g1"
    $r1 = FlushJob $f1 $g1 { [IO.File]::WriteAllText("$f1\$added", 'must survive') }
    $j1 = WaitJob $r1.job.id
    Check 'file added during the flush: job refused, the file and the original survive' {
        $j1.error -eq 'changed_during_copy' -and -not (IsMount $f1) -and
            ((Get-Content -LiteralPath "$f1\$added" -Raw) -eq 'must survive') -and
            (Manifest $f1) -eq ((@($ref -split "`n") + "F $added $(Sha "$f1\$added")" | Sort-Object) -join "`n") -and
            (NothingLeft $s1) -and -not (Folder $f1)
    }
    [void](Send '{"cmd":"quit"}'); [void]$script:proc.WaitForExit(20000)
    Remove-Item "$g1", "$g1.reached" -Force
    $f2 = "$root\f2\$ruName"; MakeSample $f2; $s2 = Sib $f2
    $script:proc = StartCore '' "flush_window|$g1"
    $r2 = FlushJob $f2 $g1 { [IO.File]::WriteAllText("$f2\many\f1.txt", 'changed during the flush') }
    $j2 = WaitJob $r2.job.id
    Check 'file changed during the flush: job refused, the new content survives' {
        $j2.error -eq 'changed_during_copy' -and -not (IsMount $f2) -and
            ((Get-Content -LiteralPath "$f2\many\f1.txt" -Raw) -eq 'changed during the flush') -and (NothingLeft $s2)
    }
    [void](Send '{"cmd":"quit"}'); [void]$script:proc.WaitForExit(20000)
    Remove-Item "$g1", "$g1.reached" -Force
    # Zaperli vse (Win+L, son) vo vremya raboty: v konce papka ne otkryvaetsya
    $f3 = "$root\f3\$ruName"; MakeSample $f3
    $script:proc = StartCore '' "flush_window|$g1"
    $r3 = FlushJob $f3 $g1 { [void](Send '{"cmd":"lock_all"}') }
    $j3 = WaitJob $r3.job.id
    Check 'lock all during the job: encrypted, but stays locked' {
        $j3.state -eq 'done' -and (Folder $f3).state -eq 'locked' -and (Journals) -eq 0
    }
    Check 'that folder opens with the password, same content' {
        $u = Send "{`"cmd`":`"unlock`",`"path`":`"$(J $f3)`",`"password`":`"$pw`"}"
        $u.ok -and (Manifest $f3) -eq $ref
    }
    [void](Send '{"cmd":"quit"}'); [void]$script:proc.WaitForExit(20000)
    # Avariya posle sbrosa na disk, fayl poyavilsya do perezapuska: vosstanovlenie sveryaet s opisyu
    foreach ($case in @(@{ P = 'verified'; InAside = $false }, @{ P = 'original_aside'; InAside = $true })) {
        $e = "$root\r-$($case.P)\$ruName"; MakeSample $e; $se = Sib $e
        $script:proc = StartCore $case.P
        try { [void](Send (EncryptReq $e)) } catch {}
        [void]$script:proc.WaitForExit(120000)
        $where = if ($case.InAside) { $se.Aside } else { $e }
        [IO.File]::WriteAllText("$where\$added", 'must survive')
        $script:proc = StartCore
        Check "crash at $($case.P), file added before restart: recovery rolls back, the file survives" {
            -not (IsMount $e) -and (Test-Path -LiteralPath "$e\$added") -and
                ((Get-Content -LiteralPath "$e\$added" -Raw) -eq 'must survive') -and (NothingLeft $se) -and -not (Folder $e) -or
                $(throw "mount=$(IsMount $e) file=$(Test-Path -LiteralPath "$e\$added") tmp=$(Test-Path $se.Tmp) vault=$(Test-Path $se.Vault) aside=$(Test-Path $se.Aside) journals=$(Journals) folder=$([bool](Folder $e))")
        }
        [void](Send '{"cmd":"quit"}'); [void]$script:proc.WaitForExit(20000)
    }

    # G. Prodolzhenie kopirovaniya s mesta ostanovki
    function CrashAt([string]$point, [string]$dir) {
        if ($script:proc -and -not $script:proc.HasExited) { [void](Send '{"cmd":"quit"}'); [void]$script:proc.WaitForExit(20000) }
        $script:proc = StartCore $point
        try { [void](Send (EncryptReq $dir)) } catch {}
        if (-not $script:proc.WaitForExit(120000)) { $script:proc.Kill() }
        $script:proc = StartCore
    }
    function EncryptWait([string]$dir, [string]$p = $pw) {
        $r = Send (EncryptReq $dir $p)
        if (-not $r.ok) { return $r }
        WaitJob $r.job.id
    }
    function Waits($p, [string]$dir) { -not (IsMount $dir) -and (Test-Path $p.Tmp) -and (Journals) -eq 1 -and -not (Folder $dir) }
    function Finished($p) { -not (Test-Path $p.Tmp) -and -not (Test-Path $p.Aside) -and (Journals) -eq 0 }

    # G1. Avariya posle 20 sverennyh faylov: oni ostayutsya v kopii, ostalnoe dopisyvaetsya
    $g1 = "$root\g1\$ruName"; MakeSample $g1; $sg1 = Sib $g1
    CrashAt 'copy:20' $g1
    Check 'resume: after a crash at copy:20 the original is untouched, copy and journal wait' { (Waits $sg1 $g1) -and (Manifest $g1) -eq $ref }
    $before = CtHashes $sg1.Tmp
    $jg1 = EncryptWait $g1
    Check 'resume: done, same content, nothing left' {
        $jg1.state -eq 'done' -and (Manifest $g1) -eq $ref -and (Finished $sg1) -or $(throw "job=$($jg1 | ConvertTo-Json -Compress)")
    }
    Check 'resume: the 20 verified files were not copied again' {
        $after = CtHashes $sg1.Vault
        $same = @($before | Where-Object { $after -contains $_ }).Count
        if ($before.Count -ne 20 -or $same -ne 20) { throw "before=$($before.Count) kept=$same" }
        "kept=$same of $($after.Count)"
    }
    if ($haveCryptomator) {
        [void](Send "{`"cmd`":`"lock`",`"path`":`"$(J $g1)`"}")
        Check 'resume: Cryptomator reads the continued vault, same content' {
            ((CryptomatorFiles $sg1.Vault "$root\g1-cryptomator") -join "`n") -eq ((FilesOf $ref) -join "`n")
        }
    }

    # G2. Avariya posle zapisi 10-go fayla do ego sverki, mezhdu zapuskami papka menyalas,
    # odin fayl shifroteksta isporchen: v itoge rovno novoe soderzhimoe papki
    $g2 = "$root\g2\$ruName"; MakeSample $g2; $sg2 = Sib $g2
    CrashAt 'copy_written:10' $g2
    Check 'resume: crash before the check of a written file leaves the copy waiting' { (Waits $sg2 $g2) -and (Manifest $g2) -eq $ref }
    [IO.File]::WriteAllText("$g2\many\f1.txt", 'changed between runs')
    Remove-Item -LiteralPath "$g2\many\f30.txt"
    Remove-Item -LiteralPath "$g2\pustaya"
    [IO.File]::WriteAllText("$g2\new.txt", 'added between runs')
    $ct = Get-ChildItem -LiteralPath $sg2.Tmp -Recurse -Force -File | Where-Object { $_.Name -like '*.c9r' -and $_.Name -ne 'dir.c9r' -and $_.Name -ne 'dirid.c9r' -and $_.Length -gt 100 } | Select-Object -First 1
    $bytes = [IO.File]::ReadAllBytes($ct.FullName); [IO.File]::WriteAllBytes($ct.FullName, $bytes[0..($bytes.Length - 20)])
    $want2 = Manifest $g2
    $jg2 = EncryptWait $g2
    Check 'resume after changes in the folder and a damaged file: exactly the new content' {
        $jg2.state -eq 'done' -and (Manifest $g2) -eq $want2 -and (Finished $sg2) -or $(throw "job=$($jg2 | ConvertTo-Json -Compress)")
    }
    if ($haveCryptomator) {
        [void](Send "{`"cmd`":`"lock`",`"path`":`"$(J $g2)`"}")
        Check 'resume: Cryptomator reads it, same content as the changed folder' {
            ((CryptomatorFiles $sg2.Vault "$root\g2-cryptomator") -join "`n") -eq ((FilesOf $want2) -join "`n")
        }
    }

    # G3. Drugoy parol: prervannaya kopiya otkatyvaetsya, shifrovanie s nulya novym parolem
    $g3 = "$root\g3\$ruName"; MakeSample $g3; $sg3 = Sib $g3; $pw3 = 'zamok-other-password'
    CrashAt 'copy:5' $g3
    $before = CtHashes $sg3.Tmp
    $jg3 = EncryptWait $g3 $pw3
    Check 'resume with another password: starts over, the new password opens it' {
        [void](Send "{`"cmd`":`"lock`",`"path`":`"$(J $g3)`"}")
        $old = Send "{`"cmd`":`"unlock`",`"path`":`"$(J $g3)`",`"password`":`"$pw`"}"
        $new = Send "{`"cmd`":`"unlock`",`"path`":`"$(J $g3)`",`"password`":`"$pw3`"}"
        $after = CtHashes $sg3.Vault
        $jg3.state -eq 'done' -and -not $old.ok -and $new.ok -and (Manifest $g3) -eq $ref -and (Finished $sg3) -and
            @($before | Where-Object { $after -contains $_ }).Count -eq 0
    }

    # G4. Kopiya povrezhdena (dannye otrezany ot dereva): otkat i shifrovanie s nulya
    $g4 = "$root\g4\$ruName"; MakeSample $g4; $sg4 = Sib $g4
    CrashAt 'copy:5' $g4
    $orphan = "$($sg4.Tmp)\d\ZZ\ORPHANORPHANORPHANORPHANORPHAN22"
    New-Item -ItemType Directory -Force $orphan | Out-Null; [IO.File]::WriteAllText("$orphan\junk.c9r", 'x')
    $jg4 = EncryptWait $g4
    Check 'resume of a damaged copy: rolled back and encrypted from scratch, same content' {
        $jg4.state -eq 'done' -and (Manifest $g4) -eq $ref -and (Finished $sg4) -and -not (Test-Path "$($sg4.Vault)\d\ZZ\ORPHANORPHANORPHANORPHANORPHAN22") -or
            $(throw "job=$($jg4 | ConvertTo-Json -Compress)")
    }

    # G5. Shifruyut druguyu papku: prervannaya kopiya pervoy otkatyvaetsya, papka ne tronuta
    $g5 = "$root\g5\$ruName"; MakeSample $g5; $sg5 = Sib $g5
    $g5b = "$root\g5b\$ruName"; MakeSample $g5b; $sg5b = Sib $g5b
    CrashAt 'copy:3' $g5
    $sys5 = "$env:LOCALAPPDATA\Temp\zamok-system-test-$PID"; New-Item -ItemType Directory -Force $sys5 | Out-Null
    Check 'refused encryption of another folder keeps the interrupted copy' {
        (Send (EncryptReq $sys5)).error -eq 'system_folder' -and (Waits $sg5 $g5)
    }
    Remove-Item $sys5 -Force
    $jg5 = EncryptWait $g5b
    Check 'another folder encrypted meanwhile: the interrupted copy is dropped, its folder untouched' {
        $jg5.state -eq 'done' -and (Folder $g5b) -and -not (Test-Path $sg5.Tmp) -and (Manifest $g5) -eq $ref -and
            -not (Folder $g5) -and (Finished $sg5b)
    }
    [void](Send '{"cmd":"quit"}'); [void]$script:proc.WaitForExit(20000)

    # G6. Fayl izmenilsya posredi kopirovaniya: oshibka, kopiya zhdet; povtor dodelyvaet bez perezapuska
    $g6 = "$root\g6\$ruName"; MakeSample $g6; $sg6 = Sib $g6; $gate6 = "$root\gate6"
    $script:proc = StartCore '' "copy:5|$gate6"
    $r6 = Send (EncryptReq $g6)
    [void](WaitFor { Test-Path "$gate6.reached" } 60000)
    $big2 = New-Object byte[] (1MB); (New-Object Random 7).NextBytes($big2)
    [IO.File]::WriteAllBytes("$g6\$ruPapka\big.bin", $big2)
    $want6 = Manifest $g6
    New-Item -ItemType File $gate6 | Out-Null
    $jg6 = WaitJob $r6.job.id
    Check 'file changed during the copy: changed_during_copy, the copy waits, the folder has the new file' {
        $jg6.error -eq 'changed_during_copy' -and (Waits $sg6 $g6) -and (Manifest $g6) -eq $want6
    }
    $jg6b = EncryptWait $g6
    Check 'encrypt again in the same session: continued, the new content' {
        $jg6b.state -eq 'done' -and (Manifest $g6) -eq $want6 -and (Finished $sg6) -or $(throw "job=$($jg6b | ConvertTo-Json -Compress)")
    }
    [void](Send '{"cmd":"quit"}'); [void]$script:proc.WaitForExit(20000)

    Check 'log has no folder names, paths or passwords' {
        $log = Get-Content "$data\core.log" -Raw -Encoding UTF8
        $bad = @($ruName, $ruPapka, $root, $pw) | Where-Object { $log.Contains($_) }
        if ($bad) { "found in log: $($bad -join ', ')" -eq '' } else { 'clean' }
    }
}
finally {
    if ($script:proc -and -not $script:proc.HasExited) {
        try { [void](Send '{"cmd":"quit"}') } catch {}
        if (-not $script:proc.WaitForExit(20000)) { $script:proc.Kill() }
    }
    $results | Format-Table -AutoSize -Wrap | Out-String -Width 220
    "passed: $(@($results | Where-Object Ok).Count) / $($results.Count)"
}
