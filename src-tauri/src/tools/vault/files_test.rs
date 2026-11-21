use std::fs;
use std::path::{Path, PathBuf};

use super::engine::EncryptSettings;
use super::error::VaultError;
use super::files::{
    DecryptFileSummary, EncryptFileSummary, decrypt_file, default_output_path,
    default_restore_path, encrypt_file, find_conflicts, hidden_output_path,
};
use super::parts::part_path;
use super::parts_test::TestDir;
use super::runtime::count_parts;
use super::space::{free_bytes, required_for_encrypt};
use super::wordlist;

const MIB: usize = 1024 * 1024;

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

fn names(dir: &Path) -> Vec<String> {
    let mut all: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    all.sort();
    all
}

fn no_temp_files(dir: &Path) -> bool {
    names(dir).iter().all(|name| !name.ends_with(".erxtmp"))
}

/// Old-style part names (`base.erx.001`) used only to prove that the part
/// headers, not the names, hold a set together.
fn old_style_parts(base: &Path, count: usize) -> Vec<PathBuf> {
    (1..=count)
        .map(|index| {
            let mut name = base.as_os_str().to_os_string();
            name.push(format!(".{index:03}"));
            PathBuf::from(name)
        })
        .collect()
}

/// Writes `data` to `<dir>/<name>` and encrypts it (uncompressed).
fn encrypt(
    dir: &TestDir,
    name: &str,
    data: &[u8],
    limit: Option<u64>,
) -> (PathBuf, PathBuf, EncryptFileSummary) {
    let source = dir.path().join(name);
    fs::write(&source, data).unwrap();
    let base = default_output_path(&source, dir.path()).unwrap();
    let summary = encrypt_file(
        &source,
        &base,
        limit,
        &passphrase(),
        &EncryptSettings::for_tests(false),
        false,
        &mut |_done: u64, _total: u64| true,
    )
    .unwrap();
    (source, base, summary)
}

fn decrypt(first: &Path, out: &Path, phrase: &str) -> Result<DecryptFileSummary, VaultError> {
    decrypt_file(first, out, phrase, false, &mut |_done: u64, _total: u64| {
        true
    })
}

#[test]
fn round_trip_without_splitting() {
    let dir = TestDir::new();
    let data = noise(300_000, 1);
    let (_source, base, summary) = encrypt(&dir, "doc.txt", &data, None);
    assert_eq!(summary.parts, vec![base.clone()]);
    assert_eq!(summary.input_bytes, 300_000);
    assert_eq!(summary.output_bytes, fs::metadata(&base).unwrap().len());
    assert_eq!(names(dir.path()), ["doc.txt", "doc.txt.erx"]);

    let out = dir.path().join("restored.txt");
    let restored = decrypt(&base, &out, &passphrase()).unwrap();
    assert_eq!(restored.output_bytes, 300_000);
    assert_eq!(restored.output_path, out);
    assert_eq!(fs::read(&out).unwrap(), data);
    assert!(no_temp_files(dir.path()));
}

#[test]
fn split_layout_is_exact_and_round_trips() {
    let dir = TestDir::new();
    let data = noise(50_000, 2);
    // Stream = 144 header + 50,000 + 1,064 trailer + 16 tag = 51,224 bytes.
    // 4096-byte parts carry 4068 payload bytes -> 12 full parts + 2,408.
    let (_source, base, summary) = encrypt(&dir, "doc.txt", &data, Some(4096));
    assert_eq!(summary.parts.len(), 13);
    assert_eq!(summary.parts[0], dir.path().join("doc.txt.001.erx"));
    assert_eq!(summary.output_bytes, 51_588);
    for (index, part) in summary.parts.iter().enumerate() {
        let number = u32::try_from(index + 1).unwrap();
        assert_eq!(part, &part_path(&base, true, number));
        let length = fs::metadata(part).unwrap().len();
        if index < 12 {
            assert_eq!(length, 4096);
        } else {
            assert_eq!(length, 2436);
        }
    }
    assert!(no_temp_files(dir.path()));

    let out = dir.path().join("restored.txt");
    decrypt(&summary.parts[0], &out, &passphrase()).unwrap();
    assert_eq!(fs::read(&out).unwrap(), data);
}

#[test]
fn a_chunk_may_span_two_parts() {
    let dir = TestDir::new();
    let data = noise(2_500_000, 3);
    let (_source, _base, summary) = encrypt(&dir, "big.bin", &data, Some(1_000_000));
    assert_eq!(summary.parts.len(), 3);
    let out = dir.path().join("restored.bin");
    decrypt(&summary.parts[0], &out, &passphrase()).unwrap();
    assert_eq!(fs::read(&out).unwrap(), data);
}

#[test]
fn compressible_data_shrinks_and_round_trips() {
    let dir = TestDir::new();
    let source = dir.path().join("zeros.bin");
    let data = vec![0u8; 3 * MIB];
    fs::write(&source, &data).unwrap();
    let base = default_output_path(&source, dir.path()).unwrap();
    let summary = encrypt_file(
        &source,
        &base,
        None,
        &passphrase(),
        &EncryptSettings::for_tests(true),
        false,
        &mut |_done: u64, _total: u64| true,
    )
    .unwrap();
    assert!(summary.output_bytes < 3000);
    let out = dir.path().join("restored.bin");
    decrypt(&base, &out, &passphrase()).unwrap();
    assert_eq!(fs::read(&out).unwrap(), data);
}

#[test]
fn empty_files_round_trip_split_and_unsplit() {
    for limit in [None, Some(4096)] {
        let dir = TestDir::new();
        let (_source, _base, summary) = encrypt(&dir, "empty.txt", b"", limit);
        let out = dir.path().join("restored.txt");
        decrypt(&summary.parts[0], &out, &passphrase()).unwrap();
        assert_eq!(fs::read(&out).unwrap(), b"");
    }
}

#[test]
fn a_missing_middle_part_is_reported_by_number() {
    let dir = TestDir::new();
    let (_source, _base, summary) = encrypt(&dir, "doc.txt", &noise(50_000, 4), Some(4096));
    fs::remove_file(&summary.parts[2]).unwrap();
    let out = dir.path().join("restored.txt");
    assert_eq!(
        decrypt(&summary.parts[0], &out, &passphrase()).unwrap_err(),
        VaultError::MissingPart { index: 3 }
    );
    assert!(!out.exists());
    assert!(no_temp_files(dir.path()));
}

#[test]
fn a_missing_last_part_is_detected_as_incomplete() {
    let dir = TestDir::new();
    let (_source, _base, summary) = encrypt(&dir, "doc.txt", &noise(50_000, 5), Some(4096));
    fs::remove_file(summary.parts.last().unwrap()).unwrap();
    let out = dir.path().join("restored.txt");
    assert_eq!(
        decrypt(&summary.parts[0], &out, &passphrase()).unwrap_err(),
        VaultError::CorruptedData
    );
    assert!(!out.exists());
    assert!(no_temp_files(dir.path()));
}

#[test]
fn swapped_parts_and_parts_of_another_file_are_rejected() {
    let dir = TestDir::new();
    let (_s1, _b1, first) = encrypt(&dir, "a.bin", &noise(50_000, 6), Some(4096));
    let (_s2, _b2, second) = encrypt(&dir, "b.bin", &noise(50_000, 7), Some(4096));
    let out = dir.path().join("restored.bin");

    // Part 2 from the other file: it belongs to another set, so part 2 of
    // THIS set is reported as missing and the foreign data is never read.
    let backup = dir.path().join("backup.tmp");
    fs::copy(&first.parts[1], &backup).unwrap();
    fs::copy(&second.parts[1], &first.parts[1]).unwrap();
    assert_eq!(
        decrypt(&first.parts[0], &out, &passphrase()).unwrap_err(),
        VaultError::MissingPart { index: 2 }
    );

    // Restore, then swap parts 2 and 3.
    fs::copy(&backup, &first.parts[1]).unwrap();
    let swap = dir.path().join("swap.tmp");
    fs::rename(&first.parts[1], &swap).unwrap();
    fs::rename(&first.parts[2], &first.parts[1]).unwrap();
    fs::rename(&swap, &first.parts[2]).unwrap();
    assert_eq!(
        decrypt(&first.parts[0], &out, &passphrase()).unwrap_err(),
        VaultError::WrongPart { index: 2 }
    );
    assert!(!out.exists());
    assert!(no_temp_files(dir.path()));
}

#[test]
fn selecting_a_later_part_is_refused() {
    let dir = TestDir::new();
    let (_source, _base, summary) = encrypt(&dir, "doc.txt", &noise(50_000, 8), Some(4096));
    let out = dir.path().join("restored.txt");
    assert_eq!(
        decrypt(&summary.parts[1], &out, &passphrase()).unwrap_err(),
        VaultError::NotFirstPart
    );
}

#[test]
fn split_encryption_writes_only_new_style_part_names() {
    let dir = TestDir::new();
    let data = noise(50_000, 20);
    let (_source, _base, summary) = encrypt(&dir, "doc.txt", &data, Some(4096));
    assert!(summary.parts.len() > 1);
    assert!(dir.path().join("doc.txt.001.erx").is_file());
    assert!(!dir.path().join("doc.txt.erx.001").exists());
    assert!(
        names(dir.path())
            .iter()
            .all(|name| !name.starts_with("doc.txt.erx."))
    );
    let out = dir.path().join("restored.txt");
    decrypt(&summary.parts[0], &out, &passphrase()).unwrap();
    assert_eq!(fs::read(&out).unwrap(), data);
}

#[test]
fn an_uppercase_first_part_name_still_opens_the_whole_set() {
    let dir = TestDir::new();
    let data = noise(50_000, 26);
    let (_source, _base, summary) = encrypt(&dir, "X", &data, Some(4096));
    assert!(summary.parts.len() >= 3);
    assert_eq!(summary.parts[0], dir.path().join("X.001.erx"));

    // Windows file names are case-insensitive; the reader must agree.
    let uppercase = dir.path().join("X.001.ERX");
    fs::rename(&summary.parts[0], &uppercase).unwrap();

    let out = dir.path().join("restored.bin");
    decrypt(&uppercase, &out, &passphrase()).unwrap();
    assert_eq!(fs::read(&out).unwrap(), data);
}

#[test]
fn selecting_the_second_numbered_part_is_refused_by_the_header() {
    let dir = TestDir::new();
    let (_source, _base, summary) = encrypt(&dir, "doc.txt", &noise(50_000, 21), Some(4096));
    assert_eq!(summary.parts[1], dir.path().join("doc.txt.002.erx"));
    let out = dir.path().join("restored.txt");
    assert_eq!(
        decrypt(&summary.parts[1], &out, &passphrase()).unwrap_err(),
        VaultError::NotFirstPart
    );
    assert!(!out.exists());
}

#[test]
fn an_unsplit_file_named_like_part_one_ignores_a_stray_next_part() {
    let dir = TestDir::new();
    let data = noise(20_000, 22);
    let source = dir.path().join("x.001");
    fs::write(&source, &data).unwrap();
    // `default_output_path` keeps the whole source name.
    let base = default_output_path(&source, dir.path()).unwrap();
    assert_eq!(base, dir.path().join("x.001.erx"));
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
    assert_eq!(summary.parts, vec![base.clone()]);

    // A stray file with a matching name but not a vault part.
    fs::write(dir.path().join("x.002.erx"), b"not a vault part").unwrap();
    let out = dir.path().join("restored.001");
    decrypt(&base, &out, &passphrase()).unwrap();
    assert_eq!(fs::read(&out).unwrap(), data);
}

#[test]
fn a_new_style_set_with_a_missing_middle_part_reports_the_number() {
    let dir = TestDir::new();
    let (_source, _base, summary) = encrypt(&dir, "doc.txt", &noise(50_000, 23), Some(4096));
    assert!(summary.parts.len() > 3);
    fs::remove_file(&summary.parts[1]).unwrap();
    let out = dir.path().join("restored.txt");
    assert_eq!(
        decrypt(&summary.parts[0], &out, &passphrase()).unwrap_err(),
        VaultError::MissingPart { index: 2 }
    );
    assert!(!out.exists());
}

#[test]
fn two_unsplit_files_with_neighbouring_numbers_both_decrypt() {
    let dir = TestDir::new();
    let first_data = noise(10_000, 40);
    let second_data = noise(12_000, 41);

    // `hidden_output_path` removes only the last extension, so these two
    // independent files land as `scan.001.erx` and `scan.002.erx`.
    let first_source = dir.path().join("scan.001.txt");
    let second_source = dir.path().join("scan.002.txt");
    fs::write(&first_source, &first_data).unwrap();
    fs::write(&second_source, &second_data).unwrap();
    let first_base = hidden_output_path(&first_source, dir.path()).unwrap();
    let second_base = hidden_output_path(&second_source, dir.path()).unwrap();
    assert_eq!(first_base, dir.path().join("scan.001.erx"));
    assert_eq!(second_base, dir.path().join("scan.002.erx"));

    for (source, base) in [(&first_source, &first_base), (&second_source, &second_base)] {
        encrypt_file(
            source,
            base,
            None,
            &passphrase(),
            &EncryptSettings::for_tests(false),
            false,
            &mut |_done: u64, _total: u64| true,
        )
        .unwrap();
    }

    let first_out = dir.path().join("restored-001.txt");
    decrypt(&first_base, &first_out, &passphrase()).unwrap();
    assert_eq!(fs::read(&first_out).unwrap(), first_data);

    let second_out = dir.path().join("restored-002.txt");
    decrypt(&second_base, &second_out, &passphrase()).unwrap();
    assert_eq!(fs::read(&second_out).unwrap(), second_data);

    assert_eq!(count_parts(&first_base), 1);
    assert_eq!(count_parts(&second_base), 1);
}

#[test]
fn complete_split_set_ignores_a_neighbouring_vault_file() {
    let dir = TestDir::new();
    let data = noise(50_000, 42);
    let (_source, _base, summary) = encrypt(&dir, "x", &data, Some(40_000));
    assert_eq!(summary.parts.len(), 2);
    assert_eq!(summary.parts[0], dir.path().join("x.001.erx"));
    assert_eq!(summary.parts[1], dir.path().join("x.002.erx"));

    // An independent unsplit vault file with another UUID and a name that
    // looks like the next part of this set.
    let (_other_source, other_base, _other) = encrypt(&dir, "other", &noise(1_000, 43), None);
    fs::rename(&other_base, dir.path().join("x.003.erx")).unwrap();

    let out = dir.path().join("restored.bin");
    decrypt(&summary.parts[0], &out, &passphrase()).unwrap();
    assert_eq!(fs::read(&out).unwrap(), data);
    assert_eq!(count_parts(&summary.parts[0]), 2);
}

#[test]
fn complete_split_set_ignores_a_neighbouring_text_file() {
    let dir = TestDir::new();
    let data = noise(50_000, 44);
    let (_source, _base, summary) = encrypt(&dir, "x", &data, Some(40_000));
    assert_eq!(summary.parts.len(), 2);
    fs::write(dir.path().join("x.003.erx"), b"just a text file").unwrap();

    let out = dir.path().join("restored.bin");
    decrypt(&summary.parts[0], &out, &passphrase()).unwrap();
    assert_eq!(fs::read(&out).unwrap(), data);
    assert_eq!(count_parts(&summary.parts[0]), 2);
}

#[test]
fn a_missing_last_part_is_reported_as_an_error() {
    let dir = TestDir::new();
    let (_source, _base, summary) = encrypt(&dir, "x", &noise(2_500_000, 45), Some(1_000_000));
    assert_eq!(summary.parts.len(), 3);
    fs::remove_file(&summary.parts[2]).unwrap();

    let out = dir.path().join("restored.bin");
    let error = decrypt(&summary.parts[0], &out, &passphrase()).unwrap_err();
    eprintln!("missing last part error: {error:?}");
    assert!(!out.exists());
    assert!(no_temp_files(dir.path()));
}

#[test]
fn find_conflicts_lists_new_style_parts_only() {
    let dir = TestDir::new();
    let base = dir.path().join("photo.erx");
    let first = dir.path().join("photo.001.erx");
    let second = dir.path().join("photo.002.erx");
    let old_style = dir.path().join("photo.erx.001");
    fs::write(&first, b"x").unwrap();
    fs::write(&second, b"x").unwrap();
    fs::write(&old_style, b"x").unwrap();

    let conflicts = find_conflicts(&base, true);
    assert!(conflicts.contains(&first));
    assert!(conflicts.contains(&second));
    assert!(!conflicts.contains(&old_style));
    assert!(!conflicts.contains(&base));

    // An unsplit output only cares about the base file itself.
    assert!(find_conflicts(&base, false).is_empty());
    fs::write(&base, b"x").unwrap();
    assert_eq!(find_conflicts(&base, false), vec![base.clone()]);
    assert!(!find_conflicts(&base, true).contains(&base));
}

#[test]
fn unsplit_encryption_is_not_blocked_by_neighbouring_numbered_files() {
    let dir = TestDir::new();
    let neighbours: Vec<PathBuf> = (1..=5)
        .map(|index| dir.path().join(format!("scan.{index:03}.erx")))
        .collect();
    let contents: Vec<Vec<u8>> = (0..5).map(|index| vec![index as u8; 100]).collect();
    for (path, bytes) in neighbours.iter().zip(&contents) {
        fs::write(path, bytes).unwrap();
    }

    let source = dir.path().join("scan.pdf");
    fs::write(&source, noise(5_000, 50)).unwrap();
    let base = hidden_output_path(&source, dir.path()).unwrap();
    assert_eq!(base, dir.path().join("scan.erx"));

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
    assert!(base.is_file());
    for (path, bytes) in neighbours.iter().zip(&contents) {
        assert_eq!(fs::read(path).unwrap(), *bytes);
    }
}

#[test]
fn split_encryption_reports_existing_part_names_as_conflicts() {
    let dir = TestDir::new();
    let source = dir.path().join("x.bin");
    fs::write(&source, noise(5_000, 51)).unwrap();
    let base = hidden_output_path(&source, dir.path()).unwrap();
    assert_eq!(base, dir.path().join("x.erx"));
    let existing = dir.path().join("x.001.erx");
    fs::write(&existing, b"old part").unwrap();

    let error = encrypt_file(
        &source,
        &base,
        Some(4096),
        &passphrase(),
        &EncryptSettings::for_tests(false),
        false,
        &mut |_done: u64, _total: u64| true,
    )
    .unwrap_err();
    assert_eq!(error, VaultError::OutputExists);
    assert_eq!(fs::read(&existing).unwrap(), b"old part");

    let summary = encrypt_file(
        &source,
        &base,
        Some(4096),
        &passphrase(),
        &EncryptSettings::for_tests(false),
        true,
        &mut |_done: u64, _total: u64| true,
    )
    .unwrap();
    assert!(summary.parts.len() >= 2);
    assert_eq!(summary.parts[0], existing);
    assert_ne!(fs::read(&existing).unwrap(), b"old part");
}

#[test]
fn split_encryption_is_not_blocked_by_an_unsplit_base_file() {
    let dir = TestDir::new();
    let source = dir.path().join("x.bin");
    fs::write(&source, noise(5_000, 52)).unwrap();
    let base = hidden_output_path(&source, dir.path()).unwrap();
    fs::write(&base, b"old base").unwrap();

    let summary = encrypt_file(
        &source,
        &base,
        Some(4096),
        &passphrase(),
        &EncryptSettings::for_tests(false),
        false,
        &mut |_done: u64, _total: u64| true,
    )
    .unwrap();
    assert!(summary.parts.len() >= 2);
    assert_eq!(fs::read(&base).unwrap(), b"old base");
}

#[test]
fn overwrite_only_removes_the_listed_conflicts() {
    let dir = TestDir::new();

    // Split mode: the unsplit base file is not a conflict.
    let source = dir.path().join("x.bin");
    fs::write(&source, noise(5_000, 53)).unwrap();
    let base = hidden_output_path(&source, dir.path()).unwrap();
    fs::write(&base, b"keep me").unwrap();
    encrypt_file(
        &source,
        &base,
        Some(4096),
        &passphrase(),
        &EncryptSettings::for_tests(false),
        true,
        &mut |_done: u64, _total: u64| true,
    )
    .unwrap();
    assert_eq!(fs::read(&base).unwrap(), b"keep me");

    // Unsplit mode: numbered neighbours are not conflicts.
    let other_source = dir.path().join("y.bin");
    fs::write(&other_source, noise(5_000, 54)).unwrap();
    let other_base = hidden_output_path(&other_source, dir.path()).unwrap();
    let neighbours: Vec<PathBuf> = (1..=3)
        .map(|index| dir.path().join(format!("y.{index:03}.erx")))
        .collect();
    for path in &neighbours {
        fs::write(path, b"keep numbered").unwrap();
    }
    encrypt_file(
        &other_source,
        &other_base,
        None,
        &passphrase(),
        &EncryptSettings::for_tests(false),
        true,
        &mut |_done: u64, _total: u64| true,
    )
    .unwrap();
    assert!(other_base.is_file());
    for path in &neighbours {
        assert_eq!(fs::read(path).unwrap(), b"keep numbered");
    }
}

#[test]
fn hand_renamed_parts_still_form_one_set() {
    let dir = TestDir::new();
    let data = noise(50_000, 24);
    let (_source, base, summary) = encrypt(&dir, "doc.txt", &data, Some(4096));
    assert!(summary.parts.len() > 1);
    let old_names = old_style_parts(&base, summary.parts.len());
    for (new, old) in summary.parts.iter().zip(&old_names) {
        fs::rename(new, old).unwrap();
    }
    for (new, old) in summary.parts.iter().zip(&old_names) {
        fs::rename(old, new).unwrap();
    }
    let out = dir.path().join("restored.txt");
    decrypt(&summary.parts[0], &out, &passphrase()).unwrap();
    assert_eq!(fs::read(&out).unwrap(), data);
}

#[test]
fn selecting_an_old_style_part_is_rejected_without_output() {
    let dir = TestDir::new();
    let (_source, base, summary) = encrypt(&dir, "doc.txt", &noise(50_000, 25), Some(4096));
    assert!(summary.parts.len() > 1);
    let old_names = old_style_parts(&base, summary.parts.len());
    for (new, old) in summary.parts.iter().zip(&old_names) {
        fs::rename(new, old).unwrap();
    }

    let out = dir.path().join("restored.txt");
    assert_eq!(
        decrypt(&old_names[0], &out, &passphrase()).unwrap_err(),
        VaultError::OldStylePartName
    );
    assert!(!out.exists());
}

#[test]
fn files_that_are_not_vault_files_or_do_not_exist() {
    let dir = TestDir::new();
    let out = dir.path().join("restored.bin");
    let junk = dir.path().join("junk.erx");
    fs::write(&junk, b"hello, this is not a vault file at all").unwrap();
    assert_eq!(
        decrypt(&junk, &out, &passphrase()).unwrap_err(),
        VaultError::NotAVaultFile
    );
    let empty = dir.path().join("empty.erx");
    fs::write(&empty, b"").unwrap();
    assert_eq!(
        decrypt(&empty, &out, &passphrase()).unwrap_err(),
        VaultError::NotAVaultFile
    );
    let missing = dir.path().join("missing.erx");
    assert_eq!(
        decrypt(&missing, &out, &passphrase()).unwrap_err(),
        VaultError::CannotOpenFile
    );

    let base = dir.path().join("nothing.erx");
    let result = encrypt_file(
        &dir.path().join("does-not-exist.txt"),
        &base,
        None,
        &passphrase(),
        &EncryptSettings::for_tests(false),
        false,
        &mut |_done: u64, _total: u64| true,
    );
    assert_eq!(result.unwrap_err(), VaultError::CannotOpenFile);
    assert!(!out.exists());
    assert!(no_temp_files(dir.path()));
}

#[test]
fn existing_output_is_only_replaced_with_permission() {
    let dir = TestDir::new();
    let data = noise(50_000, 9);
    let (source, base, _summary) = encrypt(&dir, "doc.txt", &data, Some(4096));
    assert!(find_conflicts(&base, true).len() >= 13);

    // Without permission nothing changes.
    let before = names(dir.path());
    let error = encrypt_file(
        &source,
        &base,
        Some(4096),
        &passphrase(),
        &EncryptSettings::for_tests(false),
        false,
        &mut |_done: u64, _total: u64| true,
    )
    .unwrap_err();
    assert_eq!(error, VaultError::OutputExists);
    assert_eq!(names(dir.path()), before);

    // With permission the whole old split set is replaced, so no stale
    // part can be mixed into the new file.
    let summary = encrypt_file(
        &source,
        &base,
        Some(4096),
        &passphrase(),
        &EncryptSettings::for_tests(false),
        true,
        &mut |_done: u64, _total: u64| true,
    )
    .unwrap();
    let mut expected = vec!["doc.txt".to_owned()];
    expected.extend((1..=summary.parts.len()).map(|index| format!("doc.txt.{index:03}.erx")));
    assert_eq!(names(dir.path()), expected);
    let out = dir.path().join("restored.txt");
    decrypt(&summary.parts[0], &out, &passphrase()).unwrap();
    assert_eq!(fs::read(&out).unwrap(), data);

    // The same rule for decrypting.
    fs::write(&out, b"old content").unwrap();
    assert_eq!(
        decrypt(&summary.parts[0], &out, &passphrase()).unwrap_err(),
        VaultError::OutputExists
    );
    assert_eq!(fs::read(&out).unwrap(), b"old content");
    decrypt_file(
        &summary.parts[0],
        &out,
        &passphrase(),
        true,
        &mut |_d: u64, _t: u64| true,
    )
    .unwrap();
    assert_eq!(fs::read(&out).unwrap(), data);
}

#[test]
fn the_source_file_is_never_overwritten() {
    let dir = TestDir::new();
    let source = dir.path().join("secret.erx");
    fs::write(&source, b"original").unwrap();
    let result = encrypt_file(
        &source,
        &source,
        None,
        &passphrase(),
        &EncryptSettings::for_tests(false),
        true,
        &mut |_done: u64, _total: u64| true,
    );
    assert_eq!(result.unwrap_err(), VaultError::OutputIsSource);
    assert_eq!(fs::read(&source).unwrap(), b"original");
    assert!(no_temp_files(dir.path()));
}

#[test]
fn cancelling_leaves_no_files_behind() {
    let dir = TestDir::new();
    let data = noise(2_300_000, 10);
    let source = dir.path().join("big.bin");
    fs::write(&source, &data).unwrap();
    let base = default_output_path(&source, dir.path()).unwrap();

    let result = encrypt_file(
        &source,
        &base,
        Some(1_000_000),
        &passphrase(),
        &EncryptSettings::for_tests(false),
        false,
        &mut |_done: u64, _total: u64| false,
    );
    assert_eq!(result.unwrap_err(), VaultError::Cancelled);
    assert_eq!(names(dir.path()), ["big.bin"]);

    // Cancel while decrypting (needs more than one chunk).
    let (_s, _b, summary) = encrypt(&dir, "other.bin", &data, None);
    let out = dir.path().join("restored.bin");
    let result = decrypt_file(
        &summary.parts[0],
        &out,
        &passphrase(),
        false,
        &mut |_done: u64, _total: u64| false,
    );
    assert_eq!(result.unwrap_err(), VaultError::Cancelled);
    assert!(!out.exists());
    assert!(no_temp_files(dir.path()));
}

#[test]
fn wrong_passphrase_and_tampering_leave_no_output() {
    let dir = TestDir::new();
    let (_source, _base, summary) = encrypt(&dir, "doc.txt", &noise(50_000, 11), Some(4096));
    let out = dir.path().join("restored.txt");

    assert_eq!(
        decrypt(&summary.parts[0], &out, &phrase_from(100)).unwrap_err(),
        VaultError::WrongPassphraseOrHeader
    );
    assert!(!out.exists());
    assert!(no_temp_files(dir.path()));

    let mut bytes = fs::read(&summary.parts[1]).unwrap();
    bytes[100] ^= 0x01;
    fs::write(&summary.parts[1], bytes).unwrap();
    assert_eq!(
        decrypt(&summary.parts[0], &out, &passphrase()).unwrap_err(),
        VaultError::CorruptedData
    );
    assert!(!out.exists());
    assert!(no_temp_files(dir.path()));
}

#[test]
fn progress_reports_source_bytes_and_a_constant_total() {
    let dir = TestDir::new();
    let data = noise(700_000, 12);
    let source = dir.path().join("p.bin");
    fs::write(&source, &data).unwrap();
    let base = default_output_path(&source, dir.path()).unwrap();
    let mut calls: Vec<(u64, u64)> = Vec::new();
    encrypt_file(
        &source,
        &base,
        None,
        &passphrase(),
        &EncryptSettings::for_tests(false),
        false,
        &mut |done: u64, total: u64| {
            calls.push((done, total));
            true
        },
    )
    .unwrap();
    assert_eq!(calls.last().copied(), Some((700_000, 700_000)));
    assert!(calls.iter().all(|(_, total)| *total == 700_000));
    assert!(calls.windows(2).all(|pair| pair[0].0 <= pair[1].0));

    // Decrypting a multi-chunk file reports progress that never exceeds
    // the total.
    let big = noise(2_300_000, 13);
    let (_s, _b, summary) = encrypt(&dir, "q.bin", &big, None);
    let out = dir.path().join("restored.bin");
    let mut seen: Vec<(u64, u64)> = Vec::new();
    decrypt_file(
        &summary.parts[0],
        &out,
        &passphrase(),
        false,
        &mut |done: u64, total: u64| {
            seen.push((done, total));
            true
        },
    )
    .unwrap();
    assert!(!seen.is_empty());
    assert!(seen.iter().all(|(done, total)| done <= total));
}

#[test]
fn default_names() {
    let out_dir = Path::new("out");
    assert_eq!(
        default_output_path(Path::new("photo.jpg"), out_dir),
        Some(out_dir.join("photo.jpg.erx"))
    );
    assert_eq!(
        default_restore_path(Path::new("photo.jpg.erx"), out_dir),
        Some(out_dir.join("photo.jpg"))
    );
    assert_eq!(
        default_restore_path(Path::new("photo.001.erx"), out_dir),
        Some(out_dir.join("photo"))
    );
    assert_eq!(
        default_restore_path(Path::new("photo.jpg.001.erx"), out_dir),
        Some(out_dir.join("photo.jpg"))
    );
    assert_eq!(
        default_restore_path(Path::new("UPPER.ERX"), out_dir),
        Some(out_dir.join("UPPER"))
    );
    assert_eq!(
        default_restore_path(Path::new("weird"), out_dir),
        Some(out_dir.join("weird.restored"))
    );
}

#[test]
fn space_estimate_is_generous_and_grows_with_the_input() {
    let four_mib = 4 * 1024 * 1024;
    assert!(required_for_encrypt(0, None) >= four_mib);
    let gib = 1024u64 * 1024 * 1024;
    let twenty = 20 * gib;
    assert!(required_for_encrypt(twenty, Some(500 * 1024 * 1024)) > twenty);
    assert!(required_for_encrypt(twenty, None) > required_for_encrypt(gib, None));
    assert!(required_for_encrypt(gib, Some(500 * 1024 * 1024)) > required_for_encrypt(gib, None));
}

#[cfg(windows)]
#[test]
fn windows_reports_free_space_for_the_temp_folder() {
    assert!(free_bytes(&std::env::temp_dir()).is_some());
}

#[cfg(not(windows))]
#[test]
fn free_space_is_unknown_off_windows() {
    assert!(free_bytes(&std::env::temp_dir()).is_none());
}
