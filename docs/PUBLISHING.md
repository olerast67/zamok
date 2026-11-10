# Что сделать при открытии репозитория

Репозиторий открыт 10.11.2026: описание и темы заданы, включены приватные сообщения об уязвимостях, сайт на GitHub Pages (https://olerast67.github.io/zamok/), Dependabot, поиск секретов с запретом их отправки, CodeQL. В `main` сливается только запрос, у которого зелёные проверки `core` и `ui`. Релиза и тега пока нет: шаг 6 ниже не выполнен.

## Решить до открытия

Подписывать ли первую публичную сборку. В [ROADMAP.md](../ROADMAP.md) открытие и подпись стоят на этапе 12, а альфа выходит раньше. SignPath Foundation подписывает только открытые проекты, поэтому заявка подаётся после открытия.

Решено автором: строка о том, что большую часть кода написал ИИ-ассистент, есть в README.md и README.ru.md. Сочетание GPLv3 и Windows App SDK оформлено в [LICENSE-EXCEPTION.md](../LICENSE-EXCEPTION.md).

## Проверка на личные данные

Проверено 10.11.2026 дважды, второй раз перед самым открытием, по всей истории git (все ветки и коммиты), по текстам запросов на слияние, задач и комментариев:

- авторы и коммитеры только `olerast <53008835+olerast67@users.noreply.github.com>` и `GitHub <noreply@github.com>`;
- нет адреса почты, имени компьютера, SID пользователя, путей к домашней папке пользователя и к папкам пакета приложения Claude, токенов и ключей (`ghp_`, `github_pat_`, `sk-ant`, `AKIA`, `PRIVATE KEY`);
- в картинках, значках и тестовых файлах нет метаданных EXIF и текстовых блоков PNG, в снимках окон только выдуманные папки в `C:\Demo`;
- имя `olerast` встречается только как открытый логин `olerast67` и имя издателя в установщике.

Если после этой даты появились коммиты, повторить перед открытием:

```bash
git grep -I -i -E 'Users[\\/]+olerast|@gmail|S-1-5-21-[0-9]|ghp_|github_pat_|sk-ant' $(git rev-list --all)
```

## Описание и темы

Описание:

> Folder encryption for Windows 10 and 11 from File Explorer. The folder stays at its path. Cryptomator format, GPL-3.0, alpha.

Темы: `encryption`, `windows`, `file-explorer`, `winfsp`, `cryptomator`, `rust`, `winui3`, `folder-encryption`, `privacy`, `gpl3`

## Pages

Источник: GitHub Actions (сценарий `.github/workflows/pages.yml` уже в репозитории, пока репозиторий приватный, он пропускается). Адрес: https://olerast67.github.io/zamok/

## Текст релиза 0.1.0-alpha

Сохранить в `release-notes.md`, вместо `<SHA-256>` подставить отпечаток:

```markdown
First alpha of Zamok, folder encryption for Windows 10 and 11 from File Explorer.

It has not been audited by a third party. Keep a backup of important data outside Zamok.

Download `Zamok-Setup-0.1.0.exe`. It contains the WinFsp driver and installs it only if absent.

SHA-256: `<SHA-256>`

The installer is not signed, so SmartScreen shows "Windows protected your PC". Check the SHA-256, then choose More info and Run anyway.

Do not drag an open folder to another place with the mouse: Explorer copies the files unencrypted and deletes them from the vault. Lock the folder first.

Without Zamok, Cryptomator opens the folder on Windows, macOS, Linux, iOS or Android.

Full list of changes: CHANGELOG.md.
```

## Команды по порядку

```powershell
# 1. Сборка и отпечаток описаны в RELEASING.md
# 2. Описание и темы
gh repo edit olerast67/zamok --description "Folder encryption for Windows 10 and 11 from File Explorer. The folder stays at its path. Cryptomator format, GPL-3.0, alpha." --homepage "https://olerast67.github.io/zamok/"
gh repo edit olerast67/zamok --add-topic encryption --add-topic windows --add-topic file-explorer --add-topic winfsp --add-topic cryptomator --add-topic rust --add-topic winui3 --add-topic folder-encryption --add-topic privacy --add-topic gpl3

# 3. Открыть репозиторий. Это необратимо для всех, кто успеет склонировать
gh repo edit olerast67/zamok --visibility public --accept-visibility-change-consequences

# 4. Приватные сообщения об уязвимостях, на них ссылается SECURITY.md
gh api -X PUT repos/olerast67/zamok/private-vulnerability-reporting

# 5. Pages из GitHub Actions и первая публикация
gh api -X POST repos/olerast67/zamok/pages -f build_type=workflow
gh workflow run pages.yml --repo olerast67/zamok

# 6. Релиз как предварительная версия
gh release create v0.1.0-alpha installer\out\Zamok-Setup-0.1.0.exe --repo olerast67/zamok --prerelease --title "Zamok 0.1.0-alpha" --notes-file release-notes.md
```

## После открытия

В [CHANGELOG.md](../CHANGELOG.md) заменить «not released yet» на дату. Проверить в браузере без входа в GitHub: значки в README, картинки, сайт на русском и английском, ссылку на релиз.
