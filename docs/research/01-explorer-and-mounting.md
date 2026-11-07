# Исследование 01: Explorer и монтирование (win-lock)

Дата: 2026-11-07. Метод: веб-поиск и 10 загрузок страниц. Пометки: [проверено] есть источник, [не проверено] из общих знаний или косвенных данных, нужен тест на железе.

Главный вывод: WinFsp в режиме «монтирование в папку» подходит для расшифрованного вида, но у него жёсткое ограничение (папка-точка монтирования не должна существовать заранее), и оно определяет всю схему «тот же путь». Двойной клик надёжнее всего перехватывать самим WinFsp-томом, а не shell-расширением внутри Explorer.

---

## 1. WinFsp: монтирование в путь папки

### 1.1 Можно ли монтировать в произвольную папку NTFS

Да. `FspFileSystemSetMountPoint` принимает либо букву диска, либо путь к каталогу. В документации сказано: каталоги подходят для дисковых файловых систем и не подходят для сетевых, это ограничение junction в Windows. NULL означает «первая свободная буква с Z вниз». [проверено] https://docs.rs/winfsp-sys/latest/winfsp_sys/fn.FspFileSystemSetMountPoint.html (зеркало документации WinFsp)

Требования к пути (по обсуждениям rclone и cgofuse): [проверено, вторичные источники]
- Последний компонент пути НЕ должен существовать (в Linux наоборот). Родитель должен существовать.
- WinFsp сам создаёт каталог и ставит на него reparse point, поэтому том с родителем должен поддерживать reparse points (NTFS/ReFS; на сетевых шарах вроде Samba бывают проблемы).
- Источники: https://forum.rclone.org/t/windows-rclone-mount-refuses-pathname-mount-point-winfsp-issue/15264 , https://github.com/winfsp/cgofuse/issues/33

Следствие: чтобы смонтировать том в `C:\Users\me\Docs\secret`, исходную папку `secret` нужно убрать с этого пути, иначе mount не пройдёт. «Зашифровать» = (1) переместить содержимое в Cryptomator vault в другом месте, (2) удалить исходный каталог, (3) смонтировать том на освободившийся путь.

### 1.2 Как реализовано

Официального описания в доступных страницах нет. Из фразы «limitation Windows imposes on junctions» и из поведения (в `dir` точка монтирования видна как `<JUNCTION>`) следует: это mount-point reparse point (junction) на каталоге, указывающий на `\Device\Volume{...}` тома WinFsp. [частично проверено: junction следует из документации и сообщений Cryptomator, внутренности по исходникам не подтверждены]

Попутно: относительные symlink внутри тома (например `..`) могут не разрешаться при обходе через точку монтирования (issue winfsp #454). Для нас маргинально. [проверено] https://github.com/winfsp/winfsp/issues/454

### 1.3 Что остаётся при аварии или потере питания

- Если процесс-файловая система упал, том исчезает, а каталог с reparse point на диске остаётся: это постоянная запись NTFS, она переживает перезагрузку. Путь дальше ведёт в никуда.
- Подтверждение: у Cryptomator (с Dokany, механика та же, junction) при выключении Windows с примонтированным хранилищем папка остаётся ссылкой, следующий mount падает с «Mount Point does not exist». Решение: удалить ссылку, создать пустую папку с тем же именем. https://community.cryptomator.org/t/custom-mount-point-directory-changed-disabled-when-cryptomator-is-shut-down-by-windows/4225
- В группе WinFsp пользователь решал это так: перед монтированием определял осиротевшую папку (после краха или рестарта) и удалял её. https://groups.google.com/g/winfsp/c/ESmwF52KjmY
- Пока Explorer или другой процесс держит открытый handle на каталог, драйвер не может превратить его в reparse point (Dokany-тред). https://community.cryptomator.org/t/empty-vault-when-mounting-point-is-on-same-file-system/7034
- Расшифрованные данные при аварии на диск не попадают (они только в памяти процесса). Остаётся только пустой reparse point.

Очистка в win-lock: при старте службы и перед каждым монтированием проверять путь. Если это reparse point с тегом `IO_REPARSE_TAG_MOUNT_POINT` и живого тома за ним нет, снять reparse (`FSCTL_DELETE_REPARSE_POINT`) и `RemoveDirectory`. Это наша рекомендация, [не проверено] именно на WinFsp, но та же процедура описана у Cryptomator.

Чтобы восстановиться после сбоя, храним в метаданных вне P запись «на P должен быть смонтирован такой-то vault».

### 1.4 Поведение Explorer с такой точкой монтирования

Прямых источников не найдено, ниже ожидаемое поведение. [не проверено, нужны тесты]
- Refresh, навигация, drag-and-drop, «открыть расположение файла», открытие из IDE: как на любом отдельном томе, пока ФС корректно реализует Create, QueryDirectory и атрибуты. Cryptomator работает именно так (чаще с буквой диска, но режим «папка» поддерживается).
- Корзина: том WinFsp не имеет `$Recycle.Bin`, удаление в Проводнике обычно предлагает «удалить без возможности восстановления». Для приватности это плюс (удалённое не уезжает в `C:\$Recycle.Bin`), для UX минус: предупредить пользователя.
- Windows Search: по умолчанию такие пути, как правило, не индексируются, но пользователь может включить. Риск: индекс `Windows.edb` хранит имена и фрагменты содержимого на диске. Нужно исключить путь программно (`ISearchCrawlScopeManager`) и/или выставить `FILE_ATTRIBUTE_NOT_CONTENT_INDEXED` на корне, проверить тестом.
- Миниатюры: Explorer пишет кэш `%LOCALAPPDATA%\Microsoft\Windows\Explorer\thumbcache_*.db` на системный диск. Это утечка данных на диск, и отключить её для конкретного пути нельзя. Варианты: предупреждать, чистить thumbcache при lock, не отдавать форматы, для которых строятся миниатюры (неприемлемо). Важный риск дизайна.
- Другие следы от самой ОС, не от нас: Recent (`.lnk` в `%APPDATA%\Microsoft\Windows\Recent`), Jump Lists, pagefile и hiberfil, Prefetch, кэши антивируса. В модели угроз формулировать как «программа сама не пишет расшифрованное на диск, но ОС оставляет следы».

### 1.5 Windows 11 24H2 и 25H2

Поиск не нашёл issue или релиз-нотов WinFsp про 24H2/25H2. README заявляет поддержку «Windows 7 to Windows 11» на x86, x64, ARM64. [проверено] https://github.com/winfsp/winfsp/blob/master/README.md Проблемы именно 24H2/25H2 не найдены, значит неизвестны; тест в VM обязателен (у автора Windows 10 LTSC 19044).

### 1.6 HVCI (Memory Integrity) и Smart App Control

- HVCI не даёт загружаться драйверам с W+X-секциями и несовместимым с VBS; список виден в «Review incompatible drivers». Данных по WinFsp в поиске нет. [не проверено] Из общих знаний (низкая уверенность): драйвер WinFsp подписан Microsoft и Cryptomator с WinFsp обычно работает при включённой Memory Integrity. Проверить тестом на `winfsp.sys`.
- Вторичные источники пишут, что с октября 2026 Microsoft начинает автоматически включать Memory Integrity на подходящих Windows 11 (не включает при конфликтующих драйверах). Тогда совместимость с HVCI станет практически обязательной. [не проверено] https://www.purepc.pl/windows-11-memory-integrity-hvci-pazdziernik-2026
- Smart App Control проверяет user-mode приложения, не драйверы. Подробнее в разделе 3.

### 1.7 Лицензия WinFsp при поставке установщика в open-source приложении

GPLv3 плюс FLOSS-исключение. Без перехода на GPLv3 разрешено: (1) линковка с `winfsp-*.dll`, (2) распространение неизменённых бинарных установщиков WinFsp. Условия: [проверено] https://github.com/winfsp/winfsp/blob/master/License.txt
1. Наш софт под лицензией, соответствующей Free Software Definition или Open Source Definition.
2. В UI и пользовательской документации есть строка «WinFsp - Windows File System Proxy, Copyright (C) Bill Zissimopoulos» и ссылка на репозиторий WinFsp.
3. Нельзя смешивать с проприетарным ПО в одной поставке.
Коммерческая лицензия возможна по запросу автору.

Практика: кладём неизменённый `winfsp-*.msi` в установщик (или скачиваем при установке), в «О программе» и README пишем нужную строку со ссылкой, лицензия win-lock OSI-совместимая.

---

## 2. Перехват двойного клика по заблокированной папке

Из п.1.1: в момент монтирования на P не должно быть каталога. Значит в «заблокированном» состоянии на P нужно что-то держать, и оно определяет способ перехвата.

### (a) desktop.ini с CLSID/CLSID2 (shell namespace junction, свой IShellFolder)

- Механизм: в `[.ShellClassInfo]` указывается `CLSID={...}`, папка (атрибут System/ReadOnly) становится shell-папкой, при входе Explorer создаёт наш COM-объект `IShellFolder`. В официальной MS-странице про desktop.ini перечислены только IconFile, IconIndex, InfoTip, ConfirmFileOp, NoSharing, CLSID там нет. [проверено] https://learn.microsoft.com/en-us/windows/win32/shell/how-to-customize-folders-with-desktop-ini
- Подтверждения, что это работает на Win10/11, не найдено, только форумный совет. [не проверено] https://www.sordum.org/10084/desktop-ini-editor-v1-2/comment-page-1/
- Тот же путь: да. Но DLL грузится внутрь `explorer.exe` и любого процесса с диалогом открытия файла (IDE, браузеры). Ошибка в `IShellFolder` роняет Explorer или чужие программы.
- Риск краха Explorer: высокий. Усилия: высокие (IShellFolder, IShellFolder2, IEnumIDList, IPersistFolder, 32/64 бит, отладка внутри Explorer). [оценка]

### (b) Folder Shortcut {0AFACED1-E828-11D1-9187-B532F1E9575D} с target.lnk

- Папка с `desktop.ini` (`CLSID2={0AFACED1-...}`, `Flags=2`) и `target.lnk` внутри ведёт себя как ярлык: двойной клик открывает целевую папку (форумное описание). Работает, лишь если тип папки ещё не закреплён в Shell\Bags. [не проверено на Win11] https://www.sordum.org/10084/desktop-ini-editor-v1-2/comment-page-1/
- Тот же путь: нет, открывается цель ярлыка, адресная строка показывает цель. Цель обязана быть папкой, exe с паролем не запустить (без возни с `shell:::{CLSID}`, то есть снова (a)).
- Риск краха Explorer: низкий (встроенный компонент). Усилия: малые. Вердикт: не подходит.

### (c) Замена папки файлом (.wlock), как у WinFolderLock

- Проект Atron1792/WinFolderLock использует расширение `.wflck`: при блокировке папка сжимается и шифруется в файл, при временной разблокировке открывается сессионная папка и после закрытия шифруется снова. Пароли хранятся в `C:\ProgramData\WinFolderLock\passwords.json`; проект сам называет это защитой от случайного доступа. [проверено через зеркало] https://github.com/Atron1792/WinFolderLock/releases
- Тот же путь: нет (папка превращается в файл), данные при разблокировке лежат на диске. Нарушает два ключевых требования. Риск краха Explorer: нет. Усилия: малые. Вердикт: не подходит.

### (d) Прочее

**Cloud Files API (cfapi).** Sync root регистрирует целое дерево, корни не могут перекрываться и вкладываться. [проверено] https://learn.microsoft.com/en-us/windows/win32/api/cfapi/nf-cfapi-cfregistersyncroot Плейсхолдеры гидратируются на реальный NTFS-том, то есть расшифрованные данные ложатся на диск. Плюс конфликты с OneDrive и подобными. Вердикт: не подходит, прежде всего по требованию безопасности. [вывод из документа и общих знаний]

**Сама папка как постоянный WinFsp-том (рекомендуемый вариант).**
- Служба win-lock при логине монтирует том на P. Том имеет два состояния. Locked: корень недоступен (STATUS_ACCESS_DENIED) или содержит один файл-заглушку. Unlocked: проекция Cryptomator vault. Переключение происходит внутри процесса ФС, поэтому unmount и remount на тот же путь не нужны (нет гонок с handle Explorer и проблемы «путь не существует»).
- Триггер prompt: WinFsp позволяет узнать процесс-инициатор операции (название API `FspFileSystemOperationProcessId*`, [не проверено]). В обработчике открытия корня в состоянии locked, если инициатор `explorer.exe`, запускаем отдельный UI-процесс с паролем и держим Create до ответа. Риск: корень открывают также индексатор, Defender, предпросмотр, диалоги других программ. Нужна эвристика, поведение надо проверить тестом. [не проверено]
- Запасной канал: в locked-состоянии том отдаёт единственный файл «Разблокировать» (двойной клик запускает UI).
- Тот же путь: да, всегда. Риск краха Explorer: низкий (наш код вне Explorer). Усилия: средние.
- Недостаток: если служба не запущена, на P остаётся осиротевший reparse point (п.1.3), папка выглядит битой. Нужны автозапуск и самовосстановление. После перезагрузки старую «мёртвую» точку надо удалить до нового монтирования.

### Сводка

| Способ | Тот же путь | Риск краха explorer.exe | Усилия | Данные не на диске | Вердикт |
|---|---|---|---|---|---|
| (a) desktop.ini CLSID + IShellFolder | да | высокий | высокие | зависит от реализации | избегать |
| (b) Folder Shortcut + target.lnk | нет | низкий | малые | n/a | не годится |
| (c) файл .wlock | нет | нет | малые | нет | не годится |
| (d1) cfapi | да | низкий | высокие | нет | не годится |
| (d2) постоянный WinFsp-том, два состояния | да | низкий | средние | да | рекомендую |

---

## 3. Контекстное меню Windows 11 и подпись

### 3.1 Верхний уровень меню Windows 11

- Нужны COM-класс `IExplorerCommand`, манифест с регистрацией как расширения контекстного меню Проводника и package identity. Для неупакованного Win32 приложения identity даёт sparse package (packaging with external location). [проверено] https://learn.microsoft.com/en-my/WINDOWS/APPS/desktop/modernize/integrate-packaged-app-with-file-explorer , https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/grant-identity-to-nonpackaged-apps
- Требования sparse package: `uap10:AllowExternalContent=true`, `MinVersion=10.0.19041.0`, `runFullTrust`, `Publisher` совпадает с Subject сертификата, регистрация `Add-AppxPackage -ExternalLocation` или `PackageManager.AddPackageByUriAsync`. [проверено] там же.
- Подпись обязательна: пакет подписывается сертификатом, которому доверяет целевая машина. Самоподписанный работает, если его `.cer` импортирован в Trusted People (`Cert:\CurrentUser\TrustedPeople`, либо `LocalMachine` с админом), иначе ошибка `0x800B0109`. Для компьютеров конечных пользователей Microsoft требует «production certificate». [проверено] ссылка выше.
- Практичность самоподписанного: технически работает (пример: https://github.com/2xRon/7zipExplorerExtension), но установщик должен добавить наш сертификат в доверенные на машине пользователя. Это правка настроек безопасности, нужны права, выглядит подозрительно. Годится как dev/portable вариант, не как основной. [оценка]
- Запасной путь без подписи: классический verb в `HKCU\Software\Classes\Directory\shell`, в Windows 11 виден в «Показать дополнительные параметры». [общие знания, не проверено в этой сессии]
- После регистрации пакета может понадобиться перезапуск Explorer. [проверено, вторично] https://www.advancedinstaller.com/user-guide/context-menu.html

### 3.2 Бесплатная и дешёвая подпись

**SignPath Foundation (бесплатно для open source).** Условия [проверено, страница помечена как draft]: https://signpath.org/terms
- Все компоненты под OSI-лицензией, без коммерческого dual-licensing и без проприетарного кода.
- Проект активно развивается, подписываемая версия уже опубликована, функциональность описана на странице загрузки.
- Для исполняемых файлов нужна проверяемая репутация проекта; подписывает та же команда, что отвечает за исходники; подписываются только собственные сборки из исходников.
- Роли author, reviewer, approver, MFA у всех участников, политика подписи на главной странице, без хакерских инструментов и скрытой отправки данных, поддержка деинсталляции, обязательные метаданные имени и версии. Организация вправе отклонить любой проект.
- Для нового проекта без истории риск отказа высокий. Ограничений по стране автора в условиях не видно, но платёжные и санкционные ограничения не проверялись. [не проверено] Стоит просто подать заявку.

**Azure Artifact Signing (бывший Trusted Signing).**
- Цена: Basic 9,99 USD в месяц за до 5000 подписей, далее 0,005 USD за подпись. [вторично, страница цен не отрисовалась] https://azure.microsoft.com/en-us/products/artifact-signing . Нужна платная подписка Azure (free, trial, sponsored не подходят). [проверено] https://learn.microsoft.com/azure/artifact-signing/faq
- География Public Trust: организации в США, Канаде, ЕС, Великобритании, Австралии, Новой Зеландии, Японии, Южной Корее, Сингапуре, Швейцарии, Норвегии, Израиле. Индивидуальные разработчики только в США или Канаде. России нет ни там, ни там. [проверено] https://learn.microsoft.com/en-us/azure/artifact-signing/quickstart Для автора из РФ закрыто (кроме юрлица в разрешённой стране).
- Не EV; репутация SmartScreen набирается постепенно. [проверено] FAQ выше.

**Платные OV-сертификаты у CA (Sectigo, SSL.com, Certum и др.)**: порядка десятков-сотен USD в год, с 2023 ключ на аппаратном токене, доступность для РФ надо проверять. [общие знания, не проверено]

### 3.3 SmartScreen и Smart App Control

- SmartScreen на неподписанный EXE из интернета показывает «Windows protected your PC» с возможностью «Run anyway». [общие знания] У подписанных репутация набирается по истории скачиваний. [проверено частично] FAQ выше.
- Smart App Control (только на чистых установках Windows 11): сначала облачная репутация, при её отсутствии нужна валидная подпись, иначе блок. [вторично] https://www.windowsmode.com/?p=48159 . Раньше выключить можно было необратимо, по сообщениям (декабрь 2025, апрель 2026) теперь переключается без переустановки, но есть баг KB5127648, из-за которого переключатель серый. [вторично] https://windowslatest.com/2025/12/16/microsoft-confirms-you-can-soon-disable-smart-app-control-without-reinstalling-windows-11
- Итог: неподписанный win-lock.exe на машине с включённым SAC не запустится. Драйвер WinFsp подписан автором WinFsp, блок касается нашего user-mode кода.

---

## 4. Иконка замка на папке

### 4.1 Варианты

- `desktop.ini` (`IconResource=путь,индекс` или `IconFile`/`IconIndex`), папка должна быть System или ReadOnly. Официальная страница описывает IconFile и IconIndex и правило system folder; `IconResource` там не упомянут, но давно используется Explorer. [проверено частично] https://learn.microsoft.com/en-us/windows/win32/shell/how-to-customize-folders-with-desktop-ini
- Icon overlay handler (`IShellIconOverlayIdentifier`) в `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\ShellIconOverlayIdentifiers`. Известный лимит: используются только первые 15 обработчиков по алфавиту ключей, реально свободно меньше, так как часть занимает Windows, OneDrive, Dropbox, TortoiseGit. Нужны права админа, DLL грузится в Explorer. [общие знания; страница Microsoft вернула 404, не перепроверено]

### 4.2 Что надёжнее

В схеме (d2) на P стоит WinFsp-том, значит `desktop.ini` должна отдавать сама ФС из корня (Hidden+System, `IconResource` на ресурс нашего exe/dll), на корне атрибут System или ReadOnly. Подхватит ли Explorer `desktop.ini` из корня тома, смонтированного в папку, [не проверено]. При смене состояния вызывать `SHChangeNotify`; иконки кэшируются в IconCache, смена может проявиться не сразу. [общие знания]

Сравнение: `desktop.ini` проще и не грузит наш код в Explorer, лимита слотов нет. Overlay надёжнее в отображении, но исчерпаем (15) и опасен как in-process DLL.

---

## Рекомендация

### Вопрос 1 (WinFsp в папку)
Использовать WinFsp, монтирование в путь папки. Принять правило «P не существует при монтировании»: «Зашифровать» = переместить данные в Cryptomator vault, удалить исходную папку, смонтировать том на P. Хранить вне P запись о соответствии P и vault для самовосстановления. Реализовать очистку мёртвых reparse points на P при старте службы и перед каждым монтированием. Поставлять неизменённый MSI WinFsp, в UI и README указать авторство WinFsp со ссылкой, лицензия win-lock OSI-совместимая. До релиза протестировать Windows 11 24H2/25H2 и включённую Memory Integrity (подтверждений в сети нет). В модели угроз честно описать утечки ОС: thumbcache, Recent и Jump Lists, индекс Search, pagefile и hiberfil.

### Вопрос 2 (перехват двойного клика)
Основной вариант (d2): постоянно смонтированный WinFsp-том на P с состояниями locked и unlocked, prompt из отдельного UI-процесса при открытии корня из `explorer.exe`, плюс запасной файл «Разблокировать» внутри locked-тома. Причины: путь всегда тот же, нет remount при смене состояния, наш код вне Explorer, нет хрупких IShellFolder. Отказаться от (a) из-за риска падений Explorer и стоимости, от (b) и (c) из-за нарушения «тот же путь», от cfapi из-за гидратации данных на NTFS. Открытый риск: ложные срабатывания prompt от индексатора и антивируса.

### Вопрос 3 (меню Windows 11 и подпись)
MVP: классический verb в `HKCU\Software\Classes\Directory\shell`, подпись не нужна (на Win11 в «Показать дополнительные параметры»). Верхнее меню (IExplorerCommand и sparse package) вторым этапом, когда появится сертификат. Самоподписанный сертификат с Trusted People только для dev/portable. Для автора из России Azure Artifact Signing закрыт, реальный путь: заявка в SignPath Foundation (OSI-лицензия, публичная активная история, MFA, политика подписи на сайте) и параллельно проверить платный OV-сертификат. До подписи раздавать через GitHub Releases с хешами и инструкцией про SmartScreen, помня что Smart App Control заблокирует неподписанный EXE.

### Вопрос 4 (иконка)
`desktop.ini` с `IconResource`, атрибут System на корне, `SHChangeNotify` при смене состояния: не грузит наш код в Explorer и не ограничен 15 слотами. Обязательно проверить, что Explorer берёт `desktop.ini` из корня WinFsp-тома в папке. Overlay handler оставить резервом на v2.

### Что проверить первым делом (всё пока [не проверено])
1. Прототип WinFsp-тома в папке на NTFS: kill процесса, перезагрузка, очистка reparse point.
2. Explorer на таком томе: корзина, поиск, миниатюры, drag-and-drop, «открыть расположение файла», IDE.
3. Подхват `desktop.ini` и иконки из корня тома.
4. Эвристика prompt при открытии корня и число ложных открытий.
5. Windows 11 24H2/25H2 в VM с Memory Integrity и Smart App Control.

## Источники

- https://github.com/winfsp/winfsp/blob/master/License.txt
- https://github.com/winfsp/winfsp/blob/master/README.md
- https://docs.rs/winfsp-sys/latest/winfsp_sys/fn.FspFileSystemSetMountPoint.html
- https://forum.rclone.org/t/windows-rclone-mount-refuses-pathname-mount-point-winfsp-issue/15264
- https://github.com/winfsp/cgofuse/issues/33
- https://groups.google.com/g/winfsp/c/ESmwF52KjmY
- https://github.com/winfsp/winfsp/issues/454
- https://community.cryptomator.org/t/custom-mount-point-directory-changed-disabled-when-cryptomator-is-shut-down-by-windows/4225
- https://community.cryptomator.org/t/empty-vault-when-mounting-point-is-on-same-file-system/7034
- https://learn.microsoft.com/en-us/windows/win32/shell/how-to-customize-folders-with-desktop-ini
- https://www.sordum.org/10084/desktop-ini-editor-v1-2/comment-page-1/
- https://github.com/Atron1792/WinFolderLock/releases
- https://learn.microsoft.com/en-us/windows/win32/api/cfapi/nf-cfapi-cfregistersyncroot
- https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/grant-identity-to-nonpackaged-apps
- https://learn.microsoft.com/en-my/WINDOWS/APPS/desktop/modernize/integrate-packaged-app-with-file-explorer
- https://github.com/2xRon/7zipExplorerExtension
- https://signpath.org/terms
- https://learn.microsoft.com/en-us/azure/artifact-signing/quickstart
- https://learn.microsoft.com/azure/artifact-signing/faq
- https://azure.microsoft.com/en-us/products/artifact-signing
- https://windowslatest.com/2025/12/16/microsoft-confirms-you-can-soon-disable-smart-app-control-without-reinstalling-windows-11
- https://www.windowsmode.com/?p=48159
- https://www.purepc.pl/windows-11-memory-integrity-hvci-pazdziernik-2026
