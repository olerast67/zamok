//! Память для ключей, которую Windows не выгружает в файл подкачки.
//!
//! Каждое значение лежит в своей странице: `VirtualUnlock` снимает запрет со всей страницы,
//! поэтому делить её с другими данными нельзя. Страница выделена в куче (`Box`), при перемещении
//! [`Locked`] перемещается только указатель: адрес ключа не меняется, пока он заперт.
//!
//! Запретить выгрузку удаётся не всегда: число запертых страниц ограничено минимальным рабочим
//! набором процесса. При отказе рабочий набор увеличивается, а если и это не помогло, ключ
//! работает без запрета и в журнал один раз пишется предупреждение (без имён и путей).
//!
//! Чего это не закрывает: промежуточные копии внутри RustCrypto (раунды AES на стеке) и память
//! scrypt, ключи отдельных файлов (их может быть больше, чем разрешено запереть страниц),
//! пароль в разборе JSON. Эти данные стираются, но могут попасть в файл подкачки.

use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicBool, Ordering};

use zeroize::{Zeroize, ZeroizeOnDrop};

/// Размер страницы Windows на x86-64 и ARM64.
const PAGE: usize = 4096;

#[repr(align(4096))]
struct Page([u8; PAGE]);

/// `N` байт секрета в отдельной запертой странице. Стирается и отпирается при удалении.
pub struct Locked<const N: usize> {
    page: Box<Page>,
    locked: bool,
}

impl<const N: usize> Locked<N> {
    /// Новый буфер из нулей.
    pub fn new() -> Self {
        const { assert!(N <= PAGE, "secret does not fit in one page") };
        let page = Box::new(Page([0; PAGE]));
        let locked = os::lock(page.0.as_ptr());
        static WARNED: AtomicBool = AtomicBool::new(false);
        if !locked && !WARNED.swap(true, Ordering::Relaxed) {
            tracing::warn!("key memory could not be locked; keys may reach the page file");
        }
        Locked { page, locked }
    }

    /// Запрещена ли выгрузка этой страницы.
    pub fn is_locked(&self) -> bool {
        self.locked
    }

    fn release(&mut self) {
        // Сначала стереть, потом отпереть: без запрета страницу могут выгрузить в любой момент
        self.page.0.zeroize();
        if self.locked {
            os::unlock(self.page.0.as_ptr());
            self.locked = false;
        }
    }
}

impl<const N: usize> Default for Locked<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Deref for Locked<N> {
    type Target = [u8; N];
    fn deref(&self) -> &[u8; N] {
        self.page.0.first_chunk().expect("N is at most one page")
    }
}

impl<const N: usize> DerefMut for Locked<N> {
    fn deref_mut(&mut self) -> &mut [u8; N] {
        self.page
            .0
            .first_chunk_mut()
            .expect("N is at most one page")
    }
}

impl<const N: usize> Clone for Locked<N> {
    fn clone(&self) -> Self {
        let mut copy = Self::new();
        *copy = **self;
        copy
    }
}

impl<const N: usize> PartialEq for Locked<N> {
    fn eq(&self, other: &Self) -> bool {
        subtle::ConstantTimeEq::ct_eq(&self[..], &other[..]).into()
    }
}

impl<const N: usize> Eq for Locked<N> {}

impl<const N: usize> std::fmt::Debug for Locked<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Locked(..)")
    }
}

impl<const N: usize> Drop for Locked<N> {
    fn drop(&mut self) {
        self.release();
    }
}

impl<const N: usize> ZeroizeOnDrop for Locked<N> {}

#[cfg(windows)]
mod os {
    use std::ffi::c_void;
    use std::sync::Mutex;

    use windows::Win32::System::Memory::{VirtualLock, VirtualUnlock};
    use windows::Win32::System::Threading::{
        GetCurrentProcess, GetProcessWorkingSetSize, SetProcessWorkingSetSize,
    };

    use super::PAGE;

    /// На сколько увеличивать рабочий набор при нехватке (256 страниц ключей).
    const GROW: usize = 1024 * 1024;
    /// Чтение и запись размера рабочего набора из разных потоков не должны перепутаться.
    static RAISE: Mutex<()> = Mutex::new(());

    pub fn lock(page: *const u8) -> bool {
        let page = page as *const c_void;
        // SAFETY: page указывает на выделенную страницу длиной PAGE, принадлежащую вызывающему
        unsafe {
            VirtualLock(page, PAGE).is_ok()
                || (raise_working_set() && VirtualLock(page, PAGE).is_ok())
        }
    }

    pub fn unlock(page: *const u8) {
        // SAFETY: page указывает на выделенную страницу длиной PAGE, принадлежащую вызывающему
        let _ = unsafe { VirtualUnlock(page as *const c_void, PAGE) };
    }

    fn raise_working_set() -> bool {
        let _guard = RAISE.lock().unwrap_or_else(|e| e.into_inner());
        let (mut min, mut max) = (0usize, 0usize);
        // SAFETY: указатели на локальные переменные, описатель текущего процесса не закрывается
        unsafe {
            let process = GetCurrentProcess();
            GetProcessWorkingSetSize(process, &mut min, &mut max).is_ok()
                && SetProcessWorkingSetSize(process, min + GROW, max + GROW).is_ok()
        }
    }
}

#[cfg(not(windows))]
mod os {
    pub fn lock(_page: *const u8) -> bool {
        false
    }

    pub fn unlock(_page: *const u8) {}
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use windows::Win32::System::Memory::{VirtualLock, VirtualUnlock};

    /// Спросить у Windows, заперта ли страница: второй `VirtualUnlock` подряд вернул бы отказ.
    fn really_locked<const N: usize>(b: &Locked<N>) -> bool {
        let p = b.page.0.as_ptr() as *const std::ffi::c_void;
        unsafe {
            let was = VirtualUnlock(p, PAGE).is_ok();
            let _ = VirtualLock(p, PAGE);
            was
        }
    }

    #[test]
    fn page_is_locked_and_address_is_stable() {
        let mut b = Locked::<64>::new();
        assert!(b.is_locked());
        assert!(really_locked(&b));
        b[0] = 7;
        let before = b.as_ptr();
        let moved = b; // перемещается только указатель на страницу
        assert_eq!(moved.as_ptr(), before);
        assert_eq!(moved.as_ptr() as usize % PAGE, 0);
        assert_eq!(moved[0], 7);
    }

    #[test]
    fn release_wipes_the_page_before_unlocking_it() {
        let mut b = Locked::<32>::new();
        b.fill(0xAA);
        let p = b.page.0.as_ptr() as *const std::ffi::c_void;
        b.release();
        assert!(!b.is_locked());
        assert!(b.page.0.iter().all(|x| *x == 0));
        // Запрета уже нет: у Windows нечего снимать
        assert!(unsafe { VirtualUnlock(p, PAGE) }.is_err());
    }

    #[test]
    fn more_keys_than_the_default_quota_are_still_locked() {
        // Минимальный рабочий набор по умолчанию (200 КиБ) допускает около 50 страниц
        let keys: Vec<Locked<64>> = (0..600).map(|_| Locked::new()).collect();
        assert!(keys.iter().filter(|k| k.is_locked()).count() > 100);
        assert!(keys.iter().all(|k| k.iter().all(|x| *x == 0)));
    }

    #[test]
    fn equal_and_clone() {
        let mut a = Locked::<8>::new();
        a[3] = 1;
        let b = a.clone();
        assert_eq!(a, b);
        assert_ne!(a.as_ptr(), b.as_ptr());
        assert_eq!(format!("{a:?}"), "Locked(..)");
    }
}
