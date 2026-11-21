//! Splitting one logical encrypted stream over several part files, and
//! reading such a set back as one stream.
//!
//! Every part file starts with a 28-byte part header:
//!   0   8  magic "ERXPART1"
//!   8  16  file UUID (the same UUID as inside the engine header)
//!  24   4  part index, little-endian, starting at 1
//! The rest of the file is a slice of the logical stream (the output of
//! the engine). Part boundaries are independent of the 1 MiB chunk
//! boundaries: a chunk may span two parts.
//!
//! The part header is NOT a security feature. It only lets the reader give
//! a friendly message for a wrong, missing or reordered part. The real
//! protection is the engine: any missing, swapped or changed part breaks
//! the STREAM authentication of the logical stream.
//!
//! Part file names: split off -> `name.erx`; split on -> `name.001.erx`,
//! `name.002.erx`, ... (even when only one part results). While writing,
//! every part has a temporary name (`<final name>.<token>.erxtmp`); the
//! caller renames them once everything is written.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use super::error::VaultError;
use super::format::UUID_LEN;

/// Magic bytes at the start of every part file.
pub const PART_MAGIC: [u8; 8] = *b"ERXPART1";
/// Size of the part header in bytes.
pub const PART_HEADER_LEN: usize = 28;
/// Highest part number (names use three digits: .001 to .999).
pub const MAX_PARTS: u32 = 999;
/// Smallest part size a writer accepts (header plus a little payload).
const MIN_PART_LIMIT: u64 = 64;
/// Offset of the file UUID inside the engine header (see format.rs).
const ENGINE_UUID_OFFSET: usize = 12;
/// How many part numbers after a gap are checked to tell "a part in the
/// middle is missing" from "this was the last part".
const MISSING_PROBE: u32 = 8;
/// Extension of temporary files.
pub const TEMP_EXTENSION: &str = "erxtmp";

/// The part-size choices offered to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartSize {
    Off,
    Mib500,
    Gib1,
    Gib2,
}

impl PartSize {
    /// Largest size of one part file in bytes (header included), or
    /// `None` when the output is not split. Binary units, as Windows
    /// shows them; all are below the FAT32 limit of 4 GiB.
    pub fn limit_bytes(self) -> Option<u64> {
        const MIB: u64 = 1024 * 1024;
        match self {
            PartSize::Off => None,
            PartSize::Mib500 => Some(500 * MIB),
            PartSize::Gib1 => Some(1024 * MIB),
            PartSize::Gib2 => Some(2048 * MIB),
        }
    }
}

fn io_error(error: VaultError) -> io::Error {
    io::Error::other(error)
}

/// Path of part `index`. Without splitting this is the base path itself.
///
/// With splitting the number goes before the `.erx` extension:
/// `Thefile.erx` -> `Thefile.001.erx`; `doc.bin.erx` ->
/// `doc.bin.001.erx`. A base without an `.erx` extension gets the number
/// and extension appended to its whole name.
pub fn part_path(base: &Path, split: bool, index: u32) -> PathBuf {
    if !split {
        return base.to_path_buf();
    }
    let suffix = format!(".{index:03}.erx");
    let Some(file_name) = base.file_name() else {
        let mut name = base.as_os_str().to_os_string();
        name.push(suffix);
        return PathBuf::from(name);
    };
    let is_erx = base
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("erx"));
    let mut name = if is_erx {
        match base.file_stem() {
            Some(stem) => stem.to_os_string(),
            None => file_name.to_os_string(),
        }
    } else {
        file_name.to_os_string()
    };
    name.push(suffix);
    match base.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(name),
        _ => PathBuf::from(name),
    }
}

/// Temporary name used while a file is being written.
pub fn temp_path(final_path: &Path, token: &str) -> PathBuf {
    let mut name = final_path.as_os_str().to_os_string();
    name.push(format!(".{token}.{TEMP_EXTENSION}"));
    PathBuf::from(name)
}

/// Eight random hex characters that make temporary names unique.
pub fn random_token() -> Result<String, VaultError> {
    let mut bytes = [0u8; 4];
    getrandom::fill(&mut bytes).map_err(|_| VaultError::RandomUnavailable)?;
    Ok(format!("{:08x}", u32::from_le_bytes(bytes)))
}

fn part_header_bytes(uuid: &[u8; UUID_LEN], index: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(PART_HEADER_LEN);
    out.extend_from_slice(&PART_MAGIC);
    out.extend_from_slice(uuid);
    out.extend_from_slice(&index.to_le_bytes());
    out
}

fn parse_part_header(bytes: &[u8]) -> Option<([u8; UUID_LEN], u32)> {
    let (magic, rest) = bytes.split_at_checked(PART_MAGIC.len())?;
    if magic != PART_MAGIC.as_slice() {
        return None;
    }
    let (uuid_bytes, index_bytes) = rest.split_at_checked(UUID_LEN)?;
    let uuid = <[u8; UUID_LEN]>::try_from(uuid_bytes).ok()?;
    let index = u32::from_le_bytes(<[u8; 4]>::try_from(index_bytes).ok()?);
    Some((uuid, index))
}

pub(crate) fn read_part_header(file: &mut File) -> Option<([u8; UUID_LEN], u32)> {
    let mut buffer = [0u8; PART_HEADER_LEN];
    file.read_exact(&mut buffer).ok()?;
    parse_part_header(&buffer)
}

/// The file UUID from the first bytes of the engine header, once enough
/// bytes are available.
fn uuid_from_engine_header(prelude: &[u8]) -> Option<[u8; UUID_LEN]> {
    let slice = prelude.get(ENGINE_UUID_OFFSET..ENGINE_UUID_OFFSET + UUID_LEN)?;
    <[u8; UUID_LEN]>::try_from(slice).ok()
}

/// Temporary and final path of one finished part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartPaths {
    pub temp: PathBuf,
    pub final_path: PathBuf,
}

/// Writes the logical stream into part files with temporary names.
///
/// The first bytes written must be the engine header (the engine always
/// writes it first); the file UUID is taken from it. Dropping the writer
/// without calling `finish` deletes every temporary file.
pub struct SplitWriter {
    base: PathBuf,
    limit: Option<u64>,
    token: String,
    uuid: Option<[u8; UUID_LEN]>,
    prelude: Vec<u8>,
    current: Option<File>,
    current_len: u64,
    index: u32,
    created: Vec<PathBuf>,
}

impl SplitWriter {
    /// `base` is the final path of the unsplit file (`name.erx`).
    /// `limit` is the largest size of one part in bytes (header included)
    /// or `None` for a single unsplit file.
    pub fn new(base: &Path, limit: Option<u64>, token: String) -> Result<Self, VaultError> {
        if let Some(value) = limit
            && value < MIN_PART_LIMIT
        {
            return Err(VaultError::Internal);
        }
        Ok(Self {
            base: base.to_path_buf(),
            limit,
            token,
            uuid: None,
            prelude: Vec::new(),
            current: None,
            current_len: 0,
            index: 0,
            created: Vec::new(),
        })
    }

    fn is_split(&self) -> bool {
        self.limit.is_some()
    }

    fn final_path(&self, index: u32) -> PathBuf {
        part_path(&self.base, self.is_split(), index)
    }

    fn close_current(&mut self) -> io::Result<()> {
        if let Some(file) = self.current.take() {
            file.sync_all()?;
        }
        Ok(())
    }

    fn start_part(&mut self, uuid: [u8; UUID_LEN]) -> io::Result<()> {
        self.close_current()?;
        let next = self
            .index
            .checked_add(1)
            .ok_or_else(|| io_error(VaultError::TooManyParts))?;
        if next > MAX_PARTS {
            return Err(io_error(VaultError::TooManyParts));
        }
        let temp = temp_path(&self.final_path(next), &self.token);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        // Registered right after creation so that abort / drop deletes it.
        self.created.push(temp);
        file.write_all(&part_header_bytes(&uuid, next))?;
        self.current = Some(file);
        self.current_len = PART_HEADER_LEN as u64;
        self.index = next;
        Ok(())
    }

    fn write_payload(&mut self, data: &[u8]) -> io::Result<()> {
        let Some(uuid) = self.uuid else {
            return Err(io_error(VaultError::Internal));
        };
        let mut rest = data;
        while !rest.is_empty() {
            if self.current.is_none() {
                self.start_part(uuid)?;
            }
            let room = match self.limit {
                Some(limit) => limit.saturating_sub(self.current_len),
                None => u64::MAX,
            };
            if room == 0 {
                // This part is full; the next loop round starts a new one.
                // A new part is only started when more data arrives, so
                // the last part is never empty.
                self.close_current()?;
                continue;
            }
            let take = usize::try_from(room).unwrap_or(usize::MAX).min(rest.len());
            let Some((head, tail)) = rest.split_at_checked(take) else {
                return Err(io_error(VaultError::Internal));
            };
            let file = self
                .current
                .as_mut()
                .ok_or_else(|| io_error(VaultError::Internal))?;
            file.write_all(head)?;
            self.current_len = self.current_len.saturating_add(take as u64);
            rest = tail;
        }
        Ok(())
    }

    /// Closes the last part and hands the temporary/final path pairs to
    /// the caller, who renames them. After this the writer no longer
    /// deletes anything.
    pub fn finish(&mut self) -> Result<Vec<PartPaths>, VaultError> {
        if self.uuid.is_none() {
            return Err(VaultError::Internal);
        }
        self.close_current()?;
        let created = std::mem::take(&mut self.created);
        let mut out = Vec::with_capacity(created.len());
        for (position, temp) in created.into_iter().enumerate() {
            let index = u32::try_from(position + 1).map_err(|_| VaultError::Internal)?;
            out.push(PartPaths {
                temp,
                final_path: self.final_path(index),
            });
        }
        Ok(out)
    }

    /// Closes the open file and deletes every temporary file.
    pub fn abort(&mut self) {
        // The handle must be closed first: Windows cannot delete open files.
        self.current = None;
        for temp in self.created.drain(..) {
            let _ = fs::remove_file(temp);
        }
    }
}

impl Write for SplitWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if self.uuid.is_none() {
            // Hold the first bytes back until the UUID inside the engine
            // header is complete.
            self.prelude.extend_from_slice(data);
            if let Some(uuid) = uuid_from_engine_header(&self.prelude) {
                self.uuid = Some(uuid);
                let pending = std::mem::take(&mut self.prelude);
                self.write_payload(&pending)?;
            }
            return Ok(data.len());
        }
        self.write_payload(data)?;
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.current.as_mut() {
            Some(file) => file.flush(),
            None => Ok(()),
        }
    }
}

impl Drop for SplitWriter {
    fn drop(&mut self) {
        self.abort();
    }
}

/// How the file the user selected is named.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PartLayout {
    /// One file that holds the whole stream (`name.erx`).
    Single,
    /// One part of a numbered set (`name.001.erx` ... `name.999.erx`).
    Numbered,
}

/// Detects the layout from the NAME of the selected file and returns the
/// base path of the set (`name.erx`).
///
/// A name is numbered when its last extension is `erx` and its stem ends
/// with a dot followed by exactly three digits, with something non-empty
/// before that dot (`Thefile.001.erx`). Everything else is a single file.
/// Only the part header decides later whether a numbered file really is
/// part of a set; the digits in the name never do.
pub(crate) fn detect_layout(path: &Path) -> (PathBuf, PartLayout) {
    let single = || (path.to_path_buf(), PartLayout::Single);
    if !path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("erx"))
    {
        return single();
    }
    let Some(stem) = path.file_stem() else {
        return single();
    };
    let Some(stem) = stem.to_str() else {
        return single();
    };
    let Some((prefix, digits)) = stem.rsplit_once('.') else {
        return single();
    };
    if prefix.is_empty() || digits.len() != 3 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return single();
    }
    let mut name = OsString::from(prefix);
    name.push(".erx");
    let base = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(name),
        _ => PathBuf::from(name),
    };
    (base, PartLayout::Numbered)
}

/// Sum of the payload sizes (part headers excluded) of the parts that
/// really belong to the set: same UUID and their expected number.
fn measure_total(base: &Path, layout: PartLayout, uuid: [u8; UUID_LEN]) -> u64 {
    match layout {
        PartLayout::Single => fs::metadata(base)
            .map(|metadata| metadata.len().saturating_sub(PART_HEADER_LEN as u64))
            .unwrap_or(0),
        PartLayout::Numbered => {
            let mut total = 0u64;
            for index in 1..=MAX_PARTS {
                let path = part_path(base, true, index);
                let Ok(mut file) = File::open(&path) else {
                    break;
                };
                let Some((part_uuid, part_index)) = read_part_header(&mut file) else {
                    break;
                };
                if part_uuid != uuid || part_index != index {
                    break;
                }
                if let Ok(metadata) = file.metadata() {
                    total =
                        total.saturating_add(metadata.len().saturating_sub(PART_HEADER_LEN as u64));
                }
            }
            total
        }
    }
}

/// True when both paths point at the same file. Used to tell a numbered
/// name that really is part 1 of a set from an unsplit file that only looks
/// numbered (`scan.002.erx`).
fn same_path(first: &Path, second: &Path) -> bool {
    match (fs::canonicalize(first), fs::canonicalize(second)) {
        (Ok(a), Ok(b)) => a == b,
        _ => first == second,
    }
}

/// Reads a set of part files as one continuous stream.
pub struct SplitReader {
    base: PathBuf,
    split: bool,
    uuid: [u8; UUID_LEN],
    current: Option<File>,
    index: u32,
    logical_total: u64,
}

impl SplitReader {
    /// Opens the FIRST part (`name.erx` or `name.001.erx`).
    pub fn open(first: &Path) -> Result<Self, VaultError> {
        let (base, layout) = detect_layout(first);
        let mut file = File::open(first).map_err(|_| VaultError::CannotOpenFile)?;
        let Some((uuid, index)) = read_part_header(&mut file) else {
            return Err(VaultError::NotAVaultFile);
        };
        if index != 1 {
            return Err(VaultError::NotFirstPart);
        }
        // A numbered name is only a split set when the selected file is the
        // real first part of that base. An unsplit file can have the same
        // shape (`scan.002.erx` next to `scan.001.erx`): read it alone.
        let split = layout == PartLayout::Numbered && same_path(&part_path(&base, true, 1), first);
        let logical_total = if split {
            measure_total(&base, PartLayout::Numbered, uuid)
        } else {
            measure_total(first, PartLayout::Single, uuid)
        };
        Ok(Self {
            base,
            split,
            uuid,
            current: Some(file),
            index: 1,
            logical_total,
        })
    }

    /// Payload bytes of all parts that exist right now (for the progress
    /// percentage while decrypting).
    pub fn logical_total(&self) -> u64 {
        self.logical_total
    }

    fn later_part_exists(&self, after: u32) -> bool {
        let last = after.saturating_add(MISSING_PROBE).min(MAX_PARTS);
        (after.saturating_add(1)..=last).any(|index| {
            let path = part_path(&self.base, true, index);
            let Ok(mut file) = File::open(&path) else {
                return false;
            };
            matches!(read_part_header(&mut file), Some((uuid, _)) if uuid == self.uuid)
        })
    }

    /// Opens the next part. `Ok(false)` means the logical stream ended.
    ///
    /// A file at the next name that is not a vault part, or that belongs to
    /// another set (different UUID), is treated as if it did not exist: a
    /// neighbour with a matching name must not break this set, and its data
    /// is never read.
    fn open_next(&mut self) -> io::Result<bool> {
        let next = self.index.saturating_add(1);
        if next > MAX_PARTS {
            return Ok(false);
        }
        let path = part_path(&self.base, true, next);
        let mut file = match File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return self.end_or_missing(next);
            }
            Err(_) => return Err(io_error(VaultError::CannotOpenFile)),
        };
        let Some((uuid, index)) = read_part_header(&mut file) else {
            // A file that is not a vault part at all is not part of this
            // set: a stray file with a matching name must not break it.
            return self.end_or_missing(next);
        };
        if uuid != self.uuid {
            // A file of another set that happens to have the next name.
            return self.end_or_missing(next);
        }
        if index != next {
            return Err(io_error(VaultError::WrongPart { index: next }));
        }
        self.current = Some(file);
        self.index = next;
        Ok(true)
    }

    /// The next name is not usable. A later part of THIS set means the
    /// current number is really missing; otherwise the stream ended.
    fn end_or_missing(&self, next: u32) -> io::Result<bool> {
        if self.later_part_exists(next) {
            return Err(io_error(VaultError::MissingPart { index: next }));
        }
        Ok(false)
    }
}

impl Read for SplitReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            let Some(file) = self.current.as_mut() else {
                return Ok(0);
            };
            let count = file.read(buf)?;
            if count > 0 {
                return Ok(count);
            }
            // End of this part: continue with the next one, if any.
            self.current = None;
            if !self.split {
                return Ok(0);
            }
            if !self.open_next()? {
                return Ok(0);
            }
        }
    }
}
