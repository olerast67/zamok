//! Команды по именованному каналу: одна строка JSON на запрос, одна строка на ответ.
//!
//! ```text
//! → {"cmd":"status"}
//! ← {"ok":true,"folders":[{"id":"1a2b3c4d","name":"Клиенты","path":"C:\\…","state":"locked"}]}
//! → {"cmd":"unlock","path":"C:\\…\\Клиенты","password":"…"}
//! ← {"ok":false,"error":"wrong_password"}
//! ```
//!
//! `path` может указывать и на что-то внутри папки (значок `Открыть Клиенты.zamok`).

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

#[derive(Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    /// Список папок и их состояние.
    Status,
    Unlock {
        path: String,
        password: Zeroizing<String>,
    },
    /// Открыть ключом восстановления (44 слова).
    UnlockRecovery {
        path: String,
        words: Zeroizing<String>,
    },
    Lock {
        path: String,
    },
    LockAll,
    /// Добавить готовое хранилище и подключить его на путь (пути не должно существовать).
    Add {
        path: String,
        vault: String,
        #[serde(default)]
        idle_lock_secs: Option<u64>,
    },
    /// Отключить папку и убрать её из списка. Зашифрованные данные не трогаются.
    Remove {
        path: String,
    },
    /// Через сколько секунд без работы запирать папку (0 = не запирать по времени).
    SetIdleLock {
        path: String,
        secs: u64,
    },
    /// Зашифровать существующую папку на месте. Ответ сразу: номер работы и ключ
    /// восстановления; копирование идёт в фоне (`job_status`).
    Encrypt {
        path: String,
        password: Zeroizing<String>,
        #[serde(default)]
        idle_lock_secs: Option<u64>,
    },
    JobStatus {
        id: String,
    },
    /// Отменить шифрование. Возможно только до подмены папки; дальше работа доводится до конца.
    CancelJob {
        id: String,
    },
    /// Сменить пароль. Мастер-ключ тот же, поэтому ключ восстановления остаётся прежним.
    ChangePassword {
        path: String,
        old_password: Zeroizing<String>,
        new_password: Zeroizing<String>,
    },
    /// Показать ключ восстановления (после ввода пароля).
    ShowRecoveryKey {
        path: String,
        password: Zeroizing<String>,
    },
    /// Расшифровать навсегда: папка снова станет обычной. Ответ сразу, ход в `job_status`.
    Decrypt {
        path: String,
        password: Zeroizing<String>,
    },
    /// Общие настройки: запирать ли при блокировке экрана.
    SetLockOnScreenLock {
        on: bool,
    },
    /// Включить вход по Windows Hello: после проверки пароля мастер-ключ заворачивается ключом
    /// из подписи Windows Hello (окно получает её после PIN-кода или отпечатка).
    HelloEnable {
        path: String,
        password: Zeroizing<String>,
        /// 32 байта в base64.
        wrap_key: Zeroizing<String>,
    },
    HelloDisable {
        path: String,
    },
    /// Открыть ключом из подписи Windows Hello.
    UnlockHello {
        path: String,
        wrap_key: Zeroizing<String>,
    },
    /// Запереть всё, отключить папки и завершиться.
    Quit,
}

/// Коды ошибок для окон: окно само выбирает человеческий текст по коду.
#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    BadRequest,
    NotFound,
    WrongPassword,
    WrongRecoveryKey,
    AlreadyAdded,
    /// На пути обычная папка или файл.
    PathOccupied,
    /// Нет хранилища по указанному пути или оно повреждено.
    BadVault,
    MountFailed,
    // Проверки перед шифрованием (в ответе бывают примеры: относительные пути)
    /// Не папка: файл, ссылка или точка подключения.
    NotAPlainFolder,
    /// Системная папка, корень диска, профиль или его известные папки.
    SystemFolder,
    /// Папка облачного клиента (OneDrive, Dropbox, Яндекс.Диск).
    CloudFolder,
    NotNtfs,
    /// Рядом уже есть `.Имя.zamok`, `.zamok.tmp` или `.zamok-original`.
    VaultExists,
    /// Ссылки, облачные заглушки, неподходящие имена, слишком глубокие пути.
    UnsupportedItems,
    /// Файлы открыты в других программах.
    FilesInUse,
    /// Файлы или папки нельзя прочитать.
    Unreadable,
    NotEnoughSpace,
    // Шифрование
    /// Уже идёт другое шифрование.
    JobRunning,
    /// Прошлое шифрование не завершено (журнал остался): core доведёт его при перезапуске.
    RecoveryPending,
    /// Что-то в папке изменилось во время копирования: всё отменено, оригинал цел.
    ChangedDuringCopy,
    /// Расшифрованная копия не совпала с оригиналом: всё отменено, оригинал цел.
    VerifyFailed,
    /// Не вышло записать копию (диск, права): всё отменено, оригинал цел.
    CopyFailed,
    /// Папку держит другая программа, её нельзя переименовать: всё отменено, оригинал цел.
    FolderInUse,
    /// Отменено человеком: оригинал цел.
    Cancelled,
    /// Windows Hello не подошёл или не включён для этой папки.
    HelloFailed,
    /// Папку заперли (Win+L, сон, «запереть всё»), пока проверялся пароль.
    UnlockCancelled,
    Internal,
}

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FolderState {
    Locked,
    Unlocked,
    /// Не подключена (диск отсутствует, путь занят и т. п.); подробности в `error`.
    Unavailable,
}

#[derive(Serialize, Debug, Clone)]
pub struct FolderInfo {
    pub id: String,
    pub name: String,
    pub path: String,
    pub state: FolderState,
    pub idle_lock_secs: u64,
    /// Включён вход по Windows Hello.
    pub hello: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorCode>,
}

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Running,
    Done,
    Failed,
}

#[derive(Serialize, Debug, Clone)]
pub struct JobInfo {
    pub id: String,
    pub path: String,
    pub state: JobState,
    /// Шаг из журнала: copying, verified, vault_placed, original_aside, registered.
    pub phase: String,
    pub files_done: u64,
    pub files_total: u64,
    pub bytes_done: u64,
    pub bytes_total: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorCode>,
}

#[derive(Serialize, Debug, Clone)]
pub struct Settings {
    pub lock_on_screen_lock: bool,
}

#[derive(Serialize, Debug, Default)]
pub struct Response {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorCode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folders: Option<Vec<FolderInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<FolderInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job: Option<JobInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jobs: Option<Vec<JobInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<Settings>,
    /// Ключ восстановления, 44 слова: только в ответе на `encrypt`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_key: Option<zeroize::Zeroizing<String>>,
    /// Сколько папок ждут удаления открытого оригинала (его держит другая программа).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub originals_left: Option<usize>,
    /// Примеры того, что мешает (относительные пути), при отказе проверок.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub examples: Option<Vec<String>>,
}

impl Response {
    pub fn ok() -> Self {
        Response {
            ok: true,
            ..Default::default()
        }
    }

    pub fn err(code: ErrorCode) -> Self {
        Response {
            ok: false,
            error: Some(code),
            ..Default::default()
        }
    }

    pub fn with_folder(info: FolderInfo) -> Self {
        Response {
            ok: true,
            folder: Some(info),
            ..Default::default()
        }
    }
}

pub fn parse(line: &[u8]) -> Result<Request, ErrorCode> {
    serde_json::from_slice(line).map_err(|_| ErrorCode::BadRequest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commands() {
        assert!(matches!(parse(br#"{"cmd":"status"}"#), Ok(Request::Status)));
        assert!(matches!(
            parse(br#"{"cmd":"lock_all"}"#),
            Ok(Request::LockAll)
        ));
        match parse(r#"{"cmd":"unlock","path":"C:\\Клиенты","password":"пароль"}"#.as_bytes())
        {
            Ok(Request::Unlock { path, password }) => {
                assert_eq!(path, r"C:\Клиенты");
                assert_eq!(password.as_str(), "пароль");
            }
            _ => panic!(),
        }
        match parse(br#"{"cmd":"add","path":"C:\\a","vault":"C:\\.a.zamok"}"#) {
            Ok(Request::Add { idle_lock_secs, .. }) => assert_eq!(idle_lock_secs, None),
            _ => panic!(),
        }
    }

    #[test]
    fn rejects_garbage_unknown_commands_and_fields() {
        for bad in [
            &b"not json"[..],
            br#"{"cmd":"format_c"}"#,
            br#"{"cmd":"unlock","path":"C:\\a"}"#,
            br#"{"cmd":"lock","path":"C:\\a","extra":1}"#,
        ] {
            assert_eq!(parse(bad).err(), Some(ErrorCode::BadRequest));
        }
    }

    #[test]
    fn response_shape() {
        let s = serde_json::to_string(&Response::err(ErrorCode::WrongPassword)).unwrap();
        assert_eq!(s, r#"{"ok":false,"error":"wrong_password"}"#);
        assert_eq!(
            serde_json::to_string(&Response::ok()).unwrap(),
            r#"{"ok":true}"#
        );
    }
}
