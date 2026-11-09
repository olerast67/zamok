# Что сделать при открытии репозитория

Этот файл только готовит публикацию. Ничего из перечисленного ещё не выполнено: репозиторий приватный, релиза и тега нет, Pages выключен. Команды запускает автор.

## Решить до открытия

1. Подписывать ли первую публичную сборку. В [ROADMAP.md](../ROADMAP.md) открытие и подпись стоят на этапе 12, а альфа выходит раньше.
2. Сказать ли в README, какая часть кода написана ИИ-ассистентом. В Суфлёре такая строка есть. В Zamok она не добавлена, потому что это решение автора.
3. Лицензия Windows App SDK: в [THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md) указаны условия Microsoft, файлы лежат рядом с программой под GPLv3. Проверить, устраивает ли такое сочетание (исключение GPLv3 для системных библиотек).
4. В истории git адреса авторов коммитов уже вида `users.noreply.github.com`. Перед открытием полезно ещё раз посмотреть историю на личные пути: `git log --all -p -S"C:\Users"`.

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
