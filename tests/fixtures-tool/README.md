# Эталонные хранилища

`tests/fixtures/vault1` создано библиотеками Cryptomator 1.19.6 (cryptofs 2.11.1, cryptolib 2.4.0) программой `MakeVault.java` из файлов `tests/fixtures-src/vault1`. Пароль тестовый: `zamok-test-1`. Рядом лежит сырой мастер-ключ `vault1-masterkey-hex.txt`, он нужен только тестам.

`ReadVault.java` делает обратное: читает хранилище библиотеками Cryptomator. Им проверено, что хранилище, созданное Zamok, открывается в Cryptomator (2026-11-08: 7 файлов и 3 папки, имена и содержимое совпали).

`org/cryptomator/ui/recoverykey/MakeRecoveryKey.java` создаёт `vault1-recovery.txt` кодом Cryptomator (`RecoveryKeyFactory`), `ResetWithRecoveryKey.java` проверяет ключ, созданный Zamok, и задаёт по нему новый пароль. Они лежат в пакете Cryptomator, потому что его `WordEncoder` не публичный.

Как пересоздать (нужен JDK 26: приложение Cryptomator 1.19.6 собрано под Java 26, библиотеки берутся из установленного Cryptomator):

```powershell
$j = 'C:\dev\tools\jdk-26.0.2.1+1\bin'
$jars = @(Get-ChildItem 'C:\Program Files\Cryptomator\app\mods' -Filter *.jar) + @(Get-ChildItem 'C:\Program Files\Cryptomator\app' -Filter *.jar)
$cp = ($jars | ForEach-Object FullName) -join ';'
& "$j\javac.exe" -encoding UTF-8 -cp $cp -d out MakeVault.java ReadVault.java
& "$j\java.exe" -Dfile.encoding=UTF-8 -Dsun.jnu.encoding=UTF-8 -cp "out;$cp" MakeVault ..\fixtures\vault1 ..\fixtures-src\vault1 zamok-test-1
```

После пересоздания тест `fixture_vault1` берёт новый мастер-ключ из `vault1-masterkey-hex.txt` сам.
