//! The folder archive: a whole folder tree as ONE byte stream.
//!
//! A folder is encrypted by the same engine as a single file; only the
//! bytes that go in are different. This module produces that stream on the
//! way in (`ArchiveReader`, driven by a `scan_folder` snapshot) and takes it
//! apart again on the way out (`ArchiveWriter`).
//!
//! Stream layout, all integers little-endian:
//!
//!   directory entry : tag 1, path length (2 bytes), path
//!   file entry      : tag 2, path length (2 bytes), path, size (8 bytes), content
//!   end             : tag 0 (one byte); nothing may follow it
//!
//! A path is UTF-8, relative to the folder, with '/' between its parts. A
//! directory always comes before everything inside it. Empty folders and
//! empty files are kept. Names are sorted, so the same folder always gives
//! the same stream. File dates, attributes and permissions are NOT stored.
//!
//! SECURITY: when the stream is taken apart it is UNTRUSTED, because it was
//! made by whoever encrypted the folder. Every path part must be a plain
//! Windows file name (`names::safe_file_name`), the parent of every entry
//! must already exist, nothing is ever overwritten, links are never
//! created, and nothing can be written outside the target folder.

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use super::error::VaultError;
use super::names::safe_file_name;

const TAG_END: u8 = 0;
const TAG_DIR: u8 = 1;
const TAG_FILE: u8 = 2;
/// Longest relative path in bytes.
pub const MAX_PATH_BYTES: usize = 2048;
/// Deepest nesting of folders.
pub const MAX_DEPTH: usize = 64;
/// Most items (files and folders) in one archive.
pub const MAX_ENTRIES: usize = 500_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Dir,
    File { size: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Path relative to the folder, parts joined with '/'.
    pub relative: String,
    /// Where the item is on disk (encrypt side).
    pub disk_path: PathBuf,
    pub kind: EntryKind,
}

/// A snapshot of a folder: what will go into the archive, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scan {
    pub entries: Vec<Entry>,
    pub file_count: u64,
    pub dir_count: u64,
    /// Exact length of the archive stream in bytes.
    pub stream_len: u64,
}

/// What `ArchiveWriter` created.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ArchiveStats {
    pub files: u64,
    pub dirs: u64,
    /// Total bytes of file content.
    pub bytes: u64,
}

fn join_relative(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_owned()
    } else {
        format!("{prefix}/{name}")
    }
}

/// Size of the header of one entry in the stream.
fn header_len(relative: &str, is_file: bool) -> u64 {
    let fixed: u64 = if is_file { 11 } else { 3 };
    fixed.saturating_add(relative.len() as u64)
}

fn cannot_read(path: &str) -> VaultError {
    VaultError::CannotReadEntry {
        path: path.to_owned(),
    }
}

fn walk(dir: &Path, prefix: &str, depth: usize, scan: &mut Scan) -> Result<(), VaultError> {
    if depth > MAX_DEPTH {
        return Err(VaultError::FolderTooBig);
    }
    let shown = if prefix.is_empty() {
        "the folder"
    } else {
        prefix
    };
    let listing = fs::read_dir(dir).map_err(|_| cannot_read(shown))?;
    let mut children: Vec<(String, PathBuf, fs::FileType)> = Vec::new();
    for item in listing {
        let item = item.map_err(|_| cannot_read(shown))?;
        let file_type = item.file_type().map_err(|_| cannot_read(shown))?;
        let name = match item.file_name().into_string() {
            Ok(name) => name,
            Err(other) => {
                return Err(VaultError::UnsupportedName {
                    path: join_relative(prefix, &other.to_string_lossy()),
                });
            }
        };
        children.push((name, item.path(), file_type));
    }
    children.sort_by_cached_key(|child| child.0.clone());

    for (name, path, file_type) in children {
        let relative = join_relative(prefix, &name);
        if safe_file_name(&name).is_none() {
            return Err(VaultError::UnsupportedName { path: relative });
        }
        if relative.len() > MAX_PATH_BYTES || scan.entries.len() >= MAX_ENTRIES {
            return Err(VaultError::FolderTooBig);
        }
        if file_type.is_symlink() {
            return Err(VaultError::UnsupportedEntry { path: relative });
        }
        if file_type.is_dir() {
            scan.dir_count += 1;
            scan.stream_len = scan.stream_len.saturating_add(header_len(&relative, false));
            scan.entries.push(Entry {
                relative: relative.clone(),
                disk_path: path.clone(),
                kind: EntryKind::Dir,
            });
            walk(&path, &relative, depth + 1, scan)?;
        } else if file_type.is_file() {
            let size = fs::metadata(&path)
                .map_err(|_| cannot_read(&relative))?
                .len();
            scan.file_count += 1;
            scan.stream_len = scan
                .stream_len
                .saturating_add(header_len(&relative, true))
                .saturating_add(size);
            scan.entries.push(Entry {
                relative,
                disk_path: path,
                kind: EntryKind::File { size },
            });
        } else {
            return Err(VaultError::UnsupportedEntry { path: relative });
        }
    }
    Ok(())
}

/// Lists a folder (sorted, links refused) and measures the stream it
/// will produce.
pub fn scan_folder(root: &Path) -> Result<Scan, VaultError> {
    let mut scan = Scan {
        entries: Vec::new(),
        file_count: 0,
        dir_count: 0,
        // The end record.
        stream_len: 1,
    };
    walk(root, "", 1, &mut scan)?;
    Ok(scan)
}

fn entry_header(entry: &Entry) -> Result<Vec<u8>, VaultError> {
    let name = entry.relative.as_bytes();
    let length = u16::try_from(name.len()).map_err(|_| VaultError::Internal)?;
    let tag = match entry.kind {
        EntryKind::Dir => TAG_DIR,
        EntryKind::File { .. } => TAG_FILE,
    };
    let mut out = Vec::with_capacity(11 + name.len());
    out.push(tag);
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(name);
    if let EntryKind::File { size } = entry.kind {
        out.extend_from_slice(&size.to_le_bytes());
    }
    Ok(out)
}

/// Copies as many bytes as fit and returns how many.
fn copy_prefix(target: &mut [u8], source: &[u8]) -> usize {
    let count = target.len().min(source.len());
    for (to, from) in target.iter_mut().zip(source.iter()) {
        *to = *from;
    }
    count
}

fn read_retry(file: &mut File, buf: &mut [u8]) -> io::Result<usize> {
    loop {
        match file.read(buf) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            other => return other,
        }
    }
}

fn changed(path: &str) -> io::Error {
    io::Error::other(VaultError::FileChanged {
        path: path.to_owned(),
    })
}

struct ActiveFile {
    file: File,
    remaining: u64,
    path: String,
}

/// Produces the archive stream by reading the files one after another.
/// A file that changes size while it is read is an error.
pub struct ArchiveReader {
    entries: VecDeque<Entry>,
    pending: Vec<u8>,
    pending_position: usize,
    active: Option<ActiveFile>,
    finished: bool,
}

impl ArchiveReader {
    pub fn new(scan: Scan) -> Self {
        Self {
            entries: scan.entries.into(),
            pending: Vec::new(),
            pending_position: 0,
            active: None,
            finished: false,
        }
    }
}

impl Read for ArchiveReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            // 1. Header bytes of the current entry.
            if let Some(rest) = self.pending.get(self.pending_position..)
                && !rest.is_empty()
            {
                let count = copy_prefix(buf, rest);
                self.pending_position += count;
                return Ok(count);
            }
            // 2. Content of the current file.
            if let Some(active) = self.active.as_mut() {
                if active.remaining == 0 {
                    // The file must end exactly here, otherwise it grew.
                    let mut probe = [0u8; 1];
                    let extra = read_retry(&mut active.file, &mut probe)?;
                    if extra > 0 {
                        return Err(changed(&active.path));
                    }
                    self.active = None;
                    continue;
                }
                let limit = usize::try_from(active.remaining)
                    .unwrap_or(usize::MAX)
                    .min(buf.len());
                let Some(slice) = buf.get_mut(..limit) else {
                    return Err(io::Error::other(VaultError::Internal));
                };
                let count = read_retry(&mut active.file, slice)?;
                if count == 0 {
                    // Shorter than when the folder was scanned.
                    return Err(changed(&active.path));
                }
                active.remaining = active.remaining.saturating_sub(count as u64);
                return Ok(count);
            }
            // 3. Next entry, or the end record.
            if self.finished {
                return Ok(0);
            }
            match self.entries.pop_front() {
                Some(entry) => {
                    self.pending = entry_header(&entry).map_err(io::Error::other)?;
                    self.pending_position = 0;
                    if let EntryKind::File { size } = entry.kind {
                        let file = File::open(&entry.disk_path)
                            .map_err(|_| io::Error::other(cannot_read(&entry.relative)))?;
                        self.active = Some(ActiveFile {
                            file,
                            remaining: size,
                            path: entry.relative,
                        });
                    }
                }
                None => {
                    self.pending = vec![TAG_END];
                    self.pending_position = 0;
                    self.finished = true;
                }
            }
        }
    }
}

enum ParseState {
    Tag,
    NameLength { tag: u8 },
    Name { tag: u8, length: usize },
    Size { path: PathBuf },
    Data { remaining: u64 },
    Done,
}

/// Moves bytes from `input` into `buffer` until it holds `need` bytes.
/// True when the buffer is complete.
fn take_into(buffer: &mut Vec<u8>, need: usize, input: &mut &[u8]) -> bool {
    let missing = need.saturating_sub(buffer.len());
    let count = missing.min(input.len());
    if let Some((head, tail)) = input.split_at_checked(count) {
        buffer.extend_from_slice(head);
        *input = tail;
    }
    buffer.len() >= need
}

fn array_of<const N: usize>(bytes: &[u8]) -> Result<[u8; N], VaultError> {
    <[u8; N]>::try_from(bytes).map_err(|_| VaultError::Internal)
}

/// A path that cannot be created is unsafe or duplicate; a full disk and
/// other problems keep their own message.
fn create_error(error: io::Error) -> VaultError {
    match error.kind() {
        io::ErrorKind::AlreadyExists
        | io::ErrorKind::NotFound
        | io::ErrorKind::NotADirectory
        | io::ErrorKind::InvalidInput => VaultError::UnsafeArchive,
        _ => VaultError::from(error),
    }
}

/// Takes an archive stream apart while it is being written to it, creating
/// the folders and files below `root` (which must already exist).
pub struct ArchiveWriter {
    root: PathBuf,
    state: ParseState,
    scratch: Vec<u8>,
    file: Option<File>,
    stats: ArchiveStats,
}

impl ArchiveWriter {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            state: ParseState::Tag,
            scratch: Vec::new(),
            file: None,
            stats: ArchiveStats::default(),
        }
    }

    /// Checks every part of an untrusted relative path and returns the
    /// path below the root.
    fn checked_path(&self, relative: &str) -> Result<PathBuf, VaultError> {
        let mut path = self.root.clone();
        let mut depth = 0usize;
        for part in relative.split('/') {
            if safe_file_name(part).is_none() {
                return Err(VaultError::UnsafeArchive);
            }
            depth += 1;
            path.push(part);
        }
        if depth > MAX_DEPTH {
            return Err(VaultError::UnsafeArchive);
        }
        Ok(path)
    }

    fn consume(&mut self, data: &[u8]) -> Result<(), VaultError> {
        let mut input = data;
        while !input.is_empty() {
            match std::mem::replace(&mut self.state, ParseState::Done) {
                ParseState::Tag => {
                    if !take_into(&mut self.scratch, 1, &mut input) {
                        self.state = ParseState::Tag;
                        continue;
                    }
                    let tag = self.scratch.first().copied().ok_or(VaultError::Internal)?;
                    self.scratch.clear();
                    self.state = match tag {
                        TAG_END => ParseState::Done,
                        TAG_DIR | TAG_FILE => ParseState::NameLength { tag },
                        _ => return Err(VaultError::CorruptedData),
                    };
                }
                ParseState::NameLength { tag } => {
                    if !take_into(&mut self.scratch, 2, &mut input) {
                        self.state = ParseState::NameLength { tag };
                        continue;
                    }
                    let length = usize::from(u16::from_le_bytes(array_of(&self.scratch)?));
                    self.scratch.clear();
                    if length == 0 || length > MAX_PATH_BYTES {
                        return Err(VaultError::UnsafeArchive);
                    }
                    self.state = ParseState::Name { tag, length };
                }
                ParseState::Name { tag, length } => {
                    if !take_into(&mut self.scratch, length, &mut input) {
                        self.state = ParseState::Name { tag, length };
                        continue;
                    }
                    let name = String::from_utf8(std::mem::take(&mut self.scratch))
                        .map_err(|_| VaultError::UnsafeArchive)?;
                    let path = self.checked_path(&name)?;
                    if tag == TAG_DIR {
                        fs::create_dir(&path).map_err(create_error)?;
                        self.stats.dirs += 1;
                        self.state = ParseState::Tag;
                    } else {
                        self.state = ParseState::Size { path };
                    }
                }
                ParseState::Size { path } => {
                    if !take_into(&mut self.scratch, 8, &mut input) {
                        self.state = ParseState::Size { path };
                        continue;
                    }
                    let size = u64::from_le_bytes(array_of(&self.scratch)?);
                    self.scratch.clear();
                    let file = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&path)
                        .map_err(create_error)?;
                    self.stats.files += 1;
                    if size == 0 {
                        drop(file);
                        self.state = ParseState::Tag;
                    } else {
                        self.file = Some(file);
                        self.state = ParseState::Data { remaining: size };
                    }
                }
                ParseState::Data { remaining } => {
                    let take = usize::try_from(remaining)
                        .unwrap_or(usize::MAX)
                        .min(input.len());
                    let Some((head, tail)) = input.split_at_checked(take) else {
                        return Err(VaultError::Internal);
                    };
                    let file = self.file.as_mut().ok_or(VaultError::Internal)?;
                    file.write_all(head)?;
                    input = tail;
                    self.stats.bytes = self.stats.bytes.saturating_add(take as u64);
                    let left = remaining.saturating_sub(take as u64);
                    if left == 0 {
                        self.file = None;
                        self.state = ParseState::Tag;
                    } else {
                        self.state = ParseState::Data { remaining: left };
                    }
                }
                ParseState::Done => return Err(VaultError::CorruptedData),
            }
        }
        Ok(())
    }

    /// Checks that the stream ended properly (end record seen, no file
    /// left half written) and returns what was created.
    pub fn finish(self) -> Result<ArchiveStats, VaultError> {
        if matches!(self.state, ParseState::Done) && self.file.is_none() && self.scratch.is_empty()
        {
            Ok(self.stats)
        } else {
            Err(VaultError::CorruptedData)
        }
    }
}

impl Write for ArchiveWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.consume(data).map_err(io::Error::other)?;
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
