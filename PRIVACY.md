# Privacy

Zamok has no telemetry, no analytics, no accounts and no servers. The programs go online only when you press "Check for updates" in the settings window, and never on their own.

What the update check sends. On the button press the settings window (`Zamok.exe`) makes one HTTPS GET request to GitHub, `https://api.github.com/repos/olerast67/zamok/releases/latest`. If GitHub has only prereleases, a second request goes to `https://api.github.com/repos/olerast67/zamok/releases?per_page=5`. GitHub sees your IP address and the header `User-Agent: Zamok/<version>`, for example `Zamok/0.1.0-alpha`. No cookies, no identifiers, no folder names, paths or settings are sent. If Windows has a proxy set, the request goes through it. GitHub handles the request under its own privacy statement. Zamok downloads and installs nothing: if a newer version exists, the window shows its number and a button that opens the release page in your browser. There is no automatic check: no timer, no check at start, nothing in the background.

This was checked in the code of version 0.1.0-alpha: `core/Cargo.lock` has no HTTP, TLS, socket or async network crates, and the Rust sources of `zamok-core` and `zamok-shell.dll` contain no sockets, HTTP clients or web addresses. In the C# sources of `Zamok.exe` the only HTTP client is in `ui/Zamok.Client/UpdateCheck.cs`, and only the button handler in `ui/Zamok.UI/SettingsWindow.xaml.cs` calls it. `tests/ui/test-ui.ps1` checks that opening the settings window sends no request. Only the build script `installer/build-msi.ps1` downloads a file: the WinFsp installer from its GitHub release, and only on the machine that builds Zamok.

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

В Zamok нет телеметрии, аналитики, аккаунтов и серверов. К сети программа обращается только по кнопке «Проверить обновления» в окне настроек и никогда сама.

Что отправляет проверка обновлений. По нажатию кнопки окно настроек (`Zamok.exe`) делает один запрос HTTPS GET к GitHub: `https://api.github.com/repos/olerast67/zamok/releases/latest`. Если на GitHub есть только предварительные версии, второй запрос идёт к `https://api.github.com/repos/olerast67/zamok/releases?per_page=5`. GitHub видит ваш IP-адрес и заголовок `User-Agent: Zamok/<версия>`, например `Zamok/0.1.0-alpha`. Cookie, идентификаторы, имена папок, пути и настройки не отправляются. Если в Windows задан прокси-сервер, запрос идёт через него. Запрос обрабатывает GitHub по своей политике конфиденциальности. Zamok ничего не скачивает и не ставит: если есть версия новее, окно показывает её номер и кнопку, которая открывает страницу выпуска в браузере. Автоматической проверки нет: ни по таймеру, ни при запуске, ни в фоне.

Это проверено по коду версии 0.1.0-alpha: в `core/Cargo.lock` нет библиотек HTTP, TLS, сокетов и сетевого async, а в исходниках Rust программ `zamok-core` и `zamok-shell.dll` нет сокетов, HTTP-клиентов и веб-адресов. В исходниках C# программы `Zamok.exe` единственный HTTP-клиент находится в `ui/Zamok.Client/UpdateCheck.cs`, и вызывает его только обработчик кнопки в `ui/Zamok.UI/SettingsWindow.xaml.cs`. `tests/ui/test-ui.ps1` проверяет, что открытие окна настроек не отправляет ни одного запроса. Файл скачивает только сценарий сборки `installer/build-msi.ps1`: установщик WinFsp с его страницы релизов на GitHub, и только на компьютере, где собирают Zamok.

На вашем компьютере, в `%LOCALAPPDATA%\Zamok`, хранятся:
- список зашифрованных папок с путями и настройками;
- журналы работ шифрования и расшифровки, они нужны для восстановления после сбоя;
- журнал программы `core.log`, в который по правилам проекта не пишутся имена файлов, пути и содержимое.

Рядом с каждой папкой лежат зашифрованные данные в скрытой папке `.Имя.zamok`. Пароли Zamok не хранит.

Windows Hello обслуживает сама Windows. Если включить вход для папки, мастер-ключ папки заворачивается ключом из подписи Windows Hello, а завёрнутый ключ остаётся на этом компьютере.

Ключ восстановления показывается в мастере и по запросу после ввода пароля. Где его хранить, решаете вы.

Что Windows и другие программы пишут вне Zamok, перечислено в разделе «Следы в Windows» в [README.ru.md](README.ru.md).

Сайт на GitHub Pages статический, без скриптов, веб-шрифтов, cookie и счётчиков со стороны Zamok. Его размещает GitHub, у которого своя политика конфиденциальности.
