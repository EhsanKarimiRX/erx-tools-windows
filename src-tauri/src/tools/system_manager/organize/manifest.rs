//! The undo manifest: one append-only JSON Lines file per target kind in
//! the app data dir (`organize_desktop.jsonl`, `organize_downloads.jsonl`,
//! `organize_custom.jsonl`). A new run on the same target REPLACES that
//! target's manifest. Every `move` line is appended and flushed right
//! after the move succeeds, so a crash never loses track of a moved file;
//! a missing `end` line (crash) must still be undoable.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One manifest line. Paths are stored as plain strings (absolute).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ManifestLine {
    #[serde(rename = "run")]
    Run {
        version: u32,
        target: String,
        root: String,
        started_at: i64,
    },
    #[serde(rename = "mkdir")]
    Mkdir { path: String },
    #[serde(rename = "move")]
    Move { from: String, to: String },
    #[serde(rename = "end")]
    End { moved: u32, skipped: u32 },
}

pub const MANIFEST_VERSION: u32 = 1;

/// `<app data dir>/organize_<target>.jsonl`.
pub fn manifest_path(state_dir: &Path, target: super::core::OrganizeTarget) -> PathBuf {
    state_dir.join(format!("organize_{}.jsonl", target.id()))
}

/// Opens a fresh manifest for a new run (replacing the previous one) and
/// writes the `run` line immediately.
pub fn start(
    path: &Path,
    target: super::core::OrganizeTarget,
    root: &Path,
    started_at: i64,
) -> Result<ManifestWriter, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Organize could not create its data folder: {error}"))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .map_err(|error| format!("Organize could not open its undo file: {error}"))?;
    write_line(
        &mut file,
        &ManifestLine::Run {
            version: MANIFEST_VERSION,
            target: target.id().to_string(),
            root: root.to_string_lossy().into_owned(),
            started_at,
        },
    )?;
    Ok(ManifestWriter { file })
}

/// Appender for an in-progress run; every line is flushed immediately.
pub struct ManifestWriter {
    file: File,
}

impl ManifestWriter {
    pub fn record_move(&mut self, from: &Path, to: &Path) -> Result<(), String> {
        write_line(
            &mut self.file,
            &ManifestLine::Move {
                from: from.to_string_lossy().into_owned(),
                to: to.to_string_lossy().into_owned(),
            },
        )
    }

    pub fn record_mkdir(&mut self, path: &Path) -> Result<(), String> {
        write_line(
            &mut self.file,
            &ManifestLine::Mkdir {
                path: path.to_string_lossy().into_owned(),
            },
        )
    }

    pub fn record_end(&mut self, moved: u32, skipped: u32) -> Result<(), String> {
        write_line(&mut self.file, &ManifestLine::End { moved, skipped })
    }
}

fn write_line(file: &mut File, line: &ManifestLine) -> Result<(), String> {
    let text = serde_json::to_string(line)
        .map_err(|error| format!("Organize could not record its undo step: {error}"))?;
    writeln!(file, "{text}")
        .and_then(|()| file.flush())
        .map_err(|error| format!("Organize could not write its undo file: {error}"))
}

/// A parsed manifest: the run header plus what still happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub target: String,
    pub root: PathBuf,
    pub started_at: i64,
    pub moves: Vec<(PathBuf, PathBuf)>,
    pub mkdirs: Vec<PathBuf>,
}

/// Reads a manifest. Tolerant of a missing/truncated `end` line (crash) and
/// of unknown line types (future formats). The `run` line is REQUIRED —
/// without it the file says nothing usable and is an honest error.
pub fn read(path: &Path) -> Result<Manifest, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("Organize could not read its undo file: {error}"))?;
    let mut manifest: Option<Manifest> = None;
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        // Unknown future line types are skipped instead of failing undo.
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Ok(parsed) = serde_json::from_value::<ManifestLine>(value.clone()) else {
            continue;
        };
        match parsed {
            ManifestLine::Run {
                target,
                root,
                started_at,
                ..
            } => {
                manifest = Some(Manifest {
                    target,
                    root: PathBuf::from(&root),
                    started_at,
                    moves: Vec::new(),
                    mkdirs: Vec::new(),
                });
            }
            ManifestLine::Move { from, to } => {
                if let Some(entry) = manifest.as_mut() {
                    entry.moves.push((PathBuf::from(from), PathBuf::from(to)));
                }
            }
            ManifestLine::Mkdir { path } => {
                if let Some(entry) = manifest.as_mut() {
                    entry.mkdirs.push(PathBuf::from(path));
                }
            }
            // The end line carries counts only; undo does not need it.
            ManifestLine::End { .. } => {}
        }
    }
    manifest.ok_or_else(|| {
        "The undo file is damaged (no run information). Nothing can be undone; you can ignore this and organize again.".to_string()
    })
}

/// Rewrites the manifest with only the entries that still need undoing
/// (after a partial undo), or deletes the file when nothing remains.
pub fn rewrite(path: &Path, manifest: &Manifest) -> Result<(), String> {
    if manifest.moves.is_empty() && manifest.mkdirs.is_empty() {
        return delete(path);
    }
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .map_err(|error| format!("Organize could not update its undo file: {error}"))?;
    write_line(
        &mut file,
        &ManifestLine::Run {
            version: MANIFEST_VERSION,
            target: manifest.target.clone(),
            root: manifest.root.to_string_lossy().into_owned(),
            started_at: manifest.started_at,
        },
    )?;
    for (from, to) in &manifest.moves {
        write_line(
            &mut file,
            &ManifestLine::Move {
                from: from.to_string_lossy().into_owned(),
                to: to.to_string_lossy().into_owned(),
            },
        )?;
    }
    for mkdir in &manifest.mkdirs {
        write_line(
            &mut file,
            &ManifestLine::Mkdir {
                path: mkdir.to_string_lossy().into_owned(),
            },
        )?;
    }
    Ok(())
}

/// Deletes the manifest (a missing file is fine — already fully undone).
pub fn delete(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Organize could not remove its undo file: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::system_manager::organize::core::OrganizeTarget;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn temp_path(tag: &str) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "erx-organize-manifest-{}-{}-{}",
            std::process::id(),
            tag,
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("mkdir");
        dir.join("organize_test.jsonl")
    }

    #[test]
    fn round_trip_through_writer_and_reader() {
        let path = temp_path("roundtrip");
        let mut writer =
            start(&path, OrganizeTarget::Desktop, Path::new("C:\\root"), 100).expect("start");
        writer
            .record_mkdir(Path::new("C:\\root\\Documents"))
            .expect("mkdir");
        writer
            .record_move(
                Path::new("C:\\root\\a.pdf"),
                Path::new("C:\\root\\Documents\\a.pdf"),
            )
            .expect("move");
        writer.record_end(1, 0).expect("end");

        let manifest = read(&path).expect("read");
        assert_eq!(manifest.target, "desktop");
        assert_eq!(manifest.root, Path::new("C:\\root"));
        assert_eq!(
            manifest.moves,
            vec![(
                PathBuf::from("C:\\root\\a.pdf"),
                PathBuf::from("C:\\root\\Documents\\a.pdf")
            )]
        );
        assert_eq!(manifest.mkdirs, vec![PathBuf::from("C:\\root\\Documents")]);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn missing_end_line_is_still_undoable() {
        let path = temp_path("noend");
        let mut writer =
            start(&path, OrganizeTarget::Downloads, Path::new("C:\\r"), 1).expect("start");
        writer
            .record_move(Path::new("C:\\r\\x"), Path::new("C:\\r\\More\\x"))
            .expect("move");
        drop(writer); // "crash": no end line

        let manifest = read(&path).expect("read without end");
        assert_eq!(manifest.moves.len(), 1);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn run_without_run_line_is_an_honest_error() {
        let path = temp_path("norun");
        std::fs::write(&path, "{\"type\":\"move\",\"from\":\"a\",\"to\":\"b\"}\n").expect("write");
        assert!(read(&path).is_err());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn unknown_line_types_are_skipped() {
        let path = temp_path("unknown");
        std::fs::write(
            &path,
            "{\"type\":\"run\",\"version\":1,\"target\":\"custom\",\"root\":\"C:\\\\r\",\"started_at\":5}\n\
             {\"type\":\"somethinglew\",\"x\":1}\n\
             {\"type\":\"move\",\"from\":\"C:\\\\r\\\\a\",\"to\":\"C:\\\\r\\\\More\\\\a\"}\n",
        )
        .expect("write");
        let manifest = read(&path).expect("read");
        assert_eq!(manifest.target, "custom");
        assert_eq!(manifest.moves.len(), 1);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn rewrite_keeps_only_remaining_entries_or_deletes() {
        let path = temp_path("rewrite");
        let mut writer =
            start(&path, OrganizeTarget::Custom, Path::new("C:\\r"), 7).expect("start");
        writer
            .record_move(Path::new("C:\\r\\a"), Path::new("C:\\r\\Documents\\a"))
            .expect("m1");
        writer
            .record_move(Path::new("C:\\r\\b"), Path::new("C:\\r\\Images\\b"))
            .expect("m2");
        writer
            .record_mkdir(Path::new("C:\\r\\Documents"))
            .expect("d1");
        drop(writer);

        let mut manifest = read(&path).expect("read");
        manifest.moves.remove(1); // the first move was undone, keep the rest
        rewrite(&path, &manifest).expect("rewrite");
        let reloaded = read(&path).expect("re-read");
        assert_eq!(reloaded.moves.len(), 1);
        assert_eq!(reloaded.mkdirs.len(), 1);

        manifest.moves.clear();
        manifest.mkdirs.clear();
        rewrite(&path, &manifest).expect("rewrite to delete");
        assert!(!path.exists(), "empty manifest is deleted");
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }
}
