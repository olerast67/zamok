# Etap 4: "Rasshifrovat navsegda" (DESIGN.md, razdel 5) v obratnuyu storonu ot shifrovaniya.
# Vse papki tolko vnutri tests\tmp\decrypt, core so svoey papkoy dannyh i svoim kanalom.
# Glavnoe: process snimaetsya avariyno na kazhdom shage (ZAMOK_TEST_CRASH), posle perezapuska
# libo papka po-prezhnemu zashifrovana i otkryvaetsya parolem, libo ona obychnaya s tem zhe
# soderzhimym, a hranilishche udaleno.
# Zapusk: powershell -NoProfile -ExecutionPolicy Bypass -File tests\core\test-decrypt.ps1
# Skript v ASCII: russkie imena sobirayutsya iz kodov simvolov.
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path "$PSScriptRoot/../..").Path
$root = "$repo\tests\tmp\decrypt"
$coreExe = "$repo\core\target\release\zamok-core.exe"
$pw = 'zamok-decrypt-test'
$pipeName = "zamok-dec-test-$PID"
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

function DecJournals { @(Get-ChildItem "$data\decrypt-jobs" -Filter *.json -ErrorAction SilentlyContinue).Count }
function DecTmp([string]$path) { $parent = Split-Path $path -Parent; $name = Split-Path $path -Leaf; "$parent\.$name.zamok-decrypted.tmp" }
function DecryptReq([string]$path, [string]$p = $pw) { "{`"cmd`":`"decrypt`",`"path`":`"$(J $path)`",`"password`":`"$p`"}" }
function Encrypted([string]$dir) {
    MakeSample $dir
    $r = Send (EncryptReq $dir)
    $j = WaitJob $r.job.id
    if ($j.state -ne 'done') { throw "encrypt failed: $($j.error)" }
}

$script:proc = $null
try {
    $script:proc = StartCore

    # A. Obychnaya rasshifrovka
    $a = "$root\a\$ruName"
    Encrypted $a
    $times = @{}
    Get-ChildItem -LiteralPath $a -Recurse -Force -File | ForEach-Object { $times[$_.FullName.Substring($a.Length)] = $_.LastWriteTimeUtc.Ticks }
    Check 'wrong password: refused, folder stays encrypted' {
        (Send (DecryptReq $a 'nope')).error -eq 'wrong_password' -and (IsMount $a) -and (Folder $a)
    }
    $held = [IO.File]::Open("$a\$ruPrivet.txt", 'Open', 'Read', 'ReadWrite')
    Check 'file open in a program: refused' { (Send (DecryptReq $a)).error -eq 'files_in_use' }
    $held.Dispose()
    $r = Send (DecryptReq $a)
    $job = WaitJob $r.job.id
    Check 'decrypt: job done' { $job.state -eq 'done' -or "job=$($job | ConvertTo-Json -Compress)" -eq '' }
    Check 'folder is plain again with the same content' { -not (IsMount $a) -and (Manifest $a) -eq $ref }
    Check 'vault, tmp copy and journal are gone, folder left the list' {
        -not (Test-Path (Sib $a).Vault) -and -not (Test-Path (DecTmp $a)) -and (DecJournals) -eq 0 -and -not (Folder $a)
    }
    Check 'file dates are kept' {
        $diff = @(Get-ChildItem -LiteralPath $a -Recurse -Force -File | Where-Object { $times[$_.FullName.Substring($a.Length)] -ne $_.LastWriteTimeUtc.Ticks })
        if ($diff.Count) { throw "changed: $($diff[0].Name) of $($diff.Count)" }
        "files=$($times.Count)"
    }
    Check 'the folder can be encrypted again' {
        $r = Send (EncryptReq $a); (WaitJob $r.job.id).state -eq 'done' -and (Manifest $a) -eq $ref
    }
    [void](Send '{"cmd":"quit"}'); [void]$script:proc.WaitForExit(20000)

    # B. Avariya na kazhdom shage, potom perezapusk
    $points = @(
        @{ P = 'decrypt_copying'; Back = $true }, @{ P = 'decrypt_copied_before_journal'; Back = $true },
        @{ P = 'decrypt_copied'; Back = $false }, @{ P = 'decrypt_unmounted_before_journal'; Back = $false },
        @{ P = 'decrypt_unmounted'; Back = $false }, @{ P = 'decrypt_placed_before_journal'; Back = $false },
        @{ P = 'decrypt_placed'; Back = $false }
    )
    $i = 0
    foreach ($pt in $points) {
        $i++
        $e = "$root\e$i\$ruName"
        $script:proc = StartCore
        Encrypted $e
        [void](Send '{"cmd":"quit"}'); [void]$script:proc.WaitForExit(20000)
        $script:proc = StartCore $pt.P
        try { [void](Send (DecryptReq $e)) } catch {}
        $died = $script:proc.WaitForExit(120000)
        $code = if ($died) { $script:proc.ExitCode } else { $script:proc.Kill(); 'alive' }
        $script:proc = StartCore
        if ($pt.Back) {
            Check "crash at $($pt.P) (exit $code): still encrypted, opens, nothing left" {
                $u = Send "{`"cmd`":`"unlock`",`"path`":`"$(J $e)`",`"password`":`"$pw`"}"
                $u.ok -and (Manifest $e) -eq $ref -and -not (Test-Path (DecTmp $e)) -and (DecJournals) -eq 0
            }
        } else {
            Check "crash at $($pt.P) (exit $code): plain folder, same content, vault gone" {
                (WaitFor { -not (IsMount $e) -and (Test-Path $e) } 10000) -and (Manifest $e) -eq $ref -and
                    -not (Test-Path (Sib $e).Vault) -and -not (Test-Path (DecTmp $e)) -and (DecJournals) -eq 0 -and -not (Folder $e)
            }
        }
        if (-not $script:proc.HasExited) { [void](Send '{"cmd":"quit"}'); [void]$script:proc.WaitForExit(20000) }
    }
    # Ssylka na papku isporchena: fayly otrezany ot dereva, rasshifrovka otkazyvaet, hranilishche celo
    $script:proc = StartCore
    $v = "$root\damaged\$ruName"; Encrypted $v
    [void](Send "{`"cmd`":`"lock`",`"path`":`"$(J $v)`"}")
    $links = @(Get-ChildItem -LiteralPath (Sib $v).Vault -Recurse -Force -Filter dir.c9r)
    $saved = @{}
    foreach ($l in $links) { $saved[$l.FullName] = [IO.File]::ReadAllText($l.FullName); [IO.File]::WriteAllText($l.FullName, '00000000-0000-0000-0000-000000000000-' + $saved.Count) }
    $r = Send (DecryptReq $v)
    Check 'damaged folder link: decrypt refused with vault_damaged, vault kept' {
        $r.error -eq 'vault_damaged' -and (Test-Path -LiteralPath "$((Sib $v).Vault)\masterkey.cryptomator") -and
            -not (Test-Path (DecTmp $v)) -and (DecJournals) -eq 0
    }
    foreach ($k in $saved.Keys) { [IO.File]::WriteAllText($k, $saved[$k]) }
    Check 'after the link is repaired the folder opens with the same content' {
        $u = Send "{`"cmd`":`"unlock`",`"path`":`"$(J $v)`",`"password`":`"$pw`"}"
        $u.ok -and (Manifest $v) -eq $ref
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
