# Proverka obshchego ustanovshchika na chistoy Windows (Windows Sandbox), gde WinFsp eshche net:
# ustanovka, nastoyashchaya papka Zamok cherez WinFsp, otkrytie parolem, udalenie.
# Snachala: installer\build-msi.ps1 i cargo build --release --example create_vault
# Zapusk: powershell -NoProfile -ExecutionPolicy Bypass -File tests\install\test-sandbox.ps1
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path "$PSScriptRoot/../..").Path
$box = "$repo\tests\tmp\sandbox"
$pw = 'sandbox-test'
if (-not (Test-Path C:\Windows\System32\WindowsSandbox.exe)) { throw 'Windows Sandbox is not enabled' }
if (Get-Process WindowsSandbox* -ErrorAction SilentlyContinue) { throw 'Windows Sandbox is already running' }
$setup = Get-ChildItem "$repo\installer\out" -Filter 'Zamok-Setup-*.exe' | Sort-Object LastWriteTime | Select-Object -Last 1
if (-not $setup) { throw 'build first: installer\build-msi.ps1' }
Remove-Item $box -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $box | Out-Null
Copy-Item $setup.FullName "$box\setup.exe"
Push-Location "$repo\core"
& "$env:USERPROFILE\.cargo\bin\cargo.exe" run -q --release --example create_vault -- "$repo\tests\fixtures-src\vault1" "$box\vault" $pw | Out-Null
Pop-Location

@'
$ErrorActionPreference = 'Continue'
$here = 'C:\Users\WDAGUtilityAccount\Desktop\sb'
$out = "$here\result.txt"
function Say([string]$s) { Add-Content -LiteralPath $out -Value $s -Encoding UTF8 }
function Check([string]$name, [scriptblock]$b) { $v = $false; try { $v = & $b } catch { Say "ERR $name : $($_.Exception.Message)" }; if ($v) { Say "PASS $name" } else { Say "FAIL $name" } }
function WaitFor([scriptblock]$c, [int]$ms) { $d = (Get-Date).AddMilliseconds($ms); while (-not (& $c)) { if ((Get-Date) -gt $d) { return $false }; Start-Sleep -Milliseconds 200 }; $true }
function WinFsp { (Get-ItemProperty 'HKLM:\SOFTWARE\WOW6432Node\WinFsp' -ErrorAction SilentlyContinue).InstallDir }
$pipe = "zamok-$([Security.Principal.WindowsIdentity]::GetCurrent().User.Value)"
function Send([string]$j) { $c = New-Object IO.Pipes.NamedPipeClientStream '.', $pipe, 'InOut'; $c.Connect(5000); $e = New-Object Text.UTF8Encoding $false; $w = New-Object IO.StreamWriter $c, $e; $w.AutoFlush = $true; $r = New-Object IO.StreamReader $c, $e; $w.WriteLine($j); $l = $r.ReadLine(); $c.Dispose(); $l | ConvertFrom-Json }
function J([string]$s) { $s.Replace('\', '\\') }
Say "START $([Environment]::OSVersion.VersionString)"
Check 'winfsp is absent before setup' { -not (WinFsp) }
Copy-Item "$here\setup.exe" "$env:TEMP\setup.exe"
$p = Start-Process "$env:TEMP\setup.exe" -ArgumentList '/quiet', '/norestart', '/log', "$here\install.log" -PassThru
[void]$p.WaitForExit(300000)
Say "setup exit: $($p.ExitCode)"
Check 'setup succeeded' { $p.ExitCode -eq 0 }
Check 'winfsp is installed' { (WinFsp) -and (Test-Path "$(WinFsp)bin\winfsp-x64.dll") }
Check 'zamok files are in place' { Test-Path "$env:LOCALAPPDATA\Programs\Zamok\zamok-core.exe" }
Check 'core started' { WaitFor { Test-Path "\\.\pipe\$pipe" } 30000 }
if (-not (Test-Path "\\.\pipe\$pipe")) {
    Say "procs: $(@(Get-Process zamok-core -ErrorAction SilentlyContinue).Count)"
    if (Test-Path "$env:LOCALAPPDATA\Zamok\core.log") { Get-Content "$env:LOCALAPPDATA\Zamok\core.log" -Tail 12 | ForEach-Object { Say "log: $_" } } else { Say 'no core.log' }
    Say "run key: $((Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run').Zamok)"
    $d = Start-Process "$env:LOCALAPPDATA\Programs\Zamok\zamok-core.exe" -PassThru -RedirectStandardError "$here\core-err.txt"
    [void]$d.WaitForExit(8000)
    Say "manual core: exited=$($d.HasExited) code=$(if ($d.HasExited) { '0x{0:X8}' -f $d.ExitCode })"
    if (Test-Path "$here\core-err.txt") { Get-Content "$here\core-err.txt" | ForEach-Object { Say "stderr: $_" } }
    Say "pipe after manual start: $(Test-Path "\\.\pipe\$pipe")"
    Select-String -Path "$here\install.log" -Pattern 'Applied execute package|Error 0x|e000' | Select-Object -Last 6 | ForEach-Object { Say "burn: $($_.Line)" }
    Say "dlls: $((Get-ChildItem C:\Windows\System32\vcruntime140*.dll -ErrorAction SilentlyContinue).Name -join ',')"
}
Check 'the window program runs (it builds the icons)' {
    $ic = "$env:TEMP\ic"; New-Item -ItemType Directory -Force $ic | Out-Null
    $z = Start-Process "$env:LOCALAPPDATA\Programs\Zamok\Zamok.exe" -ArgumentList '--make-icons', "`"$ic`"" -PassThru
    [void]$z.WaitForExit(60000)
    Say "Zamok.exe exit: $($z.ExitCode)"
    Test-Path "$ic\folder-locked.ico"
}
$root = 'C:\Users\WDAGUtilityAccount\t'; New-Item -ItemType Directory -Force $root | Out-Null
Copy-Item "$here\vault" "$root\.f.zamok" -Recurse
$mnt = "$root\f"
$a = Send "{`"cmd`":`"add`",`"path`":`"$(J $mnt)`",`"vault`":`"$(J "$root\.f.zamok")`"}"
Say "add: $($a.ok) $($a.error)"
Check 'folder mounts on the first try, the driver was started by core' { $a.ok }
if (-not $a.ok) {
    Get-Content "$env:LOCALAPPDATA\Zamok\core.log" -Tail 6 | ForEach-Object { Say "log: $_" }
    & sc.exe query WinFsp 2>&1 | ForEach-Object { Say "sc: $_" }
    & sc.exe query WinFsp.Launcher 2>&1 | Select-String 'STATE' | ForEach-Object { Say "launcher: $_" }
    Say "bin: $((Get-ChildItem "$(WinFsp)bin" -Name) -join ',')"
    $m = "$(WinFsp)bin\memfs-x64.exe"
    if (Test-Path $m) {
        $t = Start-Process $m -ArgumentList '-t','0','-n','1024','-s','1048576','-m','X:' -PassThru -RedirectStandardError "$here\memfs.txt"
        Start-Sleep -Seconds 4
        Say "memfs X: exists: $(Test-Path X:\), exited: $($t.HasExited)"
        if (-not $t.HasExited) { $t.Kill() }
        Get-Content "$here\memfs.txt" -ErrorAction SilentlyContinue | Select-Object -Last 3 | ForEach-Object { Say "memfs: $_" }
    }
    $names = @(Get-ChildItem 'HKLM:\SYSTEM\CurrentControlSet\Services' | Where-Object { $_.PSChildName -like 'WinFsp*' } | ForEach-Object PSChildName)
    Say "services: $($names -join ', ')"
    foreach ($n in $names) { if ($n -ne 'WinFsp.Launcher') {
        & sc.exe qc "$n" 2>&1 | Select-String 'TYPE|START_TYPE|BINARY' | ForEach-Object { Say "qc: $_" }
        & sc.exe start "$n" 2>&1 | ForEach-Object { Say "start: $_" }
    } }
    Say "sys signature: $((Get-AuthenticodeSignature "$(WinFsp)bin\winfsp-x64.sys").Status)"
    Get-WinEvent -FilterHashtable @{LogName='System'; StartTime=(Get-Date).AddMinutes(-5)} -MaxEvents 40 -ErrorAction SilentlyContinue | Where-Object { $_.LevelDisplayName -in 'Error','Warning' -or $_.Message -match 'WinFsp' } | Select-Object -First 5 | ForEach-Object { Say "event $($_.Id) $($_.ProviderName): $(($_.Message -split "`n")[0])" }
    Say "vault files: $(@(Get-ChildItem "$root\.f.zamok" -Force).Name -join ',')"
    Say "fs of C: $((Get-Volume C -ErrorAction SilentlyContinue).FileSystem)"
    $a = Send "{`"cmd`":`"add`",`"path`":`"$(J $mnt)`",`"vault`":`"$(J "$root\.f.zamok")`"}"
    Say "add after driver start: $($a.ok) $($a.error)"
    if (-not $a.ok) { $st = Send '{"cmd":"status"}'; Say "status: $($st.folders.Count)" }
    Get-Content "$env:LOCALAPPDATA\Zamok\core.log" -Tail 2 | ForEach-Object { Say "log2: $_" }
    Say "sxs files: $((Get-ChildItem "$(WinFsp)" -Recurse -Filter 'winfsp.sxs' -ErrorAction SilentlyContinue).FullName -join ',')"
}
Check 'folder is mounted through winfsp' { $a.ok -and (WaitFor { Test-Path -LiteralPath $mnt } 15000) }
$u = Send "{`"cmd`":`"unlock`",`"path`":`"$(J $mnt)`",`"password`":`"PASSWORD`"}"
Say "unlock: $($u.ok) $($u.error)"
Check 'password opens the folder, files are readable' { $u.ok -and @(Get-ChildItem -LiteralPath $mnt -Recurse -File -Force).Count -ge 7 }
Check 'a file can be written' { Set-Content -LiteralPath "$mnt\new.txt" 'hello'; (Get-Content -LiteralPath "$mnt\new.txt") -eq 'hello' }
$l = Send "{`"cmd`":`"lock`",`"path`":`"$(J $mnt)`"}"
Check 'lock hides the files' { $l.ok -and (WaitFor { -not (Test-Path -LiteralPath "$mnt\new.txt") } 8000) }
$p = Start-Process "$env:TEMP\setup.exe" -ArgumentList '/uninstall', '/quiet', '/norestart', '/log', "$here\uninstall.log" -PassThru
[void]$p.WaitForExit(180000)
Say "uninstall exit: $($p.ExitCode)"
Check 'uninstall succeeded and removed zamok' { $p.ExitCode -eq 0 -and -not (Test-Path "$env:LOCALAPPDATA\Programs\Zamok\zamok-core.exe") }
Check 'winfsp stays' { WinFsp }
Check 'encrypted data stays' { Test-Path "$root\.f.zamok\masterkey.cryptomator" }
Say 'DONE'
'@.Replace('PASSWORD', $pw) | Set-Content "$box\guest.ps1" -Encoding ASCII

@"
<Configuration>
  <Networking>Disable</Networking>
  <MappedFolders><MappedFolder><HostFolder>$box</HostFolder><SandboxFolder>C:\Users\WDAGUtilityAccount\Desktop\sb</SandboxFolder><ReadOnly>false</ReadOnly></MappedFolder></MappedFolders>
  <LogonCommand><Command>powershell -NoProfile -ExecutionPolicy Bypass -File C:\Users\WDAGUtilityAccount\Desktop\sb\guest.ps1</Command></LogonCommand>
</Configuration>
"@ | Set-Content "$box\run.wsb" -Encoding UTF8

Start-Process "$box\run.wsb"
$deadline = (Get-Date).AddMinutes(8)
while ((Get-Date) -lt $deadline) {
    if ((Test-Path "$box\result.txt") -and (Select-String -Path "$box\result.txt" -Pattern '^DONE' -Quiet -ErrorAction SilentlyContinue)) { break }
    Start-Sleep -Seconds 3
}
Get-Process WindowsSandbox* -ErrorAction SilentlyContinue | Stop-Process -Force
if (-not (Test-Path "$box\result.txt")) { throw 'the sandbox wrote no result' }
Get-Content "$box\result.txt" -Encoding UTF8
"passed: $(@(Select-String -Path "$box\result.txt" -Pattern '^PASS').Count), failed: $(@(Select-String -Path "$box\result.txt" -Pattern '^FAIL').Count)"
