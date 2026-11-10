//! Видимые строки core и DLL Проводника есть на обоих языках (ROADMAP, этап 6 «Русский и
//! английский»): значок и меню в трее, уведомления, вопросы, памятка «КАК ОТКРЫТЬ» и окно DLL.
//! Строки там парами: `t("русский", "English")` или `if russian { "…" } else { "…" }`.
//! Тест проходит по исходникам: за каждой строкой с кириллицей в том же выражении должна идти
//! строка без кириллицы с теми же подстановками `{…}`. Строки окон проверяют тесты C#
//! (`ui/Zamok.Tests/StringUsageTests.cs`).

use std::path::{Path, PathBuf};

/// Строковый литерал: начало, конец (после закрывающей кавычки) и содержимое.
struct Lit {
    start: usize,
    end: usize,
    text: String,
}

/// Литералы Rust вне комментариев: обычные, сырые `r"…"` и `r#"…"#`, символы `'"'` пропускаются.
fn literals(src: &str) -> Vec<Lit> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                    i += 1;
                }
                i += 2;
            }
            b'\'' => {
                // Символ ('"', '\'', 'x') или время жизни ('a): у символа кавычка через 1-2 знака
                if b.get(i + 2) == Some(&b'\'') {
                    i += 3;
                } else if b.get(i + 1) == Some(&b'\\') && b.get(i + 3) == Some(&b'\'') {
                    i += 4;
                } else {
                    i += 1;
                }
            }
            b'r' if (b.get(i + 1) == Some(&b'"') || b.get(i + 1) == Some(&b'#'))
                && (i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_')) =>
            {
                let start = i;
                let mut j = i + 1;
                let mut hashes = 0;
                while b.get(j) == Some(&b'#') {
                    hashes += 1;
                    j += 1;
                }
                if b.get(j) != Some(&b'"') {
                    i += 1;
                    continue;
                }
                let close: String = std::iter::once('"')
                    .chain(std::iter::repeat_n('#', hashes))
                    .collect();
                let body_start = j + 1;
                let body_end = src[body_start..]
                    .find(&close)
                    .map(|p| body_start + p)
                    .expect("raw string is closed");
                out.push(Lit {
                    start,
                    end: body_end + close.len(),
                    text: src[body_start..body_end].to_string(),
                });
                i = body_end + close.len();
            }
            b'"' => {
                let start = i;
                let mut j = i + 1;
                while j < b.len() && b[j] != b'"' {
                    if b[j] == b'\\' {
                        j += 1;
                    }
                    j += 1;
                }
                out.push(Lit {
                    start,
                    end: j + 1,
                    text: src[start + 1..j].to_string(),
                });
                i = j + 1;
            }
            _ => i += 1,
        }
    }
    out
}

fn cyrillic(s: &str) -> bool {
    s.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c))
}

/// Подстановки `{name}`, `{}`, `{0}` без `{{` и без знаков `\u{201c}`.
fn holes(s: &str) -> Vec<String> {
    let mut v = Vec::new();
    let mut rest = s.replace("{{", "").replace("}}", "");
    while let Some(a) = rest.find("\\u{") {
        let b = rest[a..].find('}').map_or(rest.len(), |b| a + b + 1);
        rest.replace_range(a..b, "");
    }
    while let Some(a) = rest.find('{') {
        let Some(b) = rest[a..].find('}') else { break };
        v.push(
            rest[a + 1..a + b]
                .split(':')
                .next()
                .unwrap_or("")
                .to_string(),
        );
        rest = rest[a + b + 1..].to_string();
    }
    v.sort();
    v
}

/// Исходник без модулей тестов: в них кириллица в именах папок, это не текст для человека.
fn without_tests(src: &str) -> &str {
    match src.find("#[cfg(test)]") {
        Some(p) => &src[..p],
        None => src,
    }
}

fn sources() -> Vec<PathBuf> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    for dir in ["zamok-core/src", "zamok-shell/src", "zamok-recover/src"] {
        for e in std::fs::read_dir(crates.join(dir)).unwrap() {
            let p = e.unwrap().path();
            if p.extension().is_some_and(|x| x == "rs") {
                files.push(p);
            }
        }
    }
    files
}

/// Ошибки пары: русская строка без английской после неё или с другими подстановками.
fn check(src: &str) -> Vec<String> {
    let src = without_tests(src);
    let lits = literals(src);
    let mut bad = Vec::new();
    for (k, l) in lits.iter().enumerate() {
        if !cyrillic(&l.text) {
            continue;
        }
        // Английская пара в том же выражении: до неё нет `;`
        let pair = lits[k + 1..].iter().find(|n| !cyrillic(&n.text));
        let line = src[..l.start].lines().count();
        match pair {
            Some(n) if !src[l.end..n.start].contains(';') => {
                if !n.text.chars().any(|c| c.is_ascii_alphabetic()) {
                    bad.push(format!("line {line}: the pair has no English text"));
                } else if holes(&l.text) != holes(&n.text) {
                    bad.push(format!(
                        "line {line}: {:?} and {:?} differ",
                        holes(&l.text),
                        holes(&n.text)
                    ));
                }
            }
            _ => bad.push(format!("line {line}: Russian text without English")),
        }
    }
    bad
}

#[test]
fn every_russian_text_has_an_english_pair() {
    let mut bad = Vec::new();
    let mut pairs = 0;
    for f in sources() {
        let src = std::fs::read_to_string(&f).unwrap();
        pairs += literals(without_tests(&src))
            .iter()
            .filter(|l| cyrillic(&l.text))
            .count();
        for b in check(&src) {
            bad.push(format!("{}: {b}", f.file_name().unwrap().to_string_lossy()));
        }
    }
    assert!(pairs >= 20, "found only {pairs} Russian texts");
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn the_check_finds_a_text_without_a_pair() {
    assert!(check(r#"fn a() { t("Привет", "Hello"); }"#).is_empty());
    assert!(check(
        r#"fn a() { if ru { format!("Папка {name}") } else { format!("Folder {name}") } }"#
    )
    .is_empty());
    assert_eq!(
        check(r#"fn a() { notify("Только по-русски"); let x = "en"; }"#).len(),
        1
    );
    assert_eq!(
        check(r#"fn a() { t("Папка {name}", "Folder {}"); }"#).len(),
        1
    );
    assert!(
        check("fn a() {}\n#[cfg(test)]\nmod tests { const X: &str = \"Клиенты\"; }").is_empty()
    );
}
