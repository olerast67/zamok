# Этап 3: проверка зашифрованной файловой системы Zamok на настоящей подключённой папке.
# Каждое изменение делается и в подключённой папке, и в обычной папке-зеркале на NTFS.
# В конце хранилище читается библиотеками Cryptomator и сравнивается с зеркалом.
# Запуск: powershell -NoProfile -ExecutionPolicy Bypass -File tests\fs\test-fs.ps1
# Скрипт в ASCII: русские имена собираются из кодов символов.
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path "$PSScriptRoot/../..").Path
$root = "$repo\tests\tmp\fs"
$vault = "$root\vault"; $mnt = "$root\mnt"; $mirror = "$root\mirror"
$mountExe = "$repo\core\target\release\zamok-mount.exe"
$pw = 'zamok-fs-test'
$results = New-Object System.Collections.Generic.List[object]
function Ru([int[]]$c) { -join ($c | ForEach-Object { [char]$_ }) }
$ruName = Ru 0x041E,0x0442,0x0447,0x0451,0x0442          
$ruDir = Ru 0x041A,0x043B,0x0438,0x0435,0x043D,0x0442,0x044B
$ruPapka = Ru 0x041F,0x0430,0x043F,0x043A,0x0430
function Check([string]$name, [scriptblock]$body) {
    try { $r = & $body; if ($r -eq $false) { throw 'returned false' }
        $results.Add([pscustomobject]@{ Test = $name; Ok = $true; Note = "$r" }) }
    catch { $results.Add([pscustomobject]@{ Test = $name; Ok = $false; Note = $_.Exception.Message }) }
}
function Both([scriptblock]$op) { & $op $mnt; & $op $mirror }

# Подготовка: хранилище из эталонных файлов, зеркало с теми же файлами
Remove-Item $root -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $root | Out-Null
Push-Location "$repo\core"
& "$env:USERPROFILE\.cargo\bin\cargo.exe" run -q --release --example create_vault -- "$repo\tests\fixtures-src\vault1" $vault $pw | Out-Null
Pop-Location
Copy-Item "$repo\tests\fixtures-src\vault1" $mirror -Recurse
New-Item -ItemType Directory -Force (Join-Path $mirror (Ru 0x041F,0x0443,0x0441,0x0442,0x0430,0x044F,0x0020,0x043F,0x0430,0x043F,0x043A,0x0430)) | Out-Null

$psi = New-Object Diagnostics.ProcessStartInfo $mountExe, "`"$vault`" `"$mnt`" TestBox"
$psi.UseShellExecute = $false; $psi.RedirectStandardInput = $true; $psi.RedirectStandardOutput = $true; $psi.CreateNoWindow = $true
$psi.EnvironmentVariables['ZAMOK_PASSWORD'] = $pw
$proc = [Diagnostics.Process]::Start($psi)
if ($proc.StandardOutput.ReadLine() -ne 'mounted') { throw 'mount failed' }
function Send([string]$cmd) { $proc.StandardInput.WriteLine($cmd); $proc.StandardInput.Flush(); $proc.StandardOutput.ReadLine() }

try {
Check 'mounted on the folder path, not admin' {
    $a = (Get-Item $mnt -Force).Attributes
    $admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    if ($admin -or -not ($a -band [IO.FileAttributes]::ReparsePoint)) { return $false }; "$a"
}
Check 'existing files from the vault are listed and decrypted' {
    $want = Get-ChildItem $mirror -Recurse -File | ForEach-Object { (Get-FileHash $_.FullName).Hash } | Sort-Object
    $got = Get-ChildItem $mnt -Recurse -File | ForEach-Object { (Get-FileHash $_.FullName).Hash } | Sort-Object
    if (Compare-Object $want $got) { return "different: $($got.Count) vs $($want.Count)" -eq '' }; "files=$($got.Count)"
}
Check 'case-insensitive lookup' { Test-Path (Join-Path $mnt '100K.BIN') }
Check 'create file with Cyrillic name, read back' {
    Both { param($d) Set-Content (Join-Path $d "$ruName.txt") 'data' -Encoding UTF8 }
    (Get-Content (Join-Path $mnt "$ruName.txt")) -eq 'data'
}
Check 'append' { Both { param($d) Add-Content (Join-Path $d "$ruName.txt") 'more' -Encoding UTF8 }; (Get-Content (Join-Path $mnt "$ruName.txt")).Count -eq 2 }
Check 'nested directories' { Both { param($d) New-Item -ItemType Directory -Force (Join-Path $d "$ruDir\2026\nov") | Out-Null }; Test-Path (Join-Path $mnt "$ruDir\2026\nov") }
Check 'rename file' { Both { param($d) Rename-Item (Join-Path $d "$ruName.txt") "$ruName-v2.txt" }; Test-Path (Join-Path $mnt "$ruName-v2.txt") }
Check 'move file into directory' { Both { param($d) Move-Item (Join-Path $d "$ruName-v2.txt") (Join-Path $d "$ruDir\2026") }; Test-Path (Join-Path $mnt "$ruDir\2026\$ruName-v2.txt") }
Check 'rename directory with content' { Both { param($d) Rename-Item (Join-Path $d "$ruDir\2026") '2026-old' }; Test-Path (Join-Path $mnt "$ruDir\2026-old\$ruName-v2.txt") }
Check 'long name (200 chars) and rename long -> short -> long' {
    $long = ('a' * 196) + '.txt'
    Both { param($d) Set-Content (Join-Path $d $long) 'long' }
    Both { param($d) Rename-Item (Join-Path $d $long) 'short.txt' }
    Both { param($d) Rename-Item (Join-Path $d 'short.txt') $long }
    (Get-Content (Join-Path $mnt $long)) -eq 'long'
}
Check 'deep path over 260 chars' {
    foreach ($d in $mnt, $mirror) {
        $p = '\\?\' + $d; 1..10 | ForEach-Object { $p = $p + '\folder_' + ('d' * 20) }
        [IO.Directory]::CreateDirectory($p) | Out-Null; [IO.File]::WriteAllText($p + '\deep.txt', 'deep')
    }
    'ok'
}
Check 'copy 50 MB file in, hashes match' {
    $big = "$root\big.bin"; $buf = New-Object byte[] (1MB); $rng = New-Object Random 3
    $fs = [IO.File]::Create($big); 1..50 | ForEach-Object { $rng.NextBytes($buf); $fs.Write($buf, 0, $buf.Length) }; $fs.Close()
    $sw = [Diagnostics.Stopwatch]::StartNew(); Copy-Item $big (Join-Path $mnt 'big.bin'); $sw.Stop()
    Copy-Item $big (Join-Path $mirror 'big.bin')
    if ((Get-FileHash $big).Hash -ne (Get-FileHash (Join-Path $mnt 'big.bin')).Hash) { return $false }
    "{0:N0} ms, {1:N0} MB/s" -f $sw.ElapsedMilliseconds, (50000 / [math]::Max(1, $sw.ElapsedMilliseconds))
}
Check 'random-access writes across chunk borders' {
    foreach ($d in $mnt, $mirror) {
        $f = [IO.File]::Open((Join-Path $d 'big.bin'), 'Open', 'ReadWrite')
        foreach ($off in 32767, 1048576, 40000000) { $f.Seek($off, 'Begin') | Out-Null; $f.Write([byte[]](1, 2, 3, 4, 5), 0, 5) }
        $f.SetLength(45000001); $f.Close()
    }
    (Get-FileHash (Join-Path $mnt 'big.bin')).Hash -eq (Get-FileHash (Join-Path $mirror 'big.bin')).Hash
}
Check 'timestamps' { $t = Get-Date '2020-01-02 03:04:05'; (Get-Item (Join-Path $mnt 'empty.bin')).LastWriteTime = $t; (Get-Item (Join-Path $mnt 'empty.bin')).LastWriteTime -eq $t }
Check 'hidden + readonly attributes' {
    $f = Join-Path $mnt '100k.bin'; (Get-Item $f).Attributes = 'Hidden, ReadOnly'
    $a = (Get-Item $f -Force).Attributes; (Get-Item $f -Force).Attributes = 'Normal'
    ($a -band [IO.FileAttributes]::Hidden) -and ($a -band [IO.FileAttributes]::ReadOnly)
}
Check 'Word-style safe save while file is open' {
    foreach ($d in $mnt, $mirror) {
        $doc = Join-Path $d 'report.docx'; Set-Content $doc 'v1'
        $h = [IO.File]::Open($doc, 'Open', 'Read', 'ReadWrite, Delete')
        try { Set-Content (Join-Path $d '~WRL0001.tmp') 'v2'; Rename-Item $doc 'report.wbk'; Rename-Item (Join-Path $d '~WRL0001.tmp') 'report.docx' } finally { $h.Close() }
        Remove-Item (Join-Path $d 'report.wbk')
    }
    (Get-Content (Join-Path $mnt 'report.docx')) -eq 'v2'
}
Check 'delete file and directory tree' { Both { param($d) Remove-Item (Join-Path $d "$ruDir") -Recurse -Force }; -not (Test-Path (Join-Path $mnt $ruDir)) }
Check 'deleting a non-empty directory directly is refused' {
    New-Item -ItemType Directory (Join-Path $mnt 'full') | Out-Null; Set-Content (Join-Path $mnt 'full\x.txt') 'x'
    $refused = $false; try { [IO.Directory]::Delete((Join-Path $mnt 'full'), $false) } catch { $refused = $true }
    Remove-Item (Join-Path $mnt 'full') -Recurse -Force
    $refused
}
Check '1000 small files: create and list' {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    foreach ($d in $mnt) { New-Item -ItemType Directory (Join-Path $d 'many') | Out-Null; 1..1000 | ForEach-Object { [IO.File]::WriteAllText((Join-Path $d "many\f$_.txt"), "$_") } }
    $n = @(Get-ChildItem (Join-Path $mnt 'many')).Count; $sw.Stop()
    New-Item -ItemType Directory (Join-Path $mirror 'many') | Out-Null; 1..1000 | ForEach-Object { [IO.File]::WriteAllText((Join-Path $mirror "many\f$_.txt"), "$_") }
    if ($n -ne 1000) { return "n=$n" -eq '' }; "{0:N0} ms" -f $sw.ElapsedMilliseconds
}
Check 'Explorer copy engine (Shell) copies a folder in' {
    $shell = New-Object -ComObject Shell.Application
    New-Item -ItemType Directory (Join-Path $mnt 'shellcopy') | Out-Null; New-Item -ItemType Directory (Join-Path $mirror 'shellcopy') | Out-Null; $shell.Namespace((Join-Path $mnt 'shellcopy')).CopyHere((Join-Path "$repo\tests\fixtures-src\vault1" $ruPapka), 4 + 16 + 512 + 1024)
    Copy-Item (Join-Path "$repo\tests\fixtures-src\vault1" $ruPapka) (Join-Path $mirror 'shellcopy') -Recurse
    Start-Sleep -Milliseconds 800; Test-Path (Join-Path (Join-Path $mnt 'shellcopy') $ruPapka)
}
Check 'ciphertext on disk has no plaintext names' {
    $plain = 'report.docx', 'big.bin', 'many', 'deep.txt', '100k.bin', 'shellcopy', 'f1.txt'
    $leak = Get-ChildItem $vault -Recurse | Where-Object { $plain -contains $_.Name }
    if ($leak) { return "leak: $($leak[0].Name)" -eq '' }; 'none'
}

# Запирание
$held = [IO.File]::Open((Join-Path $mnt 'report.docx'), 'Open', 'Read', 'ReadWrite, Delete')
$b = New-Object byte[] 2; $held.Read($b, 0, 2) | Out-Null
Check 'lock command' { (Send 'lock') -eq 'locked' }
Check 'locked: only the open item is visible' {
    $items = @(Get-ChildItem $mnt | ForEach-Object Name)
    if ($items.Count -ne 1 -or $items[0] -notlike '*TestBox.zamok') { return "items=[$($items -join ', ')]" -eq '' }; $items[0]
}
Check 'locked: desktop.ini is there (hidden) for the lock icon' { Test-Path (Join-Path $mnt 'desktop.ini') }
Check 'locked: files are not found' { -not (Test-Path (Join-Path $mnt 'report.docx')) }
Check 'locked: handle opened before lock fails' { try { $held.Seek(0, 'Begin') | Out-Null; $held.Read($b, 0, 2) | Out-Null; 'read worked' -eq '' } catch { 'denied' } }
Check 'locked: cannot create files' { try { Set-Content (Join-Path $mnt 'new.txt') 'x'; $false } catch { 'denied' } }
Check 'unlock with wrong password fails' { (Send 'unlock wrong') -like 'unlock failed*' }
Check 'unlock with right password' { (Send "unlock $pw") -eq 'unlocked' }
Check 'unlocked: files are back' { (Get-Content (Join-Path $mnt 'report.docx')) -eq 'v2' }
Check 'old handle stays dead after unlock' { try { $held.Seek(0, 'Begin') | Out-Null; $held.Read($b, 0, 2) | Out-Null; 'read worked' -eq '' } catch { 'denied' } }
try { $held.Close() } catch {}
}
finally {
    try { [void](Send 'quit') } catch {}
    if (-not $proc.WaitForExit(15000)) { $proc.Kill() }
}
Check 'unmounted, mount path gone' { -not (Test-Path $mnt) }

# Cryptomator читает результат
$j = 'C:\dev\tools\jdk-26.0.2.1+1\bin'; $t = "$repo\tests\fixtures-tool"
$cp = (Get-Content "$t\cp.txt").Substring(4).Trim([char]34)
& "$j\java.exe" "-Dfile.encoding=UTF-8" "-Dsun.jnu.encoding=UTF-8" -cp "$t/out;$cp" ReadVault $vault "$root\cryptomator-out" $pw 2>&1 | Where-Object { $_ -notmatch 'DEBUG' } | Out-Null
Check 'Cryptomator reads the vault: same files as the mirror' {
    $norm = { param($base) Get-ChildItem $base -Recurse -File | ForEach-Object { $_.FullName.Substring($base.Length).ToLower() + ' ' + (Get-FileHash -LiteralPath $_.FullName).Hash } | Sort-Object }
    $a = & $norm $mirror; $c = & $norm "$root\cryptomator-out"
    $d = Compare-Object $a $c
    if ($d) { return "diff: " + (($d | Select-Object -First 3 | ForEach-Object { "$($_.SideIndicator) $($_.InputObject.Substring(0, [math]::Min(60, $_.InputObject.Length)))" }) -join ' / ') -eq '' }
    "files=$($c.Count)"
}

$results | Format-Table -AutoSize -Wrap | Out-String -Width 220
"passed: $(@($results | Where-Object Ok).Count) / $($results.Count)"
