use thiserror::Error;

/// Ошибки криптоядра. Сообщения не содержат имён файлов, путей и ключей:
/// они могут попасть в журнал программы.
#[derive(Debug, Error)]
pub enum Error {
    #[error("wrong password or damaged masterkey file")]
    WrongPassword,
    #[error("masterkey file is malformed")]
    MalformedMasterkey,
    #[error("unsupported scrypt parameters")]
    UnsupportedScrypt,
    #[error("vault configuration is malformed or not signed by this key")]
    BadVaultConfig,
    #[error("unsupported vault format or cipher combo")]
    UnsupportedFormat,
    #[error("encrypted data is damaged or was changed")]
    Authentication,
    #[error("encrypted data has invalid length")]
    BadLength,
    #[error("recovery key is wrong: unknown word, wrong number of words or checksum mismatch")]
    BadRecoveryKey,
    #[error("operating system random number generator failed")]
    Random,
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
