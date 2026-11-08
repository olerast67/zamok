# P2: ручная проверка двойного клика (1 минута).
#   1. powershell -ExecutionPolicy Bypass -File manual-p2.ps1 -Install
#   2. В открывшемся окне дважды кликнуть по папке Locked: должно появиться окно "Zamok P2",
#      а Проводник остаться на месте. Дважды кликнуть по Normal: папка открывается как обычно.
#      Проверить ещё: двойной клик по любой своей папке, дерево слева, рабочий стол.
#   3. powershell -ExecutionPolicy Bypass -File manual-p2.ps1 -Uninstall
# Всё пишется только в HKCU (для текущего пользователя) и убирается командой -Uninstall.
param([switch]$Install, [switch]$Uninstall)
$root = 'C:\dev\zamok\tests\tmp\p2-manual'
$locked = Join-Path $root 'Locked'; $normal = Join-Path $root 'Normal'
$shellKey = 'HKCU:\Software\Classes\Directory\shell'
$verb = Join-Path $shellKey 'ZamokP2Static'

if ($Install) {
    New-Item -ItemType Directory -Force $locked, $normal | Out-Null
    New-Item -Force "$verb\command" | Out-Null
    Set-ItemProperty $verb '(default)' 'Open with password (Zamok P2)'
    Set-ItemProperty $verb 'AppliesTo' ("System.ItemPathDisplay:=""$locked""")
    $cmd = 'powershell.exe -NoProfile -WindowStyle Hidden -Command "Add-Type -AssemblyName PresentationFramework; [Windows.MessageBox]::Show(''Here Zamok will ask for the password: %1'', ''Zamok P2'')"'
    Set-ItemProperty "$verb\command" '(default)' $cmd
    Set-ItemProperty $shellKey '(default)' 'ZamokP2Static none'
    Start-Process explorer.exe $root
    'Installed. Double-click Locked and Normal, then run with -Uninstall.'
}
if ($Uninstall) {
    reg delete 'HKCU\Software\Classes\Directory\shell' /ve /f 2>$null | Out-Null   # убрать значение целиком, а не оставить пустым
    Remove-Item $verb -Recurse -Force -ErrorAction SilentlyContinue
    'Removed. HKCU Directory\shell default: [' + (Get-ItemProperty $shellKey -ErrorAction SilentlyContinue).'(default)' + ']'
}
