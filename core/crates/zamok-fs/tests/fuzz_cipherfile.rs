//! Испорченный зашифрованный файл через произвольный доступ: чтение, запись и смена размера
//! не паникуют, а неподдельные данные не выдаются за настоящие.

#[path = "../../zamok-crypto/tests/common/mutate.rs"]
mod mutate;

use mutate::{assert_no_huge_alloc, iters, mutate as spoil, Counting, Rng};
use zamok_crypto::content::{self, PAYLOAD_LEN};
use zamok_crypto::MasterKey;
use zamok_fs::cipherfile::CipherFile;

#[global_allocator]
static ALLOC: Counting = Counting;

#[test]
fn damaged_cipher_files() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("f.c9r");
    let key = MasterKey::generate().unwrap();
    let body: Vec<u8> = (0..PAYLOAD_LEN * 2 + 500).map(|i| i as u8).collect();
    let good = content::encrypt_file(&key, &body).unwrap();
    let mut rng = Rng::new(21);
    for i in 0..iters(1500) {
        let bytes = spoil(&mut rng, &good);
        std::fs::write(&path, &bytes).unwrap();
        let Ok(mut f) = CipherFile::open(&path, &key) else {
            continue;
        };
        let size = f.len().unwrap_or(0);
        let mut buf = vec![0u8; rng.below(PAYLOAD_LEN * 3)];
        let offset = rng.next() % (size + 2);
        if let Ok(n) = f.read_at(offset, &mut buf) {
            let start = offset as usize;
            assert!(
                body.get(start..start + n) == Some(&buf[..n]),
                "iteration {i}: damaged data came back as real"
            );
        }
        match rng.below(3) {
            0 => {
                let _ = f.write_at(rng.next() % (size + 2), &[1, 2, 3]);
            }
            1 => {
                let _ = f.set_len(rng.next() % (size + 2));
            }
            _ => {}
        }
        drop(f);
        // Файл открыт на запись только здесь, дескриптор закрыт до следующего круга
    }
    assert_no_huge_alloc();
}
