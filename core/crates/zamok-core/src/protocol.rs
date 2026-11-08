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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorCode>,
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
