"""Строки интерфейса Zamok.UI: одна таблица, из неё два файла Resources.resw.

Запуск после правки: python ui/Zamok.UI/Strings/strings.py
Ключ с точкой (Unlock_Open.Content) задаёт свойство элемента с x:Uid, ключ без точки
читается из кода (Strings.Get). Тест Zamok.Tests проверяет, что для каждого кода ошибки
core есть строка Error_<код> на обоих языках.
"""
import os
from xml.sax.saxutils import escape

S = {
    # Общие
    "Common_Cancel.Content": ("Отмена", "Cancel"),
    "Common_Back.Content": ("Назад", "Back"),
    "Common_Next": ("Далее", "Next"),
    "Common_Close": ("Закрыть", "Close"),
    # Окно пароля
    "Unlock_Prompt.Text": ("Введите пароль, чтобы открыть папку.", "Enter the password to open the folder."),
    "Unlock_PromptRecovery.Text": ("Вставьте файл ключа восстановления целиком или введите 44 слова по порядку.",
                                   "Paste the whole recovery key file or type the 44 words in order."),
    "Unlock_Password.Header": ("Пароль", "Password"),
    "Unlock_RecoveryWords.Header": ("Ключ восстановления", "Recovery key"),
    "Unlock_UseRecovery.Content": ("Забыли пароль? Открыть ключом восстановления", "Forgot the password? Use the recovery key"),
    "Unlock_UsePassword.Content": ("Открыть паролем", "Use the password"),
    "Unlock_Open.Content": ("Открыть", "Open"),
    # Мастер шифрования
    "Encrypt_Heading": ("Зашифровать «{0}»", "Encrypt \u201c{0}\u201d"),
    "Encrypt_StepPassword": ("Придумайте пароль: он нужен каждый раз, чтобы открыть папку", "Choose a password: you will need it every time you open the folder"),
    "Encrypt_StepKey": ("Ключ восстановления", "Recovery key"),
    "Encrypt_StepVerify": ("Проверка ключа", "Checking the key"),
    "Encrypt_StepProgress": ("Шифрование", "Encrypting"),
    "Encrypt_StepDone": ("Готово", "Done"),
    "Encrypt_Password1.Header": ("Пароль", "Password"),
    "Encrypt_Password2.Header": ("Пароль ещё раз", "Repeat the password"),
    "Encrypt_Strength0": ("Нужно хотя бы 8 знаков.", "At least 8 characters are needed."),
    "Encrypt_Strength1": ("Слабый пароль: его легко подобрать.", "Weak password: easy to guess."),
    "Encrypt_Strength2": ("Средний пароль.", "Fair password."),
    "Encrypt_Strength3": ("Хороший пароль.", "Good password."),
    "Encrypt_Strength4": ("Надёжный пароль.", "Strong password."),
    "Encrypt_MakeKey.Content": ("Создать ключ восстановления", "Create a recovery key"),
    "Encrypt_NoKeyWarning.Text": (
        "Без ключа восстановления забытый пароль значит, что файлы в папке не вернуть никак: их не откроет ни Zamok, ни кто-либо другой.",
        "Without a recovery key, a forgotten password means the files are lost for good: nobody, Zamok included, can open them."),
    "Encrypt_Mismatch": ("Пароли не совпадают.", "The passwords do not match."),
    "Encrypt_Start": ("Зашифровать", "Encrypt"),
    "Encrypt_Check": ("Проверить", "Check"),
    "Encrypt_LockNow.Content": ("Запереть сейчас", "Lock now"),
    "Encrypt_KeyIntro.Text": (
        "Если забудете пароль, папку откроет только этот ключ. Сохраните его в файл или запишите и храните отдельно от компьютера. Папка уже шифруется.",
        "If you forget the password, only this key opens the folder. Save it to a file or write it down and keep it away from this computer. The folder is already being encrypted."),
    "Encrypt_SaveKeyText.Text": ("Сохранить в файл", "Save to file"),
    "Encrypt_CopyKeyText.Text": ("Скопировать", "Copy"),
    "Encrypt_KeyFileName": ("Ключ восстановления {0}", "Recovery key {0}"),
    "Encrypt_KeyFileType": ("Текстовый файл", "Text file"),
    "Encrypt_KeyFileText": (
        "Ключ восстановления Zamok для папки «{0}»\r\nСохранён {1}\r\n\r\n{2}\r\n\r\nКак открыть папку ключом: в запертой папке дважды щёлкните «Открыть {0}.zamok», нажмите «Забыли пароль?» и вставьте весь этот файл или введите 44 слова по порядку.",
        "Zamok recovery key for the folder \u201c{0}\u201d\r\nSaved {1}\r\n\r\n{2}\r\n\r\nHow to open the folder with the key: in the locked folder double-click \u201cOpen {0}.zamok\u201d, press \u201cForgot the password?\u201d and paste this whole file or type the 44 words in order."),
    "Encrypt_KeySaved": ("Сохранено.", "Saved."),
    "Encrypt_KeyCopied": ("Скопировано. После вставки удалите ключ из буфера обмена.", "Copied. Clear the clipboard after pasting the key."),
    "Encrypt_VerifyIntro.Text": ("Введите слова с этими номерами, чтобы убедиться, что ключ записан.",
                                 "Enter the words with these numbers to make sure the key is written down."),
    "Encrypt_WordN": ("Слово № {0}", "Word {0}"),
    "Encrypt_WordsWrong": ("Слова не совпали. Нажмите «Назад» и проверьте запись.",
                           "The words do not match. Press Back and check what you wrote down."),
    "Encrypt_ProgressFiles": ("Файлов: {0} из {1}, {2:0.0} из {3:0.0} МБ", "Files: {0} of {1}, {2:0.0} of {3:0.0} MB"),
    "Encrypt_Copying": ("Шифрую и проверяю каждый файл. Оригинал удалю только в конце, после проверки копии.",
                        "Encrypting and checking every file. The original is deleted only at the end, after the copy is checked."),
    "Encrypt_Finishing": ("Подменяю папку зашифрованной.", "Replacing the folder with the encrypted one."),
    "Encrypt_Done": ("Папка «{0}» зашифрована и открыта. Она запрётся сама при блокировке экрана, перед сном и через 30 минут без работы.",
                     "The folder \u201c{0}\u201d is encrypted and open. It locks by itself when the screen locks, before sleep and after 30 minutes without use."),
    "Encrypt_Failed": ("Папка не зашифрована. Все файлы на месте.", "The folder was not encrypted. All files are in place."),
    # Сменить пароль, показать ключ
    "Manage_Password.Header": ("Текущий пароль", "Current password"),
    "Manage_NewPassword1.Header": ("Новый пароль", "New password"),
    "Manage_NewPassword2.Header": ("Новый пароль ещё раз", "Repeat the new password"),
    "Manage_ChangeStep": ("Сменить пароль. Ключ восстановления останется прежним.", "Change the password. The recovery key stays the same."),
    "Manage_KeyStep": ("Ключ восстановления: введите пароль, чтобы его увидеть", "Recovery key: enter the password to see it"),
    "Manage_Change": ("Сменить", "Change"),
    "Manage_Show": ("Показать", "Show"),
    "Manage_Changed": ("Пароль изменён. Старый больше не открывает папку.", "The password has been changed. The old one no longer opens the folder."),
    "Menu_More": ("Zamok", "Zamok"),
    # Ошибки: Error_<код ответа core>
    "Error_core_not_running": ("Фоновая программа Zamok не запущена.", "The Zamok background program is not running."),
    "Error_core_not_trusted": ("На связи не программа Zamok этого пользователя. Пароль не отправлен.",
                               "The other side is not this user's Zamok program. The password was not sent."),
    "Error_bad_request": ("Внутренняя ошибка запроса.", "Internal request error."),
    "Error_not_found": ("Эта папка не зашифрована Zamok.", "This folder is not encrypted with Zamok."),
    "Error_wrong_password": ("Пароль не подошёл.", "Wrong password."),
    "Error_wrong_recovery_key": ("Ключ восстановления не подошёл. Проверьте слова и их порядок.",
                                 "The recovery key does not match. Check the words and their order."),
    "Error_already_added": ("Папка уже зашифрована или лежит внутри зашифрованной.", "The folder is already encrypted or is inside an encrypted folder."),
    "Error_path_occupied": ("На месте папки лежит другая папка или файл.", "Another folder or file is in the place of this folder."),
    "Error_bad_vault": ("Зашифрованные данные папки не найдены или повреждены.", "The encrypted data of the folder is missing or damaged."),
    "Error_mount_failed": ("Не удалось подключить папку. Проверьте, что WinFsp установлен.", "Could not mount the folder. Check that WinFsp is installed."),
    "Error_internal": ("Внутренняя ошибка Zamok.", "Internal Zamok error."),
    "Error_unlock_cancelled": ("Папку заперли, пока проверялся пароль. Попробуйте ещё раз.", "The folder was locked while the password was being checked. Try again."),
    "Error_not_a_plain_folder": ("Это не обычная папка: файл, ссылка или уже зашифрованная папка.", "This is not a regular folder: a file, a link or an already encrypted folder."),
    "Error_system_folder": ("Системные папки, корень диска, папку профиля и папки вроде «Документы» целиком шифровать нельзя. Выберите папку внутри них.",
                            "System folders, drive roots, the profile folder and folders like Documents cannot be encrypted as a whole. Choose a folder inside them."),
    "Error_cloud_folder": ("Папки OneDrive, Dropbox и Яндекс.Диска пока не шифруются: облако могло бы забрать открытые файлы.",
                           "OneDrive, Dropbox and Yandex Disk folders are not supported yet: the cloud could upload the open files."),
    "Error_not_ntfs": ("Папка лежит не на диске NTFS. Флешки и карты памяти пока не поддерживаются.", "The folder is not on an NTFS drive. USB sticks and memory cards are not supported yet."),
    "Error_vault_exists": ("Рядом с папкой уже лежат данные Zamok с таким именем:", "Zamok data with this name already exists next to the folder:"),
    "Error_unsupported_items": ("В папке есть ссылки, файлы «только в облаке», файлы с несколькими жёсткими ссылками или неподходящими именами:",
                                "The folder contains links, cloud-only files, files with several hard links or unsupported names:"),
    "Error_files_in_use": ("Эти файлы открыты в других программах. Закройте их и попробуйте снова:", "These files are open in other programs. Close them and try again:"),
    "Error_unreadable": ("Эти файлы нельзя прочитать:", "These files cannot be read:"),
    "Error_not_enough_space": ("На диске не хватает места на вторую копию папки.", "There is not enough space on the drive for a second copy of the folder."),
    "Error_job_running": ("Уже шифруется другая папка. Дождитесь окончания.", "Another folder is being encrypted. Wait until it finishes."),
    "Error_recovery_pending": ("Прошлое шифрование не завершено. Перезапустите компьютер или фоновую программу Zamok.",
                               "The previous encryption has not finished. Restart the computer or the Zamok background program."),
    "Error_changed_during_copy": ("Пока шла работа, в папке что-то изменилось. Всё отменено, файлы на месте. Закройте программы, которые пишут в папку, и попробуйте снова.",
                                  "Something changed in the folder during the work. Everything was cancelled, the files are in place. Close the programs writing to the folder and try again."),
    "Error_verify_failed": ("Проверка зашифрованной копии не прошла. Всё отменено, файлы на месте.", "The encrypted copy failed the check. Everything was cancelled, the files are in place."),
    "Error_copy_failed": ("Не удалось записать зашифрованную копию. Всё отменено, файлы на месте.", "Could not write the encrypted copy. Everything was cancelled, the files are in place."),
    "Error_folder_in_use": ("Папку держит другая программа (например, открытое в ней окно консоли). Всё отменено, файлы на месте.",
                            "Another program is using the folder (for example, a console window opened in it). Everything was cancelled, the files are in place."),
    "Error_cancelled": ("Отменено. Файлы на месте.", "Cancelled. The files are in place."),
}

HEAD = '''<?xml version="1.0" encoding="utf-8"?>
<root>
  <resheader name="resmimetype"><value>text/microsoft-resx</value></resheader>
  <resheader name="version"><value>2.0</value></resheader>
  <resheader name="reader"><value>System.Resources.ResXResourceReader, System.Windows.Forms, Version=4.0.0.0, Culture=neutral, PublicKeyToken=b77a5c561934e089</value></resheader>
  <resheader name="writer"><value>System.Resources.ResXResourceWriter, System.Windows.Forms, Version=4.0.0.0, Culture=neutral, PublicKeyToken=b77a5c561934e089</value></resheader>
'''

here = os.path.dirname(os.path.abspath(__file__))
for idx, lang in ((0, "ru-RU"), (1, "en-US")):
    body = "".join(
        f'  <data name="{escape(k)}" xml:space="preserve"><value>{escape(v[idx])}</value></data>\n'
        for k, v in S.items())
    path = os.path.join(here, lang, "Resources.resw")
    with open(path, "w", encoding="utf-8", newline="\r\n") as f:
        f.write(HEAD + body + "</root>\n")
print(len(S), "strings")
