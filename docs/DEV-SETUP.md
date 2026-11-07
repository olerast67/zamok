# Среда разработки

Что стоит на компьютере автора и как это проверено. Список нужного с объяснением, зачем каждая вещь: [STACK.md](STACK.md), раздел 6.

## Установлено 2026-11-07

| Что | Версия | Откуда | Проверка |
|---|---|---|---|
| Visual Studio Community 2022 и 2026 с C++ и Windows SDK 10.0.22621, 10.0.26100 | уже стояли | | компоновщик MSVC собирает Rust |
| Rust (`rustup`, stable, `x86_64-pc-windows-msvc`) | rustc 1.99.1 | уже стоял, обновлён `rustup update` | пробная программа собирается и запускается |
| .NET SDK | 10.0.103 и 9.0.311 | уже стояли | пробная программа на `net10.0` собирается и запускается |
| WinFsp, все компоненты, включая Developer | 2.1.25156 | github.com/winfsp/winfsp, релиз v2.1 | подпись MSI: NAVIMATICS LLC, действительна; драйвер `winfsp-x64.sys` подписан Microsoft Windows Hardware Compatibility Publisher |
| Cryptomator | 1.19.6 | github.com/cryptomator/cryptomator, релиз 1.19.6 | подпись MSI: Skymatic GmbH, действительна |
| Process Monitor, Handle | с сайта Sysinternals | download.sysinternals.com | подпись Microsoft Corporation, действительна; лежат в `%LOCALAPPDATA%\Programs\Sysinternals`, путь добавлен в PATH |

Все скачанные установщики перед запуском проверены Microsoft Defender: угроз не найдено.

На Windows 10 LTSC нет `winget`, поэтому установщики скачаны вручную с официальных страниц релизов и проверены по цифровой подписи.

## Как проверить подпись файла самому

В PowerShell:

```powershell
Get-AuthenticodeSignature .\файл.msi | Format-List Status, SignerCertificate
```

`Status` должен быть `Valid`, а в `SignerCertificate` должна стоять компания, которая делает программу.

## Ещё не сделано

- Виртуальная машина с Windows 11 в Hyper-V. Включение Hyper-V требует прав администратора и перезагрузки, это делает автор:
  1. «Включение или отключение компонентов Windows», отметить Hyper-V, перезагрузить.
  2. Скачать ISO пробной Windows 11 Enterprise на 90 дней с Microsoft Evaluation Center (для тестов лицензия не нужна) или обычный ISO с microsoft.com/software-download/windows11.
  3. Диспетчер Hyper-V, создать машину второго поколения с TPM и безопасной загрузкой, 4 ГБ памяти, 64 ГБ диска.
  4. После установки сделать снимок чистой системы.
