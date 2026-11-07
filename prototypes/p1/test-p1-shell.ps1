# P1: Проводник (Shell COM, SHFileOperation) и Word на подключённой папке.
# Запуск: powershell -NoProfile -ExecutionPolicy Bypass -File test-p1-shell.ps1
$ErrorActionPreference = 'Stop'
$root = 'C:\dev\zamok\tests\tmp\p1'
$src = Join-Path $root 'src'
$mnt = Join-Path $root 'mnt'
$out = Join-Path $root 'outside'
$exe = Join-Path $PSScriptRoot 'ntptfs\target\release\p1-ntptfs.exe'
$results = New-Object System.Collections.Generic.List[object]
function Check([string]$name, [scriptblock]$body) {
    try { $r = & $body; if ($r -eq $false) { throw 'returned false' }
        $results.Add([pscustomobject]@{ Test = $name; Ok = $true; Note = "$r" }) }
    catch { $results.Add([pscustomobject]@{ Test = $name; Ok = $false; Note = $_.Exception.Message }) }
}

Add-Type -Namespace P1 -Name Op -MemberDefinition @'
[StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
public struct SHFILEOPSTRUCT { public IntPtr hwnd; public uint wFunc; public string pFrom; public string pTo;
  public ushort fFlags; public bool fAnyOperationsAborted; public IntPtr hNameMappings; public string lpszProgressTitle; }
[DllImport("shell32.dll", CharSet = CharSet.Unicode)] public static extern int SHFileOperation(ref SHFILEOPSTRUCT op);
public static int Run(uint func, string from, string to, ushort flags) {
  var op = new SHFILEOPSTRUCT(); op.wFunc = func; op.pFrom = from + "\0\0"; op.pTo = to == null ? null : to + "\0\0"; op.fFlags = flags;
  return SHFileOperation(ref op);
}
'@
# FOF_SILENT 0x4, FOF_NOCONFIRMATION 0x10, FOF_ALLOWUNDO 0x40, FOF_NOERRORUI 0x400, FOF_NOCONFIRMMKDIR 0x200
$quiet = 0x4 -bor 0x10 -bor 0x400 -bor 0x200

Remove-Item $src, $out -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $src, $out | Out-Null
1..20 | ForEach-Object { Set-Content (Join-Path $out "doc$_.txt") "content $_" }
$proc = Start-Process $exe -ArgumentList "-p `"$src`" -m `"$mnt`"" -PassThru -WindowStyle Hidden
$deadline = (Get-Date).AddSeconds(10)
while (-not (Test-Path $mnt)) { if ((Get-Date) -gt $deadline) { throw 'no mount' }; Start-Sleep -Milliseconds 200 }
Start-Sleep -Milliseconds 500

Check 'Explorer copy engine: copy folder into mount' {
    $rc = [P1.Op]::Run(2, $out, $mnt, $quiet)   # FO_COPY
    if ($rc -ne 0) { return "rc=$rc" -eq 'rc=0' }
    (Get-ChildItem (Join-Path $mnt 'outside')).Count -eq 20
}
Check 'Explorer copy engine: move file out of mount' {
    $rc = [P1.Op]::Run(1, (Join-Path $mnt 'outside\doc1.txt'), $root, $quiet)   # FO_MOVE
    ($rc -eq 0) -and (Test-Path (Join-Path $root 'doc1.txt'))
}
Check 'Explorer copy engine: rename' {
    $rc = [P1.Op]::Run(4, (Join-Path $mnt 'outside\doc2.txt'), (Join-Path $mnt 'outside\renamed.txt'), $quiet)  # FO_RENAME
    ($rc -eq 0) -and (Test-Path (Join-Path $mnt 'outside\renamed.txt'))
}
Check 'Delete with "allow undo": goes to Recycle Bin?' {
    $f = Join-Path $mnt 'outside\doc3.txt'
    $rc = [P1.Op]::Run(3, $f, $null, ($quiet -bor 0x40))   # FO_DELETE + FOF_ALLOWUNDO
    $gone = -not (Test-Path $f)
    $shell = New-Object -ComObject Shell.Application
    $bin = $shell.Namespace(10).Items() | Where-Object { $_.Name -like 'doc3*' }
    "rc=$rc deleted=$gone inRecycleBin=$([bool]$bin)"
}
Check 'Shell namespace lists items' {
    $shell = New-Object -ComObject Shell.Application
    $n = $shell.Namespace((Join-Path $mnt 'outside')).Items().Count
    "items=$n"
}
Check 'Office-style safe save: temp file, swap by rename, delete backup' {
    $d = Join-Path $mnt 'office'; New-Item -ItemType Directory $d | Out-Null
    $doc = Join-Path $d 'report.docx'; Set-Content $doc 'v1'
    $h = [IO.File]::Open($doc, 'Open', 'Read', 'ReadWrite, Delete')   # Word держит документ открытым
    try {
        Set-Content (Join-Path $d '~WRL0001.tmp') 'v2'
        Rename-Item $doc 'report.wbk'
        Rename-Item (Join-Path $d '~WRL0001.tmp') 'report.docx'
    } finally { $h.Close() }
    Remove-Item (Join-Path $d 'report.wbk')
    ((Get-Content $doc) -eq 'v2') -and (@(Get-ChildItem $d).Count -eq 1)
}
# Word через COM в скрытом режиме зависает на невидимом окне, поэтому только по запросу: $env:P1_WORD = '1'
if ($env:P1_WORD -eq '1') { Check 'Word: save .docx into mount, reopen, read text' {
    $w = New-Object -ComObject Word.Application
    try {
        $w.Visible = $false; $w.DisplayAlerts = 0
        $d = $w.Documents.Add(); $d.Content.Text = 'Zamok P1 test'
        $path = Join-Path $mnt 'word-test.docx'
        $d.SaveAs2($path, 16); $d.Content.Text = 'Zamok P1 test v2'; $d.Save(); $d.Close()
        $d = $w.Documents.Open($path); $t = $d.Content.Text.Trim(); $d.Close()
        if ($t -ne 'Zamok P1 test v2') { return $false }
        $left = @(Get-ChildItem $mnt -Force | Where-Object { $_.Name -like '~*' }).Count
        "text ok, temp files left=$left"
    } finally { $w.Quit(); [Runtime.InteropServices.Marshal]::ReleaseComObject($w) | Out-Null }
} }

Stop-Process $proc.Id; Start-Sleep -Seconds 2
$results | Format-Table -AutoSize -Wrap | Out-String -Width 200
"passed: $(@($results | Where-Object Ok).Count) / $($results.Count)"
