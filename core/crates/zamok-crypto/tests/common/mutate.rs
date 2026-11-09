//! Общая основа проверок разборщиков: воспроизводимые порчи данных и учёт самой большой
//! запрошенной памяти. Подключается через `#[path]` из тестов разных крейтов.
//!
//! cargo-fuzz здесь не применяется: ему нужен nightly, а на этой машине только stable для
//! Windows MSVC. Число повторов задаёт `ZAMOK_FUZZ_ITERS`, зерно печатается при падении.
#![allow(dead_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Запоминает самый большой запрос памяти за весь прогон.
pub struct Counting;

static LARGEST: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        LARGEST.fetch_max(l.size(), Ordering::Relaxed);
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l)
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        LARGEST.fetch_max(l.size(), Ordering::Relaxed);
        System.alloc_zeroed(l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        LARGEST.fetch_max(new, Ordering::Relaxed);
        System.realloc(p, l, new)
    }
}

/// Предел одного выделения в этих проверках: данные на входе не больше нескольких мегабайт.
pub const ALLOC_LIMIT: usize = 32 * 1024 * 1024;

pub fn assert_no_huge_alloc() {
    let largest = LARGEST.load(Ordering::Relaxed);
    assert!(
        largest <= ALLOC_LIMIT,
        "a parser asked for {largest} bytes at once"
    );
}

pub fn iters(default: u64) -> u64 {
    std::env::var("ZAMOK_FUZZ_ITERS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// xorshift64*: не для криптографии, только чтобы порчи повторялись от прогона к прогону.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }

    pub fn bytes(&mut self, max: usize) -> Vec<u8> {
        let n = self.below(max + 1);
        (0..n).map(|_| self.next() as u8).collect()
    }
}

const INTERESTING: &[&[u8]] = &[
    b"0",
    b"-1",
    b"4294967295",
    b"18446744073709551615",
    b"99999999999999999999999",
    b"null",
    b"\"\"",
    b"[]",
    b"{}",
    b"=",
    b".",
    b"..",
    b"\\",
    b"\xFF\xFE",
    b"\xD0",
    b"\0",
];

/// Испортить данные одним или несколькими случайными способами.
pub fn mutate(rng: &mut Rng, seed: &[u8]) -> Vec<u8> {
    let mut d = seed.to_vec();
    for _ in 0..=rng.below(4) {
        let len = d.len();
        match rng.below(10) {
            0 if len > 0 => {
                let i = rng.below(len);
                d[i] ^= 1 << rng.below(8);
            }
            1 if len > 0 => {
                let i = rng.below(len);
                d[i] = rng.next() as u8;
            }
            2 => d.truncate(rng.below(len + 1)),
            3 => {
                let extra = rng.bytes(256);
                let i = rng.below(len + 1);
                d.splice(i..i, extra);
            }
            4 if len > 0 => {
                let a = rng.below(len);
                let b = (a + rng.below(64)).min(len);
                d.drain(a..b);
            }
            5 if len > 0 => {
                let a = rng.below(len);
                let b = (a + rng.below(64)).min(len);
                let part = d[a..b].to_vec();
                let i = rng.below(len + 1);
                d.splice(i..i, part);
            }
            6 | 7 => {
                let word = INTERESTING[rng.below(INTERESTING.len())];
                let i = rng.below(len + 1);
                let end = (i + rng.below(12)).min(len);
                d.splice(i..end, word.iter().copied());
            }
            8 => d = rng.bytes(4096),
            _ => {
                // Подмена числа: длины и параметры в JSON
                if let Some(i) = d.iter().position(|b| b.is_ascii_digit()) {
                    let word = INTERESTING[rng.below(4)];
                    let end = (i + 1).min(d.len());
                    d.splice(i..end, word.iter().copied());
                }
            }
        }
    }
    d
}
