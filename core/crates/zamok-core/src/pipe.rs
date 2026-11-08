//! Именованный канал для окон Zamok: `\\.\pipe\zamok-<SID пользователя>`.
//!
//! Защита:
//! - права канала: полный доступ только текущему пользователю (SYSTEM и администраторы
//!   не перечислены, другие пользователи компьютера подключиться не могут);
//! - `PIPE_REJECT_REMOTE_CLIENTS`: с другого компьютера по сети подключиться нельзя;
//! - `FILE_FLAG_FIRST_PIPE_INSTANCE`: если канал с этим именем уже создал кто-то другой,
//!   core не станет работать рядом с подделкой и завершится с ошибкой;
//! - у канала всегда есть свободный экземпляр core (см. [`Listener::accept`]).
//!
//! Окна со своей стороны проверяют, что на том конце core этого же пользователя
//! (`GetNamedPipeServerProcessId`, затем SID владельца процесса): от процесса, который
//! занял имя до запуска core, права канала не защищают.
//!
//! Сообщения: строка JSON, перевод строки. Буфер чтения стирается после разбора, потому что
//! в нём бывает пароль.

use std::io::{Read, Write};
use std::os::windows::io::FromRawHandle;
use std::sync::Arc;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{LocalFree, ERROR_PIPE_CONNECTED, HANDLE, HLOCAL};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::Storage::FileSystem::{
    FILE_FLAGS_AND_ATTRIBUTES, FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
    PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use zeroize::Zeroizing;

use crate::core::Core;
use crate::protocol::{self, ErrorCode, Request, Response};

/// Больше одной строки такого размера окнам не нужно.
const MAX_MESSAGE: usize = 64 * 1024;

pub fn default_name() -> anyhow::Result<String> {
    Ok(format!(
        r"\\.\pipe\zamok-{}",
        zamok_fs::security::current_user_sid()?
    ))
}

struct Security {
    psd: PSECURITY_DESCRIPTOR,
}

impl Security {
    fn owner_only() -> anyhow::Result<Self> {
        let sid = zamok_fs::security::current_user_sid()?;
        // Владелец и единственная запись доступа: этот пользователь, полный доступ
        let sddl = format!("O:{sid}D:P(A;;GA;;;{sid})");
        let wide: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
        let mut psd = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(wide.as_ptr()),
                SDDL_REVISION_1,
                &mut psd,
                None,
            )?;
        }
        Ok(Security { psd })
    }
}

impl Drop for Security {
    fn drop(&mut self) {
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.psd.0)));
        }
    }
}

/// Канал, к которому ещё никто не подключился.
pub struct Listener {
    name: Vec<u16>,
    security: Security,
    next: Option<HANDLE>,
}

// HANDLE канала можно использовать из любого потока
unsafe impl Send for Listener {}

impl Listener {
    /// Создать первый экземпляр канала. Ошибка, если такой канал уже существует
    /// (запущен другой core или кто-то занял имя).
    pub fn bind(name: &str) -> anyhow::Result<Self> {
        let mut l = Listener {
            name: name.encode_utf16().chain(std::iter::once(0)).collect(),
            security: Security::owner_only()?,
            next: None,
        };
        l.next = Some(l.create(true)?);
        Ok(l)
    }

    fn create(&self, first: bool) -> anyhow::Result<HANDLE> {
        let sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.security.psd.0,
            bInheritHandle: false.into(),
        };
        let mut open = PIPE_ACCESS_DUPLEX;
        if first {
            open |= FILE_FLAG_FIRST_PIPE_INSTANCE;
        }
        let h = unsafe {
            CreateNamedPipeW(
                PCWSTR(self.name.as_ptr()),
                FILE_FLAGS_AND_ATTRIBUTES(open.0),
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                64 * 1024,
                64 * 1024,
                0,
                Some(&sa),
            )
        };
        if h.is_invalid() {
            let e = std::io::Error::last_os_error();
            // 5 (нет доступа) и 231 (канал занят): канал с этим именем уже существует
            if matches!(e.raw_os_error(), Some(5) | Some(231)) {
                anyhow::bail!("the pipe already exists: zamok-core is already running ({e})");
            }
            anyhow::bail!("cannot create the pipe: {e}");
        }
        Ok(h)
    }

    /// Ждать подключения. Возвращает канал к клиенту как `File`.
    ///
    /// Следующий свободный экземпляр создаётся раньше, чем подключённый уходит в работу:
    /// у канала всегда есть наш свободный экземпляр, и чужой процесс не может создать
    /// экземпляр с этим именем в промежутке и выдать себя за core.
    pub fn accept(&mut self) -> anyhow::Result<std::fs::File> {
        let h = match self.next.take() {
            Some(h) => h,
            None => self.create(false)?,
        };
        let r = unsafe { ConnectNamedPipe(h, None) };
        let file = unsafe { std::fs::File::from_raw_handle(h.0 as _) };
        // Даже если создать не вышло, повторим при следующем вызове (next = None)
        self.next = self.create(false).ok();
        if let Err(e) = r {
            // Клиент успел подключиться между созданием и ожиданием: это нормально
            if e.code() != ERROR_PIPE_CONNECTED.to_hresult() {
                return Err(e.into());
            }
        }
        Ok(file)
    }
}

/// Принимать подключения, каждое в своём потоке, пока core не попросили завершиться.
pub fn serve(mut listener: Listener, core: Arc<Core>, on_quit: impl Fn() + Send + Sync + 'static) {
    let on_quit = Arc::new(on_quit);
    loop {
        match listener.accept() {
            Ok(conn) => {
                let core = core.clone();
                let on_quit = on_quit.clone();
                std::thread::spawn(move || {
                    handle(conn, &core);
                    if core.should_quit() {
                        on_quit();
                    }
                });
            }
            Err(e) => {
                tracing::error!(error = %e, "pipe accept failed");
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        }
    }
}

/// Обслужить одного клиента: строки до конца соединения.
fn handle(mut conn: std::fs::File, core: &Arc<Core>) {
    let mut pending: Zeroizing<Vec<u8>> = Zeroizing::new(Vec::with_capacity(4096));
    let mut chunk = Zeroizing::new([0u8; 4096]);
    loop {
        let n = match conn.read(&mut chunk[..]) {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        pending.extend_from_slice(&chunk[..n]);
        chunk.iter_mut().for_each(|b| *b = 0);
        while let Some(pos) = pending.iter().position(|&b| b == b'\n') {
            let reply = {
                let line = &pending[..pos];
                let line = line.strip_suffix(b"\r").unwrap_or(line);
                dispatch(line, core)
            };
            // Стереть обработанную строку (в ней мог быть пароль) и сдвинуть остаток
            pending[..=pos].iter_mut().for_each(|b| *b = 0);
            let rest: Zeroizing<Vec<u8>> = Zeroizing::new(pending[pos + 1..].to_vec());
            pending.iter_mut().for_each(|b| *b = 0);
            pending.clear();
            pending.extend_from_slice(&rest);
            let mut out = serde_json::to_vec(&reply).unwrap_or_default();
            out.push(b'\n');
            if conn.write_all(&out).is_err() {
                return;
            }
            if core.should_quit() {
                return;
            }
        }
        if pending.len() > MAX_MESSAGE {
            let mut out = serde_json::to_vec(&Response::err(ErrorCode::BadRequest)).unwrap();
            out.push(b'\n');
            let _ = conn.write_all(&out);
            return;
        }
    }
}

pub fn dispatch(line: &[u8], core: &Arc<Core>) -> Response {
    let req = match protocol::parse(line) {
        Ok(r) => r,
        Err(code) => return Response::err(code),
    };
    let one = |r: Result<crate::protocol::FolderInfo, ErrorCode>| match r {
        Ok(info) => Response::with_folder(info),
        Err(code) => Response::err(code),
    };
    match req {
        Request::Status => Response {
            ok: true,
            folders: Some(core.status()),
            jobs: Some(core.jobs()),
            ..Default::default()
        },
        Request::Encrypt {
            path,
            password,
            idle_lock_secs,
        } => {
            if core.job_running() {
                return Response::err(ErrorCode::JobRunning);
            }
            let idle = idle_lock_secs.unwrap_or(crate::config::DEFAULT_IDLE_LOCK_SECS);
            let data_dir = core.data_dir().to_path_buf();
            match crate::encrypt::start(
                core.clone(),
                &data_dir,
                std::path::Path::new(&path),
                password,
                idle,
            ) {
                Ok((progress, words)) => {
                    core.add_job(progress.clone());
                    Response {
                        ok: true,
                        job: Some(progress.info()),
                        recovery_key: Some(words.to_string()),
                        ..Default::default()
                    }
                }
                Err(refusal) => Response {
                    ok: false,
                    error: Some(refusal.code),
                    examples: (!refusal.examples.is_empty()).then_some(refusal.examples),
                    ..Default::default()
                },
            }
        }
        Request::JobStatus { id } => match core.job(&id) {
            Some(j) => Response {
                ok: true,
                job: Some(j.info()),
                ..Default::default()
            },
            None => Response::err(ErrorCode::NotFound),
        },
        Request::CancelJob { id } => match core.job(&id) {
            Some(j) if j.request_cancel() => Response::ok(),
            Some(_) => Response::err(ErrorCode::BadRequest),
            None => Response::err(ErrorCode::NotFound),
        },
        Request::Unlock { path, password } => one(core.unlock(&path, &password)),
        Request::UnlockRecovery { path, words } => one(core.unlock_recovery(&path, &words)),
        Request::Lock { path } => one(core.lock(&path)),
        Request::LockAll => {
            core.lock_all("request");
            Response::ok()
        }
        Request::Add {
            path,
            vault,
            idle_lock_secs,
        } => one(core.add(&path, &vault, idle_lock_secs)),
        Request::Remove { path } => match core.remove(&path) {
            Ok(()) => Response::ok(),
            Err(code) => Response::err(code),
        },
        Request::SetIdleLock { path, secs } => one(core.set_idle_lock(&path, secs)),
        Request::ChangePassword {
            path,
            old_password,
            new_password,
        } => one(core.change_password(&path, &old_password, &new_password)),
        Request::ShowRecoveryKey { path, password } => match core.recovery_key(&path, &password) {
            Ok(words) => Response {
                ok: true,
                recovery_key: Some(words.to_string()),
                ..Default::default()
            },
            Err(code) => Response::err(code),
        },
        Request::Quit => {
            core.request_quit();
            Response::ok()
        }
    }
}
