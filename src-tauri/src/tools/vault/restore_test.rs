use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use super::engine::{EncryptSettings, decrypt_stream, encrypt_stream, encrypt_stream_named};
use super::error::VaultError;
use super::files::{
    decrypt_file_restoring_name, default_restore_path, encrypt_file, hidden_output_path,
};
use super::meta::{FileMeta, KIND_FILE, MAX_NAME_BYTES, META_LEN};
use super::names::{numbered_name, safe_file_name};
use super::parts::SplitWriter;
use super::parts_test::TestDir;
use super::wordlist;

fn passphrase() -> String {
    wordlist::words()
        .iter()
        .take(8)
        .copied()
        .collect::<Vec<&str>>()
        .join(" ")
}

fn data_of(len: usize) -> Vec<u8> {
    (0..len).map(|index| (index * 31 % 251) as u8).collect()
}

/// Encrypts `<dir>/<name>` with the app's real output naming.
fn encrypt_named_file(dir: &TestDir, name: &str, data: &[u8]) -> PathBuf {
    let source = dir.path().join(name);
    fs::write(&source, data).unwrap();
    let base = hidden_output_path(&source, dir.path()).unwrap();
    let summary = encrypt_file(
        &source,
        &base,
        None,
        &passphrase(),
        &EncryptSettings::for_tests(false),
        false,
        &mut |_done: u64, _total: u64| true,
    )
    .unwrap();
    summary.parts[0].clone()
}

fn restore(first: &Path, fallback: &Path, overwrite: bool) -> PathBuf {
    decrypt_file_restoring_name(
        first,
        fallback,
        &passphrase(),
        overwrite,
        &mut |_done: u64, _total: u64| true,
    )
    .unwrap()
    .output_path
}

/// Builds a vault file whose stored name is exactly `stored` (even a
/// hostile one) by calling the engine directly.
fn encrypt_with_stored_name(dir: &TestDir, file_name: &str, stored: &str, data: &[u8]) -> PathBuf {
    let base = dir.path().join(file_name);
    let mut writer = SplitWriter::new(&base, None, "00ff00ff".to_owned()).unwrap();
    encrypt_stream_named(
        &mut Cursor::new(data.to_vec()),
        &mut writer,
        &passphrase(),
        &EncryptSettings::for_tests(false),
        stored,
        &mut |_bytes: u64| true,
    )
    .unwrap();
    for part in writer.finish().unwrap() {
        fs::rename(&part.temp, &part.final_path).unwrap();
    }
    base
}

#[test]
fn metadata_block_round_trips_and_has_a_fixed_size() {
    for name in ["", "a", "202ERX.jpg", "na\u{ef}ve file.txt"] {
        let block = FileMeta::file(name).unwrap().encode().unwrap();
        assert_eq!(block.len(), META_LEN);
        let meta = FileMeta::decode(&block).unwrap();
        assert_eq!(meta.kind, KIND_FILE);
        assert_eq!(meta.name, name);
    }
    assert!(FileMeta::file(&"a".repeat(MAX_NAME_BYTES)).is_ok());
    assert!(FileMeta::file(&"a".repeat(MAX_NAME_BYTES + 1)).is_err());
}

#[test]
fn damaged_metadata_blocks_are_refused() {
    let good = FileMeta::file("x.txt").unwrap().encode().unwrap();
    assert!(FileMeta::decode(&good[..META_LEN - 1]).is_err());

    let mut bad = good;
    bad[0] = 2;
    assert_eq!(
        FileMeta::decode(&bad).unwrap_err(),
        VaultError::UnsupportedVersion
    );

    let mut bad = good;
    bad[META_LEN - 1] = 1;
    assert_eq!(
        FileMeta::decode(&bad).unwrap_err(),
        VaultError::CorruptedData
    );

    let mut bad = good;
    bad[1] = 0xff;
    bad[2] = 0xff;
    assert_eq!(
        FileMeta::decode(&bad).unwrap_err(),
        VaultError::CorruptedData
    );

    let mut bad = good;
    bad[3] = 0xff;
    assert_eq!(
        FileMeta::decode(&bad).unwrap_err(),
        VaultError::CorruptedData
    );
}

#[test]
fn ordinary_names_are_accepted() {
    let names = [
        "202ERX.jpg",
        "report final (2).docx",
        ".gitignore",
        "na\u{ef}ve.txt",
        "archive.tar.gz",
        "noext",
        "a",
    ];
    for name in names {
        assert_eq!(safe_file_name(name).as_deref(), Some(name), "{name}");
    }
    assert!(safe_file_name(&"a".repeat(255)).is_some());
}

#[test]
fn dangerous_names_are_refused() {
    let bad = [
        "",
        ".",
        "..",
        "a/b",
        "a\\b",
        "..\\evil.exe",
        "C:evil.txt",
        "evil.txt:stream",
        "con",
        "CON",
        "nul.txt",
        "Com1.log",
        "LPT9",
        "aux .txt",
        "name.",
        "name ",
        " name",
        "a<b",
        "a>b",
        "a\"b",
        "a|b",
        "a?b",
        "a*b",
        "a\u{0}b",
        "a\nb",
        "x\u{202e}fdp.exe",
        "x\u{200f}y",
        "a.0123abcd.erxtmp",
    ];
    for name in bad {
        assert!(safe_file_name(name).is_none(), "{name:?}");
    }
    assert!(safe_file_name(&"a".repeat(256)).is_none());
}

#[test]
fn numbered_names_keep_the_extension() {
    assert_eq!(numbered_name("photo.jpg", 1), "photo (1).jpg");
    assert_eq!(numbered_name("archive.tar.gz", 2), "archive.tar (2).gz");
    assert_eq!(numbered_name("noext", 3), "noext (3)");
    assert_eq!(numbered_name(".gitignore", 1), ".gitignore (1)");
}

#[test]
fn output_names_hide_the_file_type() {
    let out = Path::new("out");
    assert_eq!(
        hidden_output_path(Path::new("202ERX.jpg"), out),
        Some(out.join("202ERX.erx"))
    );
    assert_eq!(
        hidden_output_path(Path::new("noext"), out),
        Some(out.join("noext.erx"))
    );
    assert_eq!(
        hidden_output_path(Path::new("archive.tar.gz"), out),
        Some(out.join("archive.tar.erx"))
    );
    assert_eq!(
        hidden_output_path(Path::new(".gitignore"), out),
        Some(out.join(".gitignore.erx"))
    );
}

#[test]
fn the_stored_name_comes_back_from_the_engine_and_is_not_visible() {
    let settings = EncryptSettings::for_tests(false);
    let mut file = Vec::new();
    encrypt_stream_named(
        &mut Cursor::new(b"payload".to_vec()),
        &mut file,
        &passphrase(),
        &settings,
        "202ERX.jpg",
        &mut |_bytes: u64| true,
    )
    .unwrap();
    assert!(!file.windows(6).any(|window| window == b"202ERX"));
    let mut restored = Vec::new();
    let summary = decrypt_stream(
        &mut Cursor::new(file),
        &mut restored,
        &passphrase(),
        &mut |_bytes: u64| true,
    )
    .unwrap();
    assert_eq!(summary.meta.name, "202ERX.jpg");
    assert_eq!(summary.output_bytes, 7);
    assert_eq!(restored, b"payload");

    // The plain function stores an empty name.
    let mut file = Vec::new();
    encrypt_stream(
        &mut Cursor::new(b"x".to_vec()),
        &mut file,
        &passphrase(),
        &settings,
        &mut |_bytes: u64| true,
    )
    .unwrap();
    let summary = decrypt_stream(
        &mut Cursor::new(file),
        &mut Vec::new(),
        &passphrase(),
        &mut |_bytes: u64| true,
    )
    .unwrap();
    assert_eq!(summary.meta.name, "");
}

#[test]
fn the_type_is_hidden_in_the_file_name_and_restored_on_decrypt() {
    let dir = TestDir::new();
    let out = TestDir::new();
    let data = data_of(30_000);
    let encrypted = encrypt_named_file(&dir, "202ERX.jpg", &data);
    assert_eq!(encrypted, dir.path().join("202ERX.erx"));
    let bytes = fs::read(&encrypted).unwrap();
    assert!(!bytes.windows(6).any(|window| window == b"202ERX"));

    let fallback = default_restore_path(&encrypted, out.path()).unwrap();
    assert_eq!(fallback, out.path().join("202ERX"));
    let restored = restore(&encrypted, &fallback, false);
    assert_eq!(restored, out.path().join("202ERX.jpg"));
    assert_eq!(fs::read(&restored).unwrap(), data);

    // Renaming the encrypted file changes nothing: the name is inside.
    let renamed = dir.path().join("S.erx");
    fs::rename(&encrypted, &renamed).unwrap();
    let other = TestDir::new();
    let fallback = default_restore_path(&renamed, other.path()).unwrap();
    assert_eq!(
        restore(&renamed, &fallback, false),
        other.path().join("202ERX.jpg")
    );
}

#[test]
fn an_existing_file_is_numbered_and_only_replaced_with_permission() {
    let dir = TestDir::new();
    let out = TestDir::new();
    let data = data_of(5_000);
    let encrypted = encrypt_named_file(&dir, "202ERX.jpg", &data);
    let fallback = default_restore_path(&encrypted, out.path()).unwrap();
    fs::write(out.path().join("202ERX.jpg"), b"mine").unwrap();

    let first = restore(&encrypted, &fallback, false);
    assert_eq!(first, out.path().join("202ERX (1).jpg"));
    let second = restore(&encrypted, &fallback, false);
    assert_eq!(second, out.path().join("202ERX (2).jpg"));
    assert_eq!(fs::read(out.path().join("202ERX.jpg")).unwrap(), b"mine");
    assert_eq!(fs::read(&first).unwrap(), data);

    let replaced = restore(&encrypted, &fallback, true);
    assert_eq!(replaced, out.path().join("202ERX.jpg"));
    assert_eq!(fs::read(&replaced).unwrap(), data);
}

#[test]
fn hostile_stored_names_never_leave_the_output_folder() {
    let stored_names = [
        "..\\..\\evil.exe",
        "a/b.txt",
        "NUL.txt",
        "con",
        "",
        "x\u{202e}fdp.exe",
        "trailing.",
        "C:evil.txt",
    ];
    for stored in stored_names {
        let src = TestDir::new();
        let out = TestDir::new();
        let encrypted = encrypt_with_stored_name(&src, "in.erx", stored, b"data");
        let fallback = out.path().join("fallback");
        let restored = restore(&encrypted, &fallback, false);
        assert_eq!(restored, fallback, "stored name {stored:?}");
        assert_eq!(fs::read(&fallback).unwrap(), b"data");
        let outside = out.path().parent().unwrap().join("evil.exe");
        assert!(!outside.exists());
    }
}
