//! Errors of the Secure Notes container. Every message is shown to the user,
//! so each one is plain English and never contains words, keys or notes.

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NotesError {
    #[error("This file is not an ERX Secure Notes file.")]
    NotANotesFile,
    #[error("This notes file needs a different version of ERX Tools.")]
    UnsupportedVersion,
    #[error("The notes file is damaged.")]
    DamagedFile,
    #[error("The words are wrong, or the notes file is damaged.")]
    WrongPassphraseOrHeader,
    #[error("The notes file has invalid key settings.")]
    InvalidKdfParams,
    #[error("Could not derive the key from the words.")]
    KeyDerivationFailed,
    #[error("The notes file is too large.")]
    FileTooLarge,
    #[error("The notes are too large to save.")]
    NotesTooLarge,
    #[error("Could not get secure random numbers from Windows.")]
    RandomUnavailable,
    #[error("Internal error in Secure Notes.")]
    Internal,
    #[error("The notes data is not valid.")]
    DataInvalid,
    #[error("These notes were saved by a newer version of ERX Tools.")]
    DataTooNew,
    #[error("This note is too large.")]
    NoteTooLarge,
    #[error("The title is too long.")]
    TitleTooLong,
    #[error("There are too many notes.")]
    TooManyNotes,
    #[error("That note was not found.")]
    NoteNotFound,
    #[error("The notes file could not be read or written.")]
    Io,
    #[error("No notes file was found.")]
    NoNotesFile,
    #[error("A notes file already exists.")]
    AlreadyExists,
    #[error("The saved notes did not pass the safety check, so they were not used.")]
    VerifyFailed,
    #[error("The notes are locked.")]
    NotUnlocked,
    #[error("The notes are already unlocked.")]
    AlreadyUnlocked,
    #[error("Confirm that you saved the words first.")]
    WordsNotConfirmed,
    #[error("Type the words again to confirm that you saved them.")]
    WordsNotRetyped,
    #[error("{0}")]
    Words(String),
    #[error("Those words did not open this file.")]
    WrongImportPassphrase,
    #[error("There is no import waiting. Choose the file again.")]
    NoPendingImport,
}
