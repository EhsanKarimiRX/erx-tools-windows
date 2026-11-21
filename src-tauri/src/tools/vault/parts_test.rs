use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use super::engine::{EncryptSettings, encrypt_stream};
use super::error::VaultError;
use super::format::{HEADER_LEN, Header};
use super::parts::{
    PART_HEADER_LEN, PART_MAGIC, PartPaths, PartSize, SplitReader, SplitWriter,
    is_old_style_part_name, part_path, random_token, temp_path,
};
use super::wordlist;

/// A fresh empty folder that is deleted when dropped.
pub struct TestDir(PathBuf);

impl TestDir {
    pub fn new() -> Self {
        let token = random_token().unwrap();
        let path = std::env::temp_dir().join(format!("erx-vault-test-{token}"));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn entry_count(dir: &Path) -> usize {
    fs::read_dir(dir).unwrap().count()
}

/// Looks like an engine stream: 144 header bytes with the UUID at 12..28.
fn fake_stream(uuid_byte: u8, payload: usize) -> Vec<u8> {
    let mut data = vec![0u8; HEADER_LEN + payload];
    for byte in &mut data[12..28] {
        *byte = uuid_byte;
    }
    for (index, byte) in data.iter_mut().enumerate().skip(HEADER_LEN) {
        *byte = (index % 251) as u8;
    }
    data
}

fn part_bytes(uuid: [u8; 16], index: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&PART_MAGIC);
    out.extend_from_slice(&uuid);
    out.extend_from_slice(&index.to_le_bytes());
    out.extend_from_slice(payload);
    out
}

/// Writes `stream` and renames the temporary parts to their final names.
fn write_set(dir: &TestDir, name: &str, limit: Option<u64>, stream: &[u8]) -> Vec<PathBuf> {
    let base = dir.path().join(name);
    let mut writer = SplitWriter::new(&base, limit, "00ff00ff".to_owned()).unwrap();
    writer.write_all(stream).unwrap();
    let parts = writer.finish().unwrap();
    let mut finals = Vec::new();
    for part in &parts {
        fs::rename(&part.temp, &part.final_path).unwrap();
        finals.push(part.final_path.clone());
    }
    finals
}

fn joined_payloads(parts: &[PartPaths]) -> Vec<u8> {
    let mut all = Vec::new();
    for part in parts {
        let bytes = fs::read(&part.temp).unwrap();
        all.extend_from_slice(&bytes[PART_HEADER_LEN..]);
    }
    all
}

#[test]
fn part_size_presets_are_binary_and_below_fat32() {
    assert_eq!(PartSize::Off.limit_bytes(), None);
    assert_eq!(PartSize::Mib500.limit_bytes(), Some(500 * 1024 * 1024));
    assert_eq!(PartSize::Gib1.limit_bytes(), Some(1024 * 1024 * 1024));
    assert_eq!(PartSize::Gib2.limit_bytes(), Some(2 * 1024 * 1024 * 1024));
    let four_gib = 4u64 * 1024 * 1024 * 1024;
    assert!(PartSize::Gib2.limit_bytes().unwrap() < four_gib);
}

#[test]
fn old_style_part_names_are_recognized() {
    assert!(is_old_style_part_name(Path::new("Thefile.erx.001")));
    assert!(is_old_style_part_name(Path::new("Thefile.erx.002")));
    assert!(is_old_style_part_name(Path::new("X.ERX.001")));
    assert!(!is_old_style_part_name(Path::new("photo.jpg.001")));
    assert!(!is_old_style_part_name(Path::new("archive.001")));
    assert!(!is_old_style_part_name(Path::new("x.001.erx")));
    assert!(!is_old_style_part_name(Path::new("x.erx")));
}

#[test]
fn part_path_puts_the_number_before_the_erx_extension() {
    let dir = TestDir::new();
    let base = dir.path().join("Thefile.erx");
    assert_eq!(
        part_path(&base, true, 1),
        dir.path().join("Thefile.001.erx")
    );
    assert_eq!(
        part_path(&base, true, 12),
        dir.path().join("Thefile.012.erx")
    );
    assert_eq!(
        part_path(&base, true, 999),
        dir.path().join("Thefile.999.erx")
    );
    assert_eq!(part_path(&base, false, 7), base);

    // A dotted base keeps everything before the `.erx`.
    assert_eq!(
        part_path(&dir.path().join("doc.bin.erx"), true, 1),
        dir.path().join("doc.bin.001.erx")
    );

    // A base without `.erx` gets the number and extension appended.
    assert_eq!(
        part_path(&dir.path().join("plain"), true, 1),
        dir.path().join("plain.001.erx")
    );
}

#[test]
fn writer_uses_temporary_names_until_the_caller_renames() {
    let dir = TestDir::new();
    let base = dir.path().join("x.erx");
    let mut writer = SplitWriter::new(&base, Some(100), "abcd1234".to_owned()).unwrap();
    // 144 + 300 = 444 bytes, 72 payload bytes per part -> 7 parts.
    writer.write_all(&fake_stream(9, 300)).unwrap();
    let parts = writer.finish().unwrap();
    assert_eq!(parts.len(), 7);
    for (index, part) in parts.iter().enumerate() {
        let number = u32::try_from(index + 1).unwrap();
        assert_eq!(part.final_path, part_path(&base, true, number));
        assert_eq!(part.temp, temp_path(&part.final_path, "abcd1234"));
        assert!(part.temp.exists());
        assert!(!part.final_path.exists());
    }
    assert_eq!(fs::metadata(&parts[0].temp).unwrap().len(), 100);
    assert_eq!(fs::metadata(&parts[6].temp).unwrap().len(), 40);
    drop(writer);
    // After finish the files belong to the caller.
    assert!(parts.iter().all(|part| part.temp.exists()));
}

#[test]
fn dropping_or_aborting_a_writer_removes_every_temporary_file() {
    let dir = TestDir::new();
    let base = dir.path().join("x.erx");
    let mut writer = SplitWriter::new(&base, Some(100), "abcd1234".to_owned()).unwrap();
    writer.write_all(&fake_stream(1, 300)).unwrap();
    assert_eq!(entry_count(dir.path()), 7);
    drop(writer);
    assert_eq!(entry_count(dir.path()), 0);

    let mut writer = SplitWriter::new(&base, None, "abcd1234".to_owned()).unwrap();
    writer.write_all(&fake_stream(1, 300)).unwrap();
    assert_eq!(entry_count(dir.path()), 1);
    writer.abort();
    assert_eq!(entry_count(dir.path()), 0);
}

#[test]
fn bytes_before_the_header_is_complete_are_held_back() {
    let dir = TestDir::new();
    let base = dir.path().join("x.erx");
    let stream = fake_stream(4, 300);
    let mut writer = SplitWriter::new(&base, Some(100), "abcd1234".to_owned()).unwrap();
    writer.write_all(&stream[..7]).unwrap();
    assert_eq!(entry_count(dir.path()), 0);
    for piece in stream[7..].chunks(7) {
        writer.write_all(piece).unwrap();
    }
    let parts = writer.finish().unwrap();
    assert_eq!(parts.len(), 7);
    assert_eq!(joined_payloads(&parts), stream);
}

#[test]
fn every_part_header_carries_the_file_uuid_and_its_index() {
    let dir = TestDir::new();
    let base = dir.path().join("x.erx");
    let mut writer = SplitWriter::new(&base, Some(100), "abcd1234".to_owned()).unwrap();
    writer.write_all(&fake_stream(7, 300)).unwrap();
    let parts = writer.finish().unwrap();
    for (index, part) in parts.iter().enumerate() {
        let bytes = fs::read(&part.temp).unwrap();
        assert_eq!(&bytes[..8], PART_MAGIC.as_slice());
        assert_eq!(&bytes[8..24], [7u8; 16].as_slice());
        let number = u32::try_from(index + 1).unwrap();
        assert_eq!(&bytes[24..28], number.to_le_bytes().as_slice());
    }
}

#[test]
fn part_header_uuid_matches_the_real_engine_header_uuid() {
    let dir = TestDir::new();
    let base = dir.path().join("real.erx");
    let phrase = wordlist::words()
        .iter()
        .take(8)
        .copied()
        .collect::<Vec<&str>>()
        .join(" ");
    let mut writer = SplitWriter::new(&base, Some(5000), "feedbeef".to_owned()).unwrap();
    let mut source = Cursor::new(vec![5u8; 12_000]);
    encrypt_stream(
        &mut source,
        &mut writer,
        &phrase,
        &EncryptSettings::for_tests(false),
        &mut |_bytes: u64| true,
    )
    .unwrap();
    let parts = writer.finish().unwrap();
    assert!(parts.len() >= 3);
    let first = fs::read(&parts[0].temp).unwrap();
    let engine_header =
        Header::parse(&first[PART_HEADER_LEN..PART_HEADER_LEN + HEADER_LEN]).unwrap();
    for (index, part) in parts.iter().enumerate() {
        let bytes = fs::read(&part.temp).unwrap();
        assert_eq!(&bytes[..8], PART_MAGIC.as_slice());
        assert_eq!(&bytes[8..24], engine_header.file_uuid.as_slice());
        let number = u32::try_from(index + 1).unwrap();
        assert_eq!(&bytes[24..28], number.to_le_bytes().as_slice());
    }
}

#[test]
fn too_small_a_part_limit_is_refused() {
    let dir = TestDir::new();
    let base = dir.path().join("x.erx");
    assert_eq!(
        SplitWriter::new(&base, Some(10), "abcd1234".to_owned()).err(),
        Some(VaultError::Internal)
    );
}

#[test]
fn more_than_999_parts_are_refused_and_cleaned_up() {
    let dir = TestDir::new();
    let base = dir.path().join("x.erx");
    // 64-byte parts hold 36 payload bytes; 999 parts hold 35,964.
    let mut writer = SplitWriter::new(&base, Some(64), "abcd1234".to_owned()).unwrap();
    let error = writer.write_all(&fake_stream(2, 40_000)).unwrap_err();
    assert_eq!(VaultError::from(error), VaultError::TooManyParts);
    drop(writer);
    assert_eq!(entry_count(dir.path()), 0);
}

#[test]
fn reader_joins_all_parts_into_one_stream() {
    let dir = TestDir::new();
    let stream = fake_stream(3, 300);
    let finals = write_set(&dir, "x.erx", Some(100), &stream);
    assert_eq!(finals.len(), 7);
    let mut reader = SplitReader::open(&finals[0]).unwrap();
    assert_eq!(reader.logical_total(), stream.len() as u64);
    let mut all = Vec::new();
    reader.read_to_end(&mut all).unwrap();
    assert_eq!(all, stream);
}

#[test]
fn reader_handles_a_single_unsplit_file() {
    let dir = TestDir::new();
    let stream = fake_stream(3, 300);
    let finals = write_set(&dir, "x.erx", None, &stream);
    assert_eq!(finals.len(), 1);
    assert_eq!(finals[0], dir.path().join("x.erx"));
    let mut reader = SplitReader::open(&finals[0]).unwrap();
    let mut all = Vec::new();
    reader.read_to_end(&mut all).unwrap();
    assert_eq!(all, stream);
}

#[test]
fn reader_requires_the_first_part() {
    let dir = TestDir::new();
    let finals = write_set(&dir, "x.erx", Some(100), &fake_stream(3, 300));
    assert_eq!(
        SplitReader::open(&finals[1]).err(),
        Some(VaultError::NotFirstPart)
    );

    // A numbered file whose header says it is part 2.
    let liar = dir.path().join("y.001.erx");
    fs::write(&liar, part_bytes([3u8; 16], 2, &[1, 2, 3])).unwrap();
    assert_eq!(
        SplitReader::open(&liar).err(),
        Some(VaultError::NotFirstPart)
    );
}

#[test]
fn reader_rejects_files_that_are_not_parts() {
    let dir = TestDir::new();
    let junk = dir.path().join("junk.erx");
    fs::write(&junk, b"this is definitely not an ERX vault part file").unwrap();
    assert_eq!(
        SplitReader::open(&junk).err(),
        Some(VaultError::NotAVaultFile)
    );
    let empty = dir.path().join("empty.erx");
    fs::write(&empty, b"").unwrap();
    assert_eq!(
        SplitReader::open(&empty).err(),
        Some(VaultError::NotAVaultFile)
    );
    let missing = dir.path().join("missing.erx");
    assert_eq!(
        SplitReader::open(&missing).err(),
        Some(VaultError::CannotOpenFile)
    );
}

#[test]
fn reader_reports_a_missing_middle_part() {
    let dir = TestDir::new();
    let finals = write_set(&dir, "x.erx", Some(100), &fake_stream(3, 300));
    fs::remove_file(&finals[3]).unwrap();
    let mut reader = SplitReader::open(&finals[0]).unwrap();
    let mut all = Vec::new();
    let error = reader.read_to_end(&mut all).unwrap_err();
    assert_eq!(
        VaultError::from(error),
        VaultError::MissingPart { index: 4 }
    );
}

#[test]
fn a_missing_last_part_ends_the_stream_early() {
    let dir = TestDir::new();
    let stream = fake_stream(3, 300);
    let finals = write_set(&dir, "x.erx", Some(100), &stream);
    fs::remove_file(finals.last().unwrap()).unwrap();
    let mut reader = SplitReader::open(&finals[0]).unwrap();
    let mut all = Vec::new();
    reader.read_to_end(&mut all).unwrap();
    // The engine (not the reader) turns this into an authentication error.
    assert!(all.len() < stream.len());
}

#[test]
fn reader_rejects_a_part_of_another_file_or_in_the_wrong_place() {
    let dir = TestDir::new();
    let first = write_set(&dir, "a.erx", Some(100), &fake_stream(3, 300));
    let other = write_set(&dir, "b.erx", Some(100), &fake_stream(8, 300));

    // Part 2 of ANOTHER file in the middle: not part of this set, so the
    // reader never reads it and reports this set's part 2 as missing.
    let backup = dir.path().join("backup.tmp");
    fs::copy(&first[1], &backup).unwrap();
    fs::copy(&other[1], &first[1]).unwrap();
    let mut reader = SplitReader::open(&first[0]).unwrap();
    let mut all = Vec::new();
    let error = reader.read_to_end(&mut all).unwrap_err();
    assert_eq!(
        VaultError::from(error),
        VaultError::MissingPart { index: 2 }
    );

    // Restore, then swap parts 2 and 3: right file, wrong place.
    fs::copy(&backup, &first[1]).unwrap();
    let swap = dir.path().join("swap.tmp");
    fs::rename(&first[1], &swap).unwrap();
    fs::rename(&first[2], &first[1]).unwrap();
    fs::rename(&swap, &first[2]).unwrap();
    let mut reader = SplitReader::open(&first[0]).unwrap();
    let mut all = Vec::new();
    let error = reader.read_to_end(&mut all).unwrap_err();
    assert_eq!(VaultError::from(error), VaultError::WrongPart { index: 2 });
}
