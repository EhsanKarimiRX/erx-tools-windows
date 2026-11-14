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
    #[error("The encrypted data is damaged, incomplete (a part may be missing) or was changed.")]
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
    #[error("The output file already exists.")]
    OutputExists,
    #[error("The output would overwrite the file you are working on.")]
    OutputIsSource,
    #[error("The file could not be opened. Check that it exists and is not in use.")]
    CannotOpenFile,
    #[error("This is not the first part. Select the file ending in .001.")]
    NotFirstPart,
    #[error("Part {index} is missing.")]
    MissingPart { index: u32 },
    #[error("Part {index} belongs to a different file or is in the wrong place.")]
    WrongPart { index: u32 },
    #[error("A file can have at most 999 parts. Choose a larger part size.")]
    TooManyParts,
    #[error("There is not enough free disk space.")]
    DiskFull,
    #[error(
        "Not enough free disk space on the destination drive. About {needed_mib} MiB are needed."
    )]
    NotEnoughSpace { needed_mib: u64 },
    #[error(transparent)]
    Passphrase(#[from] PassphraseError),
}

impl From<std::io::Error> for VaultError {
    /// A `VaultError` that travelled through an `io::Error` (readers and
    /// writers must return `io::Error`) is unwrapped again; a full disk
    /// becomes `DiskFull`; everything else is `Io`.
    fn from(error: std::io::Error) -> Self {
        if let Some(inner) = error.get_ref()
            && let Some(vault) = inner.downcast_ref::<VaultError>()
        {
            return vault.clone();
        }
        if error.kind() == std::io::ErrorKind::StorageFull {
            return VaultError::DiskFull;
        }
        VaultError::Io
    }
}
