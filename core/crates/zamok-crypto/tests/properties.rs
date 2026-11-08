//! Проверки на случайных данных: для любых имён и любого содержимого
//! «зашифровать, затем расшифровать» возвращает исходное.
use proptest::prelude::*;
use zamok_crypto::{content, names, MasterKey};

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn names_roundtrip(name in "[^/\\\\\\x00]{1,120}", parent in "[a-f0-9-]{0,36}") {
        let k = MasterKey::generate().unwrap();
        let enc = names::encrypt_name(&k, &name, &parent);
        let dec = names::decrypt_name(&k, &enc, &parent).unwrap();
        // имена приводятся к NFC, поэтому сравниваем с нормализованным
        use unicode_normalization::UnicodeNormalization;
        prop_assert_eq!(dec, name.nfc().collect::<String>());
    }

    #[test]
    fn content_roundtrip(data in proptest::collection::vec(any::<u8>(), 0..100_000)) {
        let k = MasterKey::generate().unwrap();
        let ct = content::encrypt_file(&k, &data).unwrap();
        prop_assert_eq!(content::cleartext_size(ct.len() as u64).unwrap(), data.len() as u64);
        prop_assert_eq!(&*content::decrypt_file(&k, &ct).unwrap(), &data);
    }

    #[test]
    fn garbage_never_panics(data in proptest::collection::vec(any::<u8>(), 0..2_000)) {
        let k = MasterKey::generate().unwrap();
        let _ = content::decrypt_file(&k, &data);
        let _ = names::decrypt_name(&k, &String::from_utf8_lossy(&data), "");
    }
}
