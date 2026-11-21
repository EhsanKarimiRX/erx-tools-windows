use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::archive::{ArchiveReader, ArchiveWriter, scan_folder};
use super::engine::EncryptSettings;
use super::error::VaultError;
use super::files::{default_restore_path, encrypt_file, hidden_output_path};
use super::folder::{
    INCOMPLETE_MARKER, cleanup_stale_temp_dirs, decrypt_path, encrypt_path, output_path_for,
    peek_is_folder, plan_folder,
};
use super::meta::{FileMeta, KIND_FOLDER};
use super::parts::PartSize;
use super::parts_test::TestDir;
use super::wordlist;

type Tree = Vec<(String, Option<Vec<u8>>)>;

fn phrase_from(skip: usize) -> String {
    wordlist::words()
        .iter()
        .skip(skip)
        .take(8)
        .copied()
        .collect::<Vec<&str>>()
        .join(" ")
}

fn passphrase() -> String {
    phrase_from(0)
}

fn data_of(len: usize, seed: usize) -> Vec<u8> {
    (0..len)
        .map(|index| ((index * 31 + seed) % 251) as u8)
        .collect()
}

/// Deterministic, practically incompressible test data.
fn noise(len: usize, seed: u64) -> Vec<u8> {
    let mut state = seed | 1;
    let mut out = Vec::with_capacity(len + 8);
    while out.len() < len {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        out.extend_from_slice(&state.to_le_bytes());
    }
    out.truncate(len);
    out
}

/// 4 files, 3 folders (one empty), one empty file, one non-ASCII name.
fn make_tree(root: &Path) {
    fs::create_dir_all(root.join("docs").join("sub")).unwrap();
    fs::create_dir_all(root.join("empty")).unwrap();
    fs::write(root.join("a.txt"), b"alpha").unwrap();
    fs::write(root.join("docs").join("b.bin"), data_of(300_000, 3)).unwrap();
    fs::write(root.join("docs").join("sub").join("c.txt"), b"").unwrap();
    fs::write(
        root.join("docs").join("sub").join("d\u{e9}.txt"),
        b"unicode",
    )
    .unwrap();
}

fn collect(dir: &Path, prefix: &str, out: &mut Tree) {
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        let relative = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if entry.file_type().unwrap().is_dir() {
            out.push((relative.clone(), None));
            collect(&entry.path(), &relative, out);
        } else {
            out.push((relative, Some(fs::read(entry.path()).unwrap())));
        }
    }
}

/// Every item below `root` with its content, sorted.
fn snapshot(root: &Path) -> Tree {
    let mut out = Vec::new();
    collect(root, "", &mut out);
    out.sort();
    out
}

fn names(dir: &Path) -> Vec<String> {
    let mut all: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    all.sort();
    all
}

fn encrypt_tree(source: &Path, out_dir: &Path, limit: Option<u64>, compress: bool) -> Vec<PathBuf> {
    let base = output_path_for(source, out_dir).unwrap();
    encrypt_path(
        source,
        &base,
        limit,
        &passphrase(),
        &EncryptSettings::for_tests(compress),
        false,
        &mut |_done: u64, _total: u64| true,
    )
    .unwrap()
    .parts
}

fn restore_tree(first: &Path, out_dir: &Path) -> Result<PathBuf, VaultError> {
    let fallback = default_restore_path(first, out_dir).unwrap();
    decrypt_path(
        first,
        &fallback,
        &passphrase(),
        false,
        &mut |_done: u64, _total: u64| true,
    )
    .map(|summary| summary.output_path)
}

fn raw_entry(tag: u8, path: &[u8], size: Option<u64>, content: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    out.extend_from_slice(&(path.len() as u16).to_le_bytes());
    out.extend_from_slice(path);
    if let Some(size) = size {
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(content);
    }
    out
}

fn dir_entry(path: &[u8]) -> Vec<u8> {
    raw_entry(1, path, None, &[])
}

fn file_entry(path: &[u8], content: &[u8]) -> Vec<u8> {
    raw_entry(2, path, Some(content.len() as u64), content)
}

fn with_end(mut bytes: Vec<u8>) -> Vec<u8> {
    bytes.push(0);
    bytes
}

fn feed(root: &Path, bytes: &[u8]) -> Result<(), VaultError> {
    let mut writer = ArchiveWriter::new(root.to_path_buf());
    writer.write_all(bytes).map_err(VaultError::from)?;
    writer.finish().map(|_| ())
}

#[test]
fn a_folder_round_trips_under_its_own_name_and_hides_that_name() {
    let dir = TestDir::new();
    let out = TestDir::new();
    let source = dir.path().join("Photos");
    make_tree(&source);

    let parts = encrypt_tree(&source, dir.path(), None, true);
    assert_eq!(parts, vec![dir.path().join("Photos.erx")]);
    assert!(peek_is_folder(&parts[0]).unwrap());
    let bytes = fs::read(&parts[0]).unwrap();
    assert!(!bytes.windows(6).any(|window| window == b"Photos"));
    assert!(!bytes.windows(5).any(|window| window == b"b.bin"));

    let restored = restore_tree(&parts[0], out.path()).unwrap();
    assert_eq!(restored, out.path().join("Photos"));
    assert_eq!(snapshot(&restored), snapshot(&source));
    assert_eq!(names(out.path()), ["Photos"]);
}

#[test]
fn a_dot_in_the_folder_name_is_kept() {
    let dir = TestDir::new();
    let out = TestDir::new();
    let source = dir.path().join("Photos.v2");
    make_tree(&source);
    let parts = encrypt_tree(&source, dir.path(), None, false);
    assert_eq!(parts, vec![dir.path().join("Photos.v2.erx")]);
    let restored = restore_tree(&parts[0], out.path()).unwrap();
    assert_eq!(restored, out.path().join("Photos.v2"));
    assert_eq!(snapshot(&restored), snapshot(&source));
}

#[test]
fn an_existing_folder_is_never_replaced() {
    let dir = TestDir::new();
    let out = TestDir::new();
    let source = dir.path().join("Photos");
    make_tree(&source);
    let parts = encrypt_tree(&source, dir.path(), None, false);

    fs::create_dir(out.path().join("Photos")).unwrap();
    fs::write(out.path().join("Photos").join("mine.txt"), b"mine").unwrap();

    let first = restore_tree(&parts[0], out.path()).unwrap();
    assert_eq!(first, out.path().join("Photos (1)"));
    let second = restore_tree(&parts[0], out.path()).unwrap();
    assert_eq!(second, out.path().join("Photos (2)"));
    assert_eq!(
        fs::read(out.path().join("Photos").join("mine.txt")).unwrap(),
        b"mine"
    );
    assert_eq!(snapshot(&first), snapshot(&source));
}

#[test]
fn a_split_folder_round_trips() {
    let dir = TestDir::new();
    let out = TestDir::new();
    let source = dir.path().join("Photos");
    make_tree(&source);
    let parts = encrypt_tree(&source, dir.path(), Some(100_000), false);
    assert!(parts.len() >= 3);
    assert_eq!(parts[0], dir.path().join("Photos.001.erx"));
    let restored = restore_tree(&parts[0], out.path()).unwrap();
    assert_eq!(restored, out.path().join("Photos"));
    assert_eq!(snapshot(&restored), snapshot(&source));
}

#[test]
fn the_scan_describes_the_stream_exactly() {
    let dir = TestDir::new();
    let source = dir.path().join("Photos");
    make_tree(&source);
    let scan = scan_folder(&source).unwrap();
    let relatives: Vec<&str> = scan
        .entries
        .iter()
        .map(|entry| entry.relative.as_str())
        .collect();
    assert_eq!(
        relatives,
        [
            "a.txt",
            "docs",
            "docs/b.bin",
            "docs/sub",
            "docs/sub/c.txt",
            "docs/sub/d\u{e9}.txt",
            "empty"
        ]
    );
    assert_eq!(scan.file_count, 4);
    assert_eq!(scan.dir_count, 3);

    let mut stream = Vec::new();
    ArchiveReader::new(scan.clone())
        .read_to_end(&mut stream)
        .unwrap();
    assert_eq!(stream.len() as u64, scan.stream_len);
    assert_eq!(scan_folder(&source).unwrap(), scan);
}

#[test]
fn the_archive_can_be_taken_apart_in_tiny_pieces() {
    let dir = TestDir::new();
    let source = dir.path().join("Photos");
    make_tree(&source);
    let scan = scan_folder(&source).unwrap();
    let mut stream = Vec::new();
    ArchiveReader::new(scan).read_to_end(&mut stream).unwrap();

    let target = TestDir::new();
    let mut writer = ArchiveWriter::new(target.path().to_path_buf());
    for piece in stream.chunks(7) {
        writer.write_all(piece).unwrap();
    }
    let stats = writer.finish().unwrap();
    assert_eq!(stats.files, 4);
    assert_eq!(stats.dirs, 3);
    assert_eq!(stats.bytes, 5 + 300_000 + 7);
    assert_eq!(snapshot(target.path()), snapshot(&source));
}

#[test]
fn hostile_archives_are_refused_and_write_nothing() {
    let bad_paths: [&[u8]; 8] = [
        b"..",
        b"a/../b",
        b"C:evil.txt",
        b"nul.txt",
        b"a\\b",
        b"/abs.txt",
        b"dir//x.txt",
        b"ok/",
    ];
    for path in bad_paths {
        let dir = TestDir::new();
        let result = feed(dir.path(), &with_end(file_entry(path, b"x")));
        assert_eq!(
            result,
            Err(VaultError::UnsafeArchive),
            "{:?}",
            String::from_utf8_lossy(path)
        );
        assert!(names(dir.path()).is_empty());
    }

    let dir = TestDir::new();
    assert_eq!(
        feed(dir.path(), &with_end(dir_entry(b""))),
        Err(VaultError::UnsafeArchive)
    );
    let dir = TestDir::new();
    assert_eq!(
        feed(dir.path(), &with_end(dir_entry(&[0xff, 0xfe]))),
        Err(VaultError::UnsafeArchive)
    );
    let dir = TestDir::new();
    assert_eq!(
        feed(dir.path(), &with_end(file_entry(b"missing/x.txt", b"x"))),
        Err(VaultError::UnsafeArchive)
    );
    let dir = TestDir::new();
    let mut twice = dir_entry(b"d");
    twice.extend(dir_entry(b"d"));
    assert_eq!(
        feed(dir.path(), &with_end(twice)),
        Err(VaultError::UnsafeArchive)
    );
}

#[test]
fn broken_archives_are_refused() {
    let dir = TestDir::new();
    assert_eq!(feed(dir.path(), &[9]), Err(VaultError::CorruptedData));
    let dir = TestDir::new();
    assert_eq!(feed(dir.path(), &[0, 0]), Err(VaultError::CorruptedData));
    let dir = TestDir::new();
    assert_eq!(feed(dir.path(), &[]), Err(VaultError::CorruptedData));
    let dir = TestDir::new();
    assert_eq!(
        feed(dir.path(), &dir_entry(b"d")),
        Err(VaultError::CorruptedData)
    );
    let dir = TestDir::new();
    let mut cut = file_entry(b"x.txt", b"hello");
    cut.truncate(cut.len() - 2);
    assert_eq!(feed(dir.path(), &cut), Err(VaultError::CorruptedData));
}

#[test]
fn unsafe_names_in_the_source_are_refused_with_their_path() {
    let dir = TestDir::new();
    let source = dir.path().join("Docs");
    fs::create_dir_all(source.join("sub")).unwrap();
    fs::write(source.join("sub").join("a\u{200f}b.txt"), b"x").unwrap();
    match scan_folder(&source) {
        Err(VaultError::UnsupportedName { path }) => {
            assert_eq!(path, "sub/a\u{200f}b.txt");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn very_deep_folders_are_refused() {
    let dir = TestDir::new();
    let root = dir.path().join("Deep");
    let mut deep = root.clone();
    for _ in 0..66 {
        deep.push("d");
    }
    fs::create_dir_all(&deep).unwrap();
    assert_eq!(scan_folder(&root).unwrap_err(), VaultError::FolderTooBig);
}

#[test]
fn the_output_cannot_go_inside_the_folder_being_encrypted() {
    let dir = TestDir::new();
    let source = dir.path().join("Photos");
    make_tree(&source);
    let before = snapshot(&source);

    let result = encrypt_path(
        &source,
        &source.join("Photos.erx"),
        None,
        &passphrase(),
        &EncryptSettings::for_tests(false),
        false,
        &mut |_done: u64, _total: u64| true,
    );
    assert_eq!(result.unwrap_err(), VaultError::OutputInsideFolder);

    let inside = source.join("docs");
    assert_eq!(
        plan_folder(&source, Some(&inside), PartSize::Off).unwrap_err(),
        VaultError::OutputInsideFolder
    );
    assert_eq!(
        plan_folder(&source, Some(&source), PartSize::Off).unwrap_err(),
        VaultError::OutputInsideFolder
    );
    assert_eq!(snapshot(&source), before);
}

#[test]
fn failures_leave_no_temporary_folders_behind() {
    let dir = TestDir::new();
    let source = dir.path().join("Docs");
    make_tree(&source);
    fs::write(source.join("big.bin"), noise(2_300_000, 5)).unwrap();
    let parts = encrypt_tree(&source, dir.path(), None, false);

    let out = TestDir::new();
    let fallback = default_restore_path(&parts[0], out.path()).unwrap();

    // Wrong words.
    let wrong = decrypt_path(
        &parts[0],
        &fallback,
        &phrase_from(100),
        false,
        &mut |_done: u64, _total: u64| true,
    );
    assert_eq!(wrong.unwrap_err(), VaultError::WrongPassphraseOrHeader);
    assert!(names(out.path()).is_empty());

    // Cancel while decrypting (the archive spans several chunks).
    let cancelled = decrypt_path(
        &parts[0],
        &fallback,
        &passphrase(),
        false,
        &mut |_done: u64, _total: u64| false,
    );
    assert_eq!(cancelled.unwrap_err(), VaultError::Cancelled);
    assert!(names(out.path()).is_empty());

    // Cancel while encrypting.
    let target = dir.path().join("Other.erx");
    let result = encrypt_path(
        &source,
        &target,
        None,
        &passphrase(),
        &EncryptSettings::for_tests(false),
        false,
        &mut |_done: u64, _total: u64| false,
    );
    assert_eq!(result.unwrap_err(), VaultError::Cancelled);
    assert!(!target.exists());
    assert!(
        names(dir.path())
            .iter()
            .all(|name| !name.ends_with(".erxtmp"))
    );

    // A damaged last chunk is only noticed after files were written; the
    // half-restored folder must disappear.
    let mut bytes = fs::read(&parts[0]).unwrap();
    let last = bytes.len() - 20;
    bytes[last] ^= 0x01;
    fs::write(&parts[0], bytes).unwrap();
    let damaged = decrypt_path(
        &parts[0],
        &fallback,
        &passphrase(),
        false,
        &mut |_done: u64, _total: u64| true,
    );
    assert_eq!(damaged.unwrap_err(), VaultError::CorruptedData);
    assert!(names(out.path()).is_empty());
}

#[test]
fn stale_temporary_folders_are_removed_only_when_marked() {
    let dir = TestDir::new();
    let marked = dir.path().join("a.0123abcd.erxtmpdir");
    fs::create_dir(&marked).unwrap();
    fs::write(marked.join(INCOMPLETE_MARKER), b"").unwrap();
    fs::write(marked.join("x.txt"), b"x").unwrap();
    fs::create_dir(dir.path().join("b.0123abcd.erxtmpdir")).unwrap();
    fs::create_dir(dir.path().join("c.erxtmpdir")).unwrap();
    fs::create_dir(dir.path().join("keep")).unwrap();

    assert_eq!(
        cleanup_stale_temp_dirs(dir.path(), Duration::from_secs(3600)),
        0
    );
    assert_eq!(cleanup_stale_temp_dirs(dir.path(), Duration::ZERO), 1);
    assert_eq!(
        names(dir.path()),
        ["b.0123abcd.erxtmpdir", "c.erxtmpdir", "keep"]
    );
}

#[test]
fn the_plan_describes_a_folder() {
    let dir = TestDir::new();
    let source = dir.path().join("Photos");
    make_tree(&source);

    let plan = plan_folder(&source, None, PartSize::Off).unwrap();
    assert!(plan.is_folder);
    assert_eq!(plan.item_count, 4);
    assert_eq!(plan.file_name, "Photos");
    assert_eq!(plan.out_dir.as_path(), dir.path());
    assert_eq!(plan.base_path, dir.path().join("Photos.erx"));
    assert_eq!(plan.size_bytes, scan_folder(&source).unwrap().stream_len);
    assert!(plan.conflicts.is_empty());
    assert!(plan.compress_by_default);

    fs::write(dir.path().join("Photos.erx"), b"old").unwrap();
    let plan = plan_folder(&source, None, PartSize::Off).unwrap();
    assert_eq!(plan.conflicts, vec![dir.path().join("Photos.erx")]);
}

#[test]
fn single_files_still_go_through_the_file_path() {
    let dir = TestDir::new();
    let out = TestDir::new();
    let source = dir.path().join("doc.txt");
    fs::write(&source, b"just a file").unwrap();
    let base = hidden_output_path(&source, dir.path()).unwrap();
    assert_eq!(output_path_for(&source, dir.path()), Some(base.clone()));
    encrypt_file(
        &source,
        &base,
        None,
        &passphrase(),
        &EncryptSettings::for_tests(false),
        false,
        &mut |_done: u64, _total: u64| true,
    )
    .unwrap();
    assert!(!peek_is_folder(&base).unwrap());
    let restored = restore_tree(&base, out.path()).unwrap();
    assert_eq!(restored, out.path().join("doc.txt"));
    assert_eq!(fs::read(&restored).unwrap(), b"just a file");
}

#[test]
fn a_file_that_changes_while_it_is_read_is_reported() {
    for replacement in [&b"abc"[..], &b"alpha and more"[..]] {
        let dir = TestDir::new();
        let source = dir.path().join("Docs");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("a.txt"), b"alpha").unwrap();
        let scan = scan_folder(&source).unwrap();
        fs::write(source.join("a.txt"), replacement).unwrap();
        let mut reader = ArchiveReader::new(scan);
        let mut out = Vec::new();
        let error = reader.read_to_end(&mut out).unwrap_err();
        assert_eq!(
            VaultError::from(error),
            VaultError::FileChanged {
                path: "a.txt".to_owned()
            }
        );
    }
}

#[test]
fn folder_metadata_round_trips() {
    let block = FileMeta::folder("Photos").unwrap().encode().unwrap();
    let meta = FileMeta::decode(&block).unwrap();
    assert_eq!(meta.kind, KIND_FOLDER);
    assert_eq!(meta.name, "Photos");
}
