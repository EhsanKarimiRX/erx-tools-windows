//! Errors of the vault engine. Every message is shown to the user, so each
//! one is plain English and never contains words, keys or file contents.

use super::passphrase::PassphraseError;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VaultError {
    #[error("This file is not an ERX vault file.")]
    NotAVaultFile,
    #[error("This file was made by a newer version of ERX Tools. Please update the app.")]
    UnsupportedVersion,
    #[error("The file header is damaged.")]
    CorruptedHeader,
    #[error("Wrong passphrase, or the file header was changed.")]
    WrongPassphraseOrHeader,
    #[error("The encrypted data is damaged, incomplete or was changed.")]
    CorruptedData,
    #[error("The restored file does not match the original (integrity check failed).")]
    IntegrityMismatch,
    #[error("The operation was cancelled.")]
    Cancelled,
    #[error("Could not derive the key from the passphrase.")]
    KeyDerivationFailed,
    #[error("Windows could not provide secure random numbers.")]
    RandomUnavailable,
    #[error("The security settings are not allowed.")]
    InvalidKdfParams,
    #[error("Reading or writing the file failed.")]
    Io,
    #[error("An internal encryption error occurred.")]
    Internal,
    #[error(transparent)]
    Passphrase(#[from] PassphraseError),
}

impl From<std::io::Error> for VaultError {
    /// A `VaultError` that travelled through an `io::Error` (writers must
    /// return `io::Error`) is unwrapped again; everything else is `Io`.
    fn from(error: std::io::Error) -> Self {
        if let Some(inner) = error.get_ref()
            && let Some(vault) = inner.downcast_ref::<VaultError>()
        {
            return vault.clone();
        }
        VaultError::Io
    }
}
