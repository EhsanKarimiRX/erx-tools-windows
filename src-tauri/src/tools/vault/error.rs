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
    #[error(
        "This is not the first part. Select the first part, the file numbered 001 (for example name.001.erx)."
    )]
    NotFirstPart,
    #[error(
        "This file is part of a split set made by an older ERX Tools, which named the parts like name.erx.001. Rename each part to the new style (name.erx.001 becomes name.001.erx, name.erx.002 becomes name.002.erx, and so on) and then select name.001.erx."
    )]
    OldStylePartName,
    #[error("Part {index} is missing.")]
    MissingPart { index: u32 },
    #[error("Part {index} belongs to a different file or is in the wrong place.")]
    WrongPart { index: u32 },
    #[error("A file can have at most 999 parts. Choose a larger part size.")]
    TooManyParts,
    #[error("There is not enough free disk space.")]
    DiskFull,
    #[error(
        "The check after writing failed. The new file was removed; your original is untouched. Try again, or choose another drive."
    )]
    VerifyFailed,
    #[error("Another vault operation is already running.")]
    Busy,
    #[error("The folder cannot be used.")]
    BadFolder,
    #[error(
        "\"{path}\" is a link, device or other special item and cannot be encrypted. Remove it from the folder and try again."
    )]
    UnsupportedEntry { path: String },
    #[error("\"{path}\" has a name that cannot be stored safely. Rename it and try again.")]
    UnsupportedName { path: String },
    #[error(
        "\"{path}\" could not be read. Check that no program is using it and that you may open it, then try again."
    )]
    CannotReadEntry { path: String },
    #[error(
        "\"{path}\" changed while it was being encrypted. Close the programs that use it and try again."
    )]
    FileChanged { path: String },
    #[error(
        "The folder has too many nested levels or too many items. Compress it into a ZIP file first, then encrypt the ZIP file."
    )]
    FolderTooBig,
    #[error(
        "The output folder is inside the folder you are encrypting. Choose another output folder."
    )]
    OutputInsideFolder,
    #[error("The encrypted folder contains an unsafe or duplicate path, so it was not restored.")]
    UnsafeArchive,
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
