# Как помочь проекту

Issues and pull requests in English are welcome too.

Zamok работает с чужими файлами, поэтому главное правило простое: данные пользователя важнее любой новой возможности.

## Сообщить об ошибке

Откройте задачу по форме. Если нашли способ прочитать запертую папку без пароля или потерять данные, сообщите приватно: [SECURITY.md](SECURITY.md).

## Что нужно для сборки

Windows 10 или 11 и диск NTFS: на exFAT подключение папок не работает.

- Visual Studio с компонентами C++ и Windows SDK
- Rust (`rustup`, stable)
- .NET 10 SDK
- WinFsp со всеми компонентами, включая Developer

Подробнее в [docs/STACK.md](docs/STACK.md).

## Сборка и установка для разработки

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\install-dev.ps1
```

Программа ставится в `%LOCALAPPDATA%\Programs\Zamok`, права администратора не нужны. Удаление: `scripts\uninstall-dev.ps1`. Общий установщик собирает `installer\build-msi.ps1`.

## Проверки

| Команда | Что проверяет |
|---|---|
| `cargo test` в папке `core` | криптоядро, файловая система, фоновая программа |
| `dotnet test ui\Zamok.Tests` | клиент канала, нужен собранный `cargo build --release -p zamok-core` |
| `tests\core\test-core.ps1` | фоновая программа на настоящей системе |
| `tests\core\test-encrypt.ps1`, `test-decrypt.ps1` | шифрование и расшифровка, авария на каждом шаге |
| `tests\fs\test-fs.ps1` | операции с файлами, результат читает Cryptomator |
| `tests\ui\test-ui.ps1` | окна через UI Automation |
| `tests\install\test-install.ps1`, `test-msi.ps1`, `test-bundle.ps1` | установка, только если Zamok не установлен |
| `tests\install\test-copyhook.ps1` | расширение Проводника, нужен установленный Zamok и диск D: с NTFS |

Скрипты запускаются так: `powershell -NoProfile -ExecutionPolicy Bypass -File <путь>`, для `test-copyhook.ps1` ещё с `-Sta`. Тесты установки работают с настоящим реестром пользователя.

После изменения окон посмотрите на них глазами: тесты не видят обрезанный текст.

## Правила кода

- Своей криптографии не писать. Только RustCrypto и формат Cryptomator по спецификации.
- В журнал (`tracing`) не писать имена файлов, пути и содержимое.
- Ни одна операция с файлами пользователя не удаляет оригинал раньше, чем проверена копия.
- `zamok-shell.dll` работает внутри Проводника: никаких обращений к диску, сети и долгих операций, никаких `unwrap` и индексации.
- Скрипты PowerShell держать в ASCII.

## Ветки и коммиты

Работа идёт в отдельных ветках и попадает в `main` через запрос на слияние, когда проверки GitHub Actions зелёные. Коммиты по-английски, коротко и в повелительном наклонении: `Add scrypt key derivation`. Документация в репозитории по-русски.

Выполненный шаг отмечайте в [ROADMAP.md](ROADMAP.md) в том же коммите, новое решение добавляйте строкой в таблицу в [docs/DESIGN.md](docs/DESIGN.md), раздел 11.

Отправляя код, вы соглашаетесь распространять его под лицензией проекта (GPLv3).
