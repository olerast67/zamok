# Выпуск версии

Установщик собирается сценарием `installeruild-msi.ps1` на компьютере разработчика или в GitHub Actions (`.github/workflows/release.yml`). `.github/workflows/ci.yml` только проверяет код.

## Что нужно

Windows, диск NTFS, инструменты из [CONTRIBUTING.md](CONTRIBUTING.md), локальный WiX (`dotnet tool restore`).

## Каждый выпуск

1. Обновить версию в `core/crates/*/Cargo.toml`, запись в [CHANGELOG.md](CHANGELOG.md), отметки в [ROADMAP.md](ROADMAP.md).
2. Запустить проверки из [CONTRIBUTING.md](CONTRIBUTING.md). `tests\ui\test-ui.ps1` и тесты установки показывают окна и работают с реестром пользователя, поэтому их запускают на отдельной системе или в Windows Sandbox (`tests\install\test-sandbox.ps1`).
3. Собрать установщик. Версия WiX состоит только из чисел, поэтому суффикс `-alpha` есть в теге, а в имени файла его нет:

   ```powershell
   powershell -NoProfile -ExecutionPolicy Bypass -File installer\build-msi.ps1 -Version 0.1.0
   ```

   Результат: `installer\out\Zamok-Setup-0.1.0.exe`, `Zamok-0.1.0.msi` и `SHA256SUMS.txt` с отпечатками обоих файлов. Сценарий скачивает WinFsp и принимает файл только с записанным SHA-256 и подписью издателя. Сборка останавливается, если в `zamok-core.exe`, `zamok-shell.dll`, `Zamok.exe` и библиотеках Zamok нашёлся путь сборочной машины (`scripts\check-paths.ps1`).
4. Проверить отпечатки: `sha256sum -c SHA256SUMS.txt` в Git Bash из папки `installer\out`. `SHA256SUMS.txt` выкладывается в релиз рядом с установщиком.

5. Поставить установщик в Windows Sandbox: зашифровать папку, запереть, открыть, открыть её в Cryptomator.
6. Создать релиз командами из [docs/PUBLISHING.md](docs/PUBLISHING.md), вписать SHA-256 в текст, приложить `SHA256SUMS.txt`.

Вместо шагов 3 и 4 можно отправить тег `v0.1.0-alpha`: `.github/workflows/release.yml` соберёт установщик на чистой машине GitHub, проверит `SHA256SUMS.txt` и создаст черновик релиза с тремя файлами. Публикует черновик автор после шага 5. Ручной запуск сценария (Run workflow, версия числами) только собирает файлы и кладёт их в артефакт.

## Воспроизводимость

Что сделано, чтобы одинаковый код давал одинаковые программы:

- Версия Rust записана в `core/rust-toolchain.toml`, rustup ставит её сам. Зависимости берутся строго из `Cargo.lock` (`cargo build --locked`).
- `scriptsuild-core-release.ps1` собирает `zamok-core.exe`, `zamok-shell.dll` и `zamok-recover.exe` с `--remap-path-prefix`: папка репозитория, реестр cargo и rustup в сообщениях об ошибках и отладочных сведениях заменяются на `/zamok`, `/cargo`, `/rustup`. Компоновщик получает `/Brepro` (без времени сборки в заголовке файла) и `/PDBALTPATH:%_PDB%` (в программе остаётся только имя файла PDB, а не полный путь).
- `SOURCE_DATE_EPOCH` равен времени последнего коммита. Rust и MSBuild его не читают, переменная нужна инструментам, которые ставят дату в файлы.
- `Zamok.exe` публикуется с `Deterministic=true` и `ContinuousIntegrationBuild=true`: без времени сборки, пути исходников заменены на `/_/`.

Чего нет: MSI и `Zamok-Setup.exe` от WiX побайтно не повторяются, в них дата сборки и случайный код пакета. Поэтому сравнивать нужно программы внутри, а не установщик.

Как проверить самому:

```powershell
# 1. Личных путей в программах нет (после сборки; без аргументов проверяет core	argetelease)
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\check-paths.ps1

# 2. Две сборки Rust в разных папках дают одинаковые файлы
git clone <repo> C:uild ; git clone <repo> C:uild   # один и тот же коммит
powershell -NoProfile -ExecutionPolicy Bypass -File C:uild\scriptsuild-core-release.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File C:uild\scriptsuild-core-release.ps1
Get-FileHash C:uild\core	argetelease\zamok-core.exe, C:uild\core	argetelease\zamok-core.exe

# 3. Программа из релиза совпадает со своей сборкой: распаковать MSI и сравнить отпечатки
msiexec /a Zamok-0.1.0.msi /qn TARGETDIR=C:uild\msi
Get-ChildItem C:uild\msi -Recurse -Include zamok-core.exe, zamok-shell.dll | Get-FileHash
```

Итог проверки на компьютере разработчика записан в [ROADMAP.md](ROADMAP.md), этап 12.

## Подпись (позже)

Сейчас установщик и программы без подписи. План: заявка в SignPath Foundation (бесплатно для открытых проектов), запасной вариант: платный сертификат OV ([DESIGN.md](docs/DESIGN.md), раздел 8). Когда подпись появится, здесь добавится шаг перед расчётом SHA-256, а из README уйдёт инструкция про SmartScreen. Bundle Burn подписывается вместе с вложенным движком, порядок описан в документации WiX (`wix burn detach` и `reattach`).

## Скриншоты

Снимаются с окон программы при запущенном отдельном core: переменные `ZAMOK_DATA_DIR` и `ZAMOK_PIPE`, как в `tests\ui\test-ui.ps1`, образцы папок в `tests\tmp`. На снимках не должно быть личных путей и имён.

## Сайт

`site/` публикуется на GitHub Pages сценарием `.github/workflows/pages.yml`, картинки он копирует из `docs/images`. Пока репозиторий приватный, сценарий пропускается.
