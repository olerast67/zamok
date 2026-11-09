# Выпуск версии

Сборка пока идёт на компьютере разработчика: `.github/workflows/ci.yml` только проверяет код. Сборка релиза в GitHub Actions и воспроизводимость есть в [ROADMAP.md](ROADMAP.md), этап 12.

## Что нужно

Windows, диск NTFS, инструменты из [CONTRIBUTING.md](CONTRIBUTING.md), локальный WiX (`dotnet tool restore`).

## Каждый выпуск

1. Обновить версию в `core/crates/*/Cargo.toml`, запись в [CHANGELOG.md](CHANGELOG.md), отметки в [ROADMAP.md](ROADMAP.md).
2. Запустить проверки из [CONTRIBUTING.md](CONTRIBUTING.md). `tests\ui\test-ui.ps1` и тесты установки показывают окна и работают с реестром пользователя, поэтому их запускают на отдельной системе или в Windows Sandbox (`tests\install\test-sandbox.ps1`).
3. Собрать установщик. Версия WiX состоит только из чисел, поэтому суффикс `-alpha` есть в теге, а в имени файла его нет:

   ```powershell
   powershell -NoProfile -ExecutionPolicy Bypass -File installer\build-msi.ps1 -Version 0.1.0
   ```

   Результат: `installer\out\Zamok-Setup-0.1.0.exe`. Сценарий скачивает WinFsp и принимает файл только с записанным SHA-256 и подписью издателя.
4. Посчитать отпечаток:

   ```powershell
   Get-FileHash installer\out\Zamok-Setup-0.1.0.exe -Algorithm SHA256
   ```

5. Поставить установщик в Windows Sandbox: зашифровать папку, запереть, открыть, открыть её в Cryptomator.
6. Создать релиз командами из [docs/PUBLISHING.md](docs/PUBLISHING.md), вписать SHA-256 в текст.

## Подпись (позже)

Сейчас установщик и программы без подписи. План: заявка в SignPath Foundation (бесплатно для открытых проектов), запасной вариант: платный сертификат OV ([DESIGN.md](docs/DESIGN.md), раздел 8). Когда подпись появится, здесь добавится шаг перед расчётом SHA-256, а из README уйдёт инструкция про SmartScreen. Bundle Burn подписывается вместе с вложенным движком, порядок описан в документации WiX (`wix burn detach` и `reattach`).

## Скриншоты

Снимаются с окон программы при запущенном отдельном core: переменные `ZAMOK_DATA_DIR` и `ZAMOK_PIPE`, как в `tests\ui\test-ui.ps1`, образцы папок в `tests\tmp`. На снимках не должно быть личных путей и имён.

## Сайт

`site/` публикуется на GitHub Pages сценарием `.github/workflows/pages.yml`, картинки он копирует из `docs/images`. Пока репозиторий приватный, сценарий пропускается.
