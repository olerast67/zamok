# Etap 4: shifrovanie sushchestvuyushchey papki na meste (DESIGN.md, razdel 5).
# Vse papki tolko vnutri tests\tmp\encrypt. Core zapuskaetsya so svoey papkoy dannyh i svoim
# kanalom, nastoyashchiy %LOCALAPPDATA%\Zamok ne zatragivaetsya.
# Glavnoe: process snimaetsya avariyno na kazhdom shage (ZAMOK_TEST_CRASH), posle perezapuska
# libo original na meste i ne tronut, libo papka zashifrovana i soderzhimoe to zhe.
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
function StartCore([string]$crash = '') {
    $psi = New-Object Diagnostics.ProcessStartInfo $coreExe
    $psi.UseShellExecute = $false
    $psi.EnvironmentVariables['ZAMOK_DATA_DIR'] = $data
    $psi.EnvironmentVariables['ZAMOK_PIPE'] = "\\.\pipe\$pipeName"
    $psi.EnvironmentVariables['ZAMOK_NO_TRAY'] = '1'
    if ($crash) { $psi.EnvironmentVariables['ZAMOK_TEST_CRASH'] = $crash }
    $p = [Diagnostics.Process]::Start($psi)
    [void](WaitFor { (Test-Path "\\.\pipe\$pipeName") -or $p.HasExited } 60000)
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
function Sib([string]$path) {
    $parent = Split-Path $path -Parent; $name = Split-Path $path -Leaf
    @{ Tmp = "$parent\.$name.zamok.tmp"; Vault = "$parent\.$name.zamok"; Aside = "$parent\.$name.zamok-original" }
}
function Journals { @(Get-ChildItem "$data\jobs" -Filter *.json -ErrorAction SilentlyContinue).Count }
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
    Check 'vault is next to the folder and hidden' { ((Get-Item -LiteralPath $sa.Vault -Force).Attributes -band [IO.FileAttributes]::Hidden) -ne 0 }
    Check 'lock, then recovery key opens it' {
        [void](Send "{`"cmd`":`"lock`",`"path`":`"$(J $a)`"}")
        $r = Send "{`"cmd`":`"unlock_recovery`",`"path`":`"$(J $a)`",`"words`":`"$($resp.recovery_key)`"}"
        $r.ok -and (Manifest $a) -eq $ref
    }
    Check 'encrypting it again is refused' { (Send (EncryptReq $a)).error -eq 'not_a_plain_folder' }
    $j = 'C:\dev\tools\jdk-26.0.2.1+1\bin'; $t = "$repo\tests\fixtures-tool"
    if ((Test-Path "$j\java.exe") -and (Test-Path "$t\cp.txt")) {
        [void](Send "{`"cmd`":`"lock`",`"path`":`"$(J $a)`"}")
        $cp = (Get-Content "$t\cp.txt").Substring(4).Trim([char]34)
        & "$j\java.exe" "-Dfile.encoding=UTF-8" "-Dsun.jnu.encoding=UTF-8" -cp "$t/out;$cp" ReadVault $sa.Vault "$root\a-cryptomator" $pw 2>&1 | Out-Null
        Check 'Cryptomator reads the encrypted folder: same content' {
            # Cryptomator ne sozdaet pustye papki v vygruzke inache, sravnivaem fayly
            $f = { param($m) ($m -split "`n" | Where-Object { $_ -like 'F *' }) -join "`n" }
            (& $f (Manifest "$root\a-cryptomator")) -eq (& $f $ref)
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
        @{ P = 'copying'; Back = $true }, @{ P = 'copy:3'; Back = $true }, @{ P = 'verified_before_journal'; Back = $true },
        @{ P = 'verified'; Back = $false }, @{ P = 'vault_placed_before_journal'; Back = $false }, @{ P = 'vault_placed'; Back = $false },
        @{ P = 'original_aside_before_journal'; Back = $false }, @{ P = 'original_aside'; Back = $false },
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
            Check "crash at $($pt.P) (exit $code): original back untouched, nothing left" {
                -not (IsMount $e) -and (Manifest $e) -eq $ref -and -not (Test-Path $se.Tmp) -and -not (Test-Path $se.Vault) -and
                    -not (Test-Path $se.Aside) -and (Journals) -eq 0 -and -not (Folder $e)
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
