# Privacy

Zamok has no telemetry, no analytics, no accounts and no servers. The programs make no network calls.

This was checked in the code of version 0.1.0-alpha: `core/Cargo.lock` has no HTTP, TLS, socket or async network crates, and the Rust and C# sources of `zamok-core`, `Zamok.exe` and `zamok-shell.dll` contain no sockets, HTTP clients or web addresses. Only the build script `installer/build-msi.ps1` downloads a file: the WinFsp installer from its GitHub release, and only on the machine that builds Zamok. There is no update check.

Stored on your computer, in `%LOCALAPPDATA%\Zamok`:
- the list of encrypted folders with their paths and settings;
- journals of encrypt and decrypt jobs, used to recover after a crash;
- the program log `core.log`, which by project rule contains no file names, paths or file contents.

Stored next to each folder: the encrypted data in the hidden `.Name.zamok` folder. Zamok stores no passwords.

Windows Hello is handled by Windows. If you turn it on for a folder, the folder's master key is wrapped with a key from a Windows Hello signature, and the wrapped key stays on this computer.

The recovery key is shown once in the wizard and on request after you enter the password. Where you save it is up to you.

What Windows and other programs write outside Zamok is listed under "Traces in Windows" in the [README](README.md).

The website on GitHub Pages is static, with no scripts, web fonts, cookies or counters from Zamok. GitHub hosts it and has its own privacy statement.

# Конфиденциальность

В Zamok нет телеметрии, аналитики, аккаунтов и серверов. Программы не обращаются к сети.

Это проверено по коду версии 0.1.0-alpha: в `core/Cargo.lock` нет библиотек HTTP, TLS, сокетов и сетевого async, а в исходниках Rust и C# программ `zamok-core`, `Zamok.exe` и `zamok-shell.dll` нет сокетов, HTTP-клиентов и веб-адресов. Файл скачивает только сценарий сборки `installer/build-msi.ps1`: установщик WinFsp с его страницы релизов на GitHub, и только на компьютере, где собирают Zamok. Проверки обновлений нет.

На вашем компьютере, в `%LOCALAPPDATA%\Zamok`, хранятся:
- список зашифрованных папок с путями и настройками;
- журналы работ шифрования и расшифровки, они нужны для восстановления после сбоя;
- журнал программы `core.log`, в который по правилам проекта не пишутся имена файлов, пути и содержимое.

Рядом с каждой папкой лежат зашифрованные данные в скрытой папке `.Имя.zamok`. Пароли Zamok не хранит.

Windows Hello обслуживает сама Windows. Если включить вход для папки, мастер-ключ папки заворачивается ключом из подписи Windows Hello, а завёрнутый ключ остаётся на этом компьютере.

Ключ восстановления показывается в мастере и по запросу после ввода пароля. Где его хранить, решаете вы.

Что Windows и другие программы пишут вне Zamok, перечислено в разделе «Следы в Windows» в [README.ru.md](README.ru.md).

Сайт на GitHub Pages статический, без скриптов, веб-шрифтов, cookie и счётчиков со стороны Zamok. Его размещает GitHub, у которого своя политика конфиденциальности.
