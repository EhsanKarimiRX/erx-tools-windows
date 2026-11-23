//! Shared test helpers: a temporary folder and a file system that can be
//! scripted to fail. Only compiled for tests.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use super::store::{FsOps, NotesStore, RealFs, RetryPolicy};

/// A folder under the system temporary folder that removes itself.
pub(crate) struct TempDir {
    pub(crate) path: PathBuf,
}

impl TempDir {
    pub(crate) fn new() -> TempDir {
        let mut bytes = [0u8; 8];
        getrandom::fill(&mut bytes).unwrap();
        let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        let path = std::env::temp_dir().join(format!("erx-notes-test-{hex}"));
        fs::create_dir_all(&path).unwrap();
        TempDir { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// A retry policy with no waiting, for tests.
pub(crate) fn retry_once() -> RetryPolicy {
    RetryPolicy {
        attempts: 1,
        delay: Duration::ZERO,
    }
}

/// A real store in `dir` that does not retry.
pub(crate) fn real_store(dir: &Path) -> NotesStore {
    NotesStore::with_ops(dir.to_path_buf(), Box::new(RealFs), retry_once())
}

pub(crate) struct ScriptedFs {
    pub(crate) calls: AtomicUsize,
    pub(crate) fail_rename_calls: Vec<usize>,
    pub(crate) fail_remove: bool,
}

impl FsOps for ScriptedFs {
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if self.fail_rename_calls.contains(&call) {
            return Err(std::io::Error::other("scripted"));
        }
        std::fs::rename(from, to)
    }

    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        if self.fail_remove {
            return Err(std::io::Error::other("scripted"));
        }
        std::fs::remove_file(path)
    }
}

pub(crate) fn scripted_store(
    dir: &Path,
    fail_rename_calls: Vec<usize>,
    fail_remove: bool,
) -> NotesStore {
    NotesStore::with_ops(
        dir.to_path_buf(),
        Box::new(ScriptedFs {
            calls: AtomicUsize::new(0),
            fail_rename_calls,
            fail_remove,
        }),
        retry_once(),
    )
}
