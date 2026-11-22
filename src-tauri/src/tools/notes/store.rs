//! Saving and loading the notes file on disk: atomic saves, one backup and
//! recovery. Every operation works on one folder chosen by the caller.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime};

use super::crypto::{CreateSettings, NotesKeys, create, open};
use super::error::NotesError;
use super::format::MAX_FILE_BYTES;
use super::model::NotesData;

/// The current notes file.
pub const MAIN_FILE_NAME: &str = "secure-notes.erxnotes";
/// The previous version of the notes file.
pub const BACKUP_FILE_NAME: &str = "secure-notes.erxnotes.bak";
/// Extension of the temporary files written next to the notes file.
pub const TEMP_EXTENSION: &str = "erxtmp";
/// Temporary files older than this are removed at the next save.
pub const STALE_TEMP_AGE: Duration = Duration::from_secs(3600);

/// File operations that the store needs. Tests replace this to script errors.
pub trait FsOps: Send + Sync {
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn remove_file(&self, path: &Path) -> io::Result<()>;
}

/// The real file system.
pub struct RealFs;

impl FsOps for RealFs {
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }
}

/// How often a rename or a remove is retried, and how long to wait between
/// tries. Windows sometimes keeps a file busy for a short moment.
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub attempts: u32,
    pub delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            attempts: 5,
            delay: Duration::from_millis(100),
        }
    }
}

fn rename_retry(ops: &dyn FsOps, from: &Path, to: &Path, retry: RetryPolicy) -> io::Result<()> {
    let attempts = retry.attempts.max(1);
    let mut last = None;
    for attempt in 0..attempts {
        match ops.rename(from, to) {
            Ok(()) => return Ok(()),
            Err(error) => {
                last = Some(error);
                if attempt + 1 < attempts {
                    thread::sleep(retry.delay);
                }
            }
        }
    }
    Err(last.unwrap_or_else(|| io::Error::other("rename was not attempted")))
}

fn remove_retry(ops: &dyn FsOps, path: &Path, retry: RetryPolicy) -> io::Result<()> {
    let attempts = retry.attempts.max(1);
    let mut last = None;
    for attempt in 0..attempts {
        match ops.remove_file(path) {
            Ok(()) => return Ok(()),
            Err(error) => {
                last = Some(error);
                if attempt + 1 < attempts {
                    thread::sleep(retry.delay);
                }
            }
        }
    }
    Err(last.unwrap_or_else(|| io::Error::other("remove was not attempted")))
}

/// The notes files of one folder.
pub struct NotesStore {
    dir: PathBuf,
    ops: Box<dyn FsOps>,
    retry: RetryPolicy,
}

/// How a save treats the previous version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupPolicy {
    /// Move the current main file to the backup name, then put the new file
    /// in place.
    Rotate,
    /// Replace the main file and leave the backup alone.
    KeepBackup,
    /// Replace the main file and write the same bytes as the new backup.
    ReplaceWithNew,
}

/// What a save did with the old backup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveReport {
    pub old_backup_gone: bool,
}

/// Which file a successful load used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadSource {
    Main,
    Backup,
}

/// The keys, the data and the file that was used. The manual `Debug` shows
/// only the source.
pub struct LoadedNotes {
    pub keys: NotesKeys,
    pub data: NotesData,
    pub source: LoadSource,
}

impl std::fmt::Debug for LoadedNotes {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let source = match self.source {
            LoadSource::Main => "Main",
            LoadSource::Backup => "Backup",
        };
        write!(formatter, "LoadedNotes {{ source: {source}, .. }}")
    }
}

impl NotesStore {
    /// A store on the real file system with the default retry policy.
    pub fn new(dir: PathBuf) -> NotesStore {
        NotesStore {
            dir,
            ops: Box::new(RealFs),
            retry: RetryPolicy::default(),
        }
    }

    /// A store with a caller-supplied file system and retry policy.
    pub fn with_ops(dir: PathBuf, ops: Box<dyn FsOps>, retry: RetryPolicy) -> NotesStore {
        NotesStore { dir, ops, retry }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn main_path(&self) -> PathBuf {
        self.dir.join(MAIN_FILE_NAME)
    }

    pub fn backup_path(&self) -> PathBuf {
        self.dir.join(BACKUP_FILE_NAME)
    }

    /// True when the main file or the backup file exists as a file.
    pub fn exists(&self) -> bool {
        self.main_path().is_file() || self.backup_path().is_file()
    }

    /// Removes temporary files of this store that are at least
    /// `STALE_TEMP_AGE` old. Returns how many were removed.
    pub fn cleanup_stale_temp(&self) -> usize {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(_) => return 0,
        };
        let prefix = format!("{MAIN_FILE_NAME}.");
        let suffix = format!(".{TEMP_EXTENSION}");
        let mut removed = 0;
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with(prefix.as_str()) || !name.ends_with(suffix.as_str()) {
                continue;
            }
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            if !metadata.is_file() {
                continue;
            }
            let modified = match metadata.modified() {
                Ok(modified) => modified,
                Err(_) => continue,
            };
            let age = SystemTime::now()
                .duration_since(modified)
                .unwrap_or(Duration::ZERO);
            if age < STALE_TEMP_AGE {
                continue;
            }
            if remove_retry(self.ops.as_ref(), &entry.path(), self.retry).is_ok() {
                removed += 1;
            }
        }
        removed
    }

    fn new_temp_path(&self) -> Result<PathBuf, NotesError> {
        let mut bytes = [0u8; 4];
        getrandom::fill(&mut bytes).map_err(|_| NotesError::RandomUnavailable)?;
        let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        Ok(self
            .dir
            .join(format!("{MAIN_FILE_NAME}.{hex}.{TEMP_EXTENSION}")))
    }

    /// Writes the bytes to a new temporary file, flushes and syncs it, reads
    /// it back and runs `verify`. The temporary file is removed on failure.
    fn write_temp(
        &self,
        file_bytes: &[u8],
        verify: &mut dyn FnMut(&[u8]) -> bool,
    ) -> Result<PathBuf, NotesError> {
        let temp_path = self.new_temp_path()?;
        let write_result = (|| -> io::Result<()> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp_path)?;
            file.write_all(file_bytes)?;
            file.flush()?;
            file.sync_all()?;
            Ok(())
        })();
        if write_result.is_err() {
            let _ = remove_retry(self.ops.as_ref(), &temp_path, self.retry);
            return Err(NotesError::Io);
        }

        let read_back = match fs::read(&temp_path) {
            Ok(bytes) => bytes,
            Err(_) => {
                let _ = remove_retry(self.ops.as_ref(), &temp_path, self.retry);
                return Err(NotesError::Io);
            }
        };
        if read_back != file_bytes || !verify(&read_back) {
            let _ = remove_retry(self.ops.as_ref(), &temp_path, self.retry);
            return Err(NotesError::VerifyFailed);
        }
        Ok(temp_path)
    }

    /// Writes and verifies a temporary file, then moves it into place
    /// according to `policy`.
    pub(crate) fn write_atomic(
        &self,
        file_bytes: &[u8],
        verify: &mut dyn FnMut(&[u8]) -> bool,
        policy: BackupPolicy,
    ) -> Result<SaveReport, NotesError> {
        fs::create_dir_all(&self.dir).map_err(|_| NotesError::Io)?;
        self.cleanup_stale_temp();
        let temp = self.write_temp(file_bytes, verify)?;
        let main = self.main_path();
        let backup = self.backup_path();

        match policy {
            BackupPolicy::Rotate => {
                if main.exists()
                    && rename_retry(self.ops.as_ref(), &main, &backup, self.retry).is_err()
                {
                    let _ = remove_retry(self.ops.as_ref(), &temp, self.retry);
                    return Err(NotesError::Io);
                }
                if rename_retry(self.ops.as_ref(), &temp, &main, self.retry).is_err() {
                    if !main.exists() && backup.exists() {
                        let _ = rename_retry(self.ops.as_ref(), &backup, &main, self.retry);
                    }
                    let _ = remove_retry(self.ops.as_ref(), &temp, self.retry);
                    return Err(NotesError::Io);
                }
                Ok(SaveReport {
                    old_backup_gone: true,
                })
            }
            BackupPolicy::KeepBackup => {
                if rename_retry(self.ops.as_ref(), &temp, &main, self.retry).is_err() {
                    let _ = remove_retry(self.ops.as_ref(), &temp, self.retry);
                    return Err(NotesError::Io);
                }
                Ok(SaveReport {
                    old_backup_gone: true,
                })
            }
            BackupPolicy::ReplaceWithNew => {
                if rename_retry(self.ops.as_ref(), &temp, &main, self.retry).is_err() {
                    let _ = remove_retry(self.ops.as_ref(), &temp, self.retry);
                    return Err(NotesError::Io);
                }
                if let Ok(temp2) = self.write_temp(file_bytes, verify) {
                    if rename_retry(self.ops.as_ref(), &temp2, &backup, self.retry).is_ok() {
                        return Ok(SaveReport {
                            old_backup_gone: true,
                        });
                    }
                    let _ = remove_retry(self.ops.as_ref(), &temp2, self.retry);
                }
                let _ = remove_retry(self.ops.as_ref(), &backup, self.retry);
                Ok(SaveReport {
                    old_backup_gone: !backup.exists(),
                })
            }
        }
    }

    fn read_optional(&self, path: &Path) -> Result<Option<Vec<u8>>, NotesError> {
        let metadata = match fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(NotesError::Io),
        };
        if metadata.len() > MAX_FILE_BYTES as u64 {
            return Err(NotesError::FileTooLarge);
        }
        match fs::read(path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(NotesError::Io),
        }
    }

    fn open_notes(
        bytes: &[u8],
        normalized_words: &str,
    ) -> Result<(NotesKeys, NotesData), NotesError> {
        let (keys, payload) = open(bytes, normalized_words)?;
        let data = NotesData::from_payload(&payload)?;
        Ok((keys, data))
    }

    /// Creates the first notes file in this folder.
    pub fn create_new(
        &self,
        normalized_words: &str,
        settings: &CreateSettings,
        data: &NotesData,
    ) -> Result<NotesKeys, NotesError> {
        if self.exists() {
            return Err(NotesError::AlreadyExists);
        }
        let payload = data.to_payload()?;
        let (file, keys) = create(normalized_words, settings, &payload)?;
        {
            let mut verify = |read: &[u8]| match keys.open_body(read) {
                Ok(opened) => opened.as_slice() == payload.as_slice(),
                Err(_) => false,
            };
            self.write_atomic(&file, &mut verify, BackupPolicy::KeepBackup)?;
        }
        Ok(keys)
    }

    /// Loads the main file, or the backup when the main file is missing or
    /// damaged. Never writes anything.
    pub fn load(&self, normalized_words: &str) -> Result<LoadedNotes, NotesError> {
        let main = self.read_optional(&self.main_path())?;
        let main_existed = main.is_some();
        let main_error = match main {
            Some(bytes) => match Self::open_notes(&bytes, normalized_words) {
                Ok((keys, data)) => {
                    return Ok(LoadedNotes {
                        keys,
                        data,
                        source: LoadSource::Main,
                    });
                }
                Err(error) => Some(error),
            },
            None => None,
        };

        if let Some(error) = &main_error
            && matches!(
                error,
                NotesError::UnsupportedVersion | NotesError::DataTooNew
            )
        {
            return Err(error.clone());
        }

        let Some(bytes) = self.read_optional(&self.backup_path())? else {
            return Err(if main_existed {
                main_error.unwrap_or(NotesError::NoNotesFile)
            } else {
                NotesError::NoNotesFile
            });
        };

        match Self::open_notes(&bytes, normalized_words) {
            Ok((keys, data)) => Ok(LoadedNotes {
                keys,
                data,
                source: LoadSource::Backup,
            }),
            Err(error) => {
                if matches!(
                    error,
                    NotesError::UnsupportedVersion | NotesError::DataTooNew
                ) {
                    return Err(error);
                }
                if main_existed {
                    Err(main_error.unwrap_or(error))
                } else {
                    Err(error)
                }
            }
        }
    }

    /// Saves the data with the keys of the open file.
    pub fn save(
        &self,
        keys: &NotesKeys,
        data: &NotesData,
        main_is_good: bool,
    ) -> Result<SaveReport, NotesError> {
        let payload = data.to_payload()?;
        let file = keys.reseal(&payload)?;
        let policy = if main_is_good {
            BackupPolicy::Rotate
        } else {
            BackupPolicy::KeepBackup
        };
        let mut verify = |read: &[u8]| match keys.open_body(read) {
            Ok(opened) => opened.as_slice() == payload.as_slice(),
            Err(_) => false,
        };
        self.write_atomic(&file, &mut verify, policy)
    }

    /// Creates a new file with new keys for the new words and makes it the
    /// current file. The backup becomes a copy of the new file.
    pub fn change_passphrase(
        &self,
        new_words: &str,
        settings: &CreateSettings,
        data: &NotesData,
    ) -> Result<(NotesKeys, SaveReport), NotesError> {
        let payload = data.to_payload()?;
        let (file, keys) = create(new_words, settings, &payload)?;
        let report = {
            let mut verify = |read: &[u8]| match keys.open_body(read) {
                Ok(opened) => opened.as_slice() == payload.as_slice(),
                Err(_) => false,
            };
            self.write_atomic(&file, &mut verify, BackupPolicy::ReplaceWithNew)?
        };
        Ok((keys, report))
    }
}
