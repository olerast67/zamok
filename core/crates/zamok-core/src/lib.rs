//! Фоновый процесс Zamok (`zamok-core`): держит файловые системы всех зашифрованных папок,
//! запирает их по событиям и по времени, принимает команды окон по именованному каналу.
//!
//! - [`config`]: список папок в `%LOCALAPPDATA%\Zamok\folders.json`.
//! - [`mountpoint`]: мёртвые точки подключения после выключения питания.
//! - [`core`]: состояние папок и операции над ними.
//! - [`protocol`], [`pipe`]: команды по каналу, доступному только этому пользователю.
//! - [`preflight`], [`encrypt`]: шифрование существующей папки на месте с журналом.
//! - [`decrypt`]: «Расшифровать навсегда» тем же способом в обратную сторону.
//! - [`autolock`]: блокировка экрана, сон, выход из системы, бездействие.
//! - [`shellverbs`]: пункты «Запереть» и «Открыть паролем» только на папках Zamok.
//! - [`shellhook`]: список папок в общей памяти для `zamok-shell.dll` и её установка.
//! - [`tray`]: значок в трее со списком папок и «Запереть всё».
//! - [`search`]: путь папки вне индекса поиска Windows.
//! - [`howto`]: памятка «КАК ОТКРЫТЬ» рядом с хранилищем.

pub mod autolock;
pub mod clipguard;
pub mod config;
pub mod core;
pub mod decrypt;
pub mod encrypt;
pub mod howto;
pub mod mountpoint;
pub mod pipe;
pub mod preflight;
pub mod protocol;
pub mod recent;
pub mod search;
pub mod shellhook;
pub mod shellverbs;
pub mod tray;
