//! Heavy and special-case tests.
//!
//! The two tests marked `#[ignore]` write several GiB into the temp folder
//! and take minutes, so they only run when asked for by hand:
//!
//!   cargo test vault::large_test -- --ignored --nocapture
//!
//! They prove that big files go through as a stream: the memory used stays
//! far below the file size, and every byte comes back identical.

use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use super::engine::EncryptSettings;
use super::files::{
    decrypt_file_restoring_name, default_restore_path, encrypt_file, hidden_output_path,
};
use super::folder::{decrypt_path, encrypt_path, output_path_for};
use super::parts_test::TestDir;
use super::runtime::{STALE_TEMP_AGE, cleanup_stale_temp};
use super::wordlist;

const MIB: u64 = 1024 * 1024;
/// A block that is deliberately not a multiple of the 1 MiB chunk size, so
/// chunk and part boundaries fall in different places.
const BLOCK: usize = 1024 * 1024 + 13;
/// Upper limit for the peak memory (working set) of the whole test process.
const MEMORY_LIMIT: u64 = 200 * MIB;

fn passphrase() -> String {
    wordlist::words()
        .iter()
        .take(8)
        .copied()
        .collect::<Vec<&str>>()
        .join(" ")
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

/// Writes `blocks` blocks of noise; every block differs. Returns the size.
fn write_noise_file(path: &Path, blocks: u64) -> u64 {
    let mut writer = BufWriter::with_capacity(1 << 20, File::create(path).unwrap());
    let mut total = 0u64;
    for index in 0..blocks {
        let block = noise(BLOCK, (index * 2 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        writer.write_all(&block).unwrap();
        total += BLOCK as u64;
    }
    writer.flush().unwrap();
    total
}

/// Writes about `target` bytes of repeating text. Returns the real size.
fn write_text_file(path: &Path, target: u64) -> u64 {
    let line = b"The quick brown fox jumps over the lazy dog. 0123456789\n";
    let mut block: Vec<u8> = Vec::with_capacity(64 * 1024);
    while block.len() + line.len() <= 64 * 1024 {
        block.extend_from_slice(line);
    }
    let blocks = target / block.len() as u64;
    let mut writer = BufWriter::with_capacity(1 << 20, File::create(path).unwrap());
    for _ in 0..blocks {
        writer.write_all(&block).unwrap();
    }
    writer.flush().unwrap();
    blocks * block.len() as u64
}

fn sha256_of(path: &Path) -> Vec<u8> {
    let mut file = File::open(path).unwrap();
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let count = file.read(&mut buffer).unwrap();
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    let digest = hasher.finalize();
    let bytes: &[u8] = &digest;
    bytes.to_vec()
}

/// Peak working set of this process in bytes (Windows only).
#[cfg(windows)]
fn peak_working_set_bytes() -> Option<u64> {
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    // SAFETY: the structure is plain old data, so all-zero is a valid value.
    let mut counters: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    counters.cb = size;
    // SAFETY: `counters` is a valid out-pointer of `size` bytes and the
    // pseudo handle of the current process is always valid.
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, size) };
    if ok == 0 {
        None
    } else {
        Some(counters.PeakWorkingSetSize as u64)
    }
}

#[cfg(not(windows))]
fn peak_working_set_bytes() -> Option<u64> {
    None
}

fn check_memory(label: &str) {
    match peak_working_set_bytes() {
        Some(peak) => {
            println!("{label}: peak memory {} MiB", peak / MIB);
            assert!(
                peak < MEMORY_LIMIT,
                "peak memory of {} MiB is too high",
                peak / MIB
            );
        }
        None => println!("{label}: peak memory unknown"),
    }
}

/// Prints a line every 10 percent and never cancels.
fn progress_printer(label: &'static str) -> impl FnMut(u64, u64) -> bool {
    let mut next = 10u64;
    move |done: u64, total: u64| {
        let percent = (done * 100).checked_div(total).unwrap_or(100);
        if percent >= next {
            println!("{label}: {percent}%");
            next = percent / 10 * 10 + 10;
        }
        true
    }
}

#[test]
#[ignore = "heavy: writes about 4.5 GiB into the temp folder"]
fn large_incompressible_file_round_trips_in_parts_with_bounded_memory() {
    let dir = TestDir::new();
    let source = dir.path().join("big.bin");
    let size = write_noise_file(&source, 1400);
    let source_hash = sha256_of(&source);
    println!("source: {} MiB", size / MIB);

    let base = hidden_output_path(&source, dir.path()).unwrap();
    let started = Instant::now();
    let summary = encrypt_file(
        &source,
        &base,
        Some(400 * MIB),
        &passphrase(),
        &EncryptSettings::for_tests(true),
        false,
        &mut progress_printer("encrypt"),
    )
    .unwrap();
    println!(
        "encrypted into {} parts in {:?}",
        summary.parts.len(),
        started.elapsed()
    );
    println!(
        "DIAG size {size}, summary.input_bytes {}, source on disk {}, parts {}, output_bytes {}",
        summary.input_bytes,
        fs::metadata(&source).unwrap().len(),
        summary.parts.len(),
        summary.output_bytes
    );
    assert_eq!(summary.input_bytes, size);
    assert_eq!(summary.parts.len(), 4);
    check_memory("after encrypting");

    let out = TestDir::new();
    let fallback = default_restore_path(&summary.parts[0], out.path()).unwrap();
    let started = Instant::now();
    let restored = decrypt_file_restoring_name(
        &summary.parts[0],
        &fallback,
        &passphrase(),
        false,
        &mut progress_printer("decrypt"),
    )
    .unwrap();
    println!("decrypted in {:?}", started.elapsed());
    assert_eq!(restored.output_bytes, size);
    assert_eq!(restored.output_path, out.path().join("big.bin"));
    assert_eq!(sha256_of(&restored.output_path), source_hash);
    check_memory("after decrypting");
}

#[test]
#[ignore = "heavy: writes about 1 GiB of text into the temp folder"]
fn large_compressible_file_shrinks_and_round_trips() {
    let dir = TestDir::new();
    let source = dir.path().join("big.txt");
    let size = write_text_file(&source, 1024 * MIB);
    let source_hash = sha256_of(&source);
    println!("source: {} MiB", size / MIB);

    let base = hidden_output_path(&source, dir.path()).unwrap();
    let summary = encrypt_file(
        &source,
        &base,
        None,
        &passphrase(),
        &EncryptSettings::for_tests(true),
        false,
        &mut progress_printer("encrypt"),
    )
    .unwrap();
    println!("output: {} bytes", summary.output_bytes);
    assert_eq!(summary.input_bytes, size);
    assert!(summary.output_bytes < 64 * MIB);
    check_memory("after encrypting");

    let out = TestDir::new();
    let fallback = default_restore_path(&summary.parts[0], out.path()).unwrap();
    let restored = decrypt_file_restoring_name(
        &summary.parts[0],
        &fallback,
        &passphrase(),
        false,
        &mut progress_printer("decrypt"),
    )
    .unwrap();
    assert_eq!(restored.output_bytes, size);
    assert_eq!(restored.output_path, out.path().join("big.txt"));
    assert_eq!(sha256_of(&restored.output_path), source_hash);
    check_memory("after decrypting");
}

fn temp_files(dir: &Path) -> Vec<String> {
    let mut found: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".erxtmp"))
        .collect();
    found.sort();
    found
}

#[test]
fn medium_incompressible_multi_part_file_reports_exact_sizes() {
    let dir = TestDir::new();
    let source = dir.path().join("mid.bin");
    let size = write_noise_file(&source, 5);
    let on_disk = fs::metadata(&source).unwrap().len();
    assert_eq!(on_disk, size, "the generated file has the wrong size");
    let source_hash = sha256_of(&source);

    let base = hidden_output_path(&source, dir.path()).unwrap();
    let summary = encrypt_file(
        &source,
        &base,
        Some(2 * MIB),
        &passphrase(),
        &EncryptSettings::for_tests(true),
        false,
        &mut |_done: u64, _total: u64| true,
    )
    .unwrap();
    println!(
        "DIAG size {size}, input_bytes {}, parts {}, output_bytes {}",
        summary.input_bytes,
        summary.parts.len(),
        summary.output_bytes
    );
    assert_eq!(summary.input_bytes, size, "input_bytes is wrong");
    assert!(
        (3..=4).contains(&summary.parts.len()),
        "unexpected number of parts: {}",
        summary.parts.len()
    );

    let out = TestDir::new();
    let fallback = default_restore_path(&summary.parts[0], out.path()).unwrap();
    let restored = decrypt_file_restoring_name(
        &summary.parts[0],
        &fallback,
        &passphrase(),
        false,
        &mut |_done: u64, _total: u64| true,
    )
    .unwrap();
    assert_eq!(restored.output_bytes, size, "output_bytes is wrong");
    assert_eq!(sha256_of(&restored.output_path), source_hash);
}

/// Helper for the kill test below. It runs in a CHILD process (started by
/// the kill test), encrypts the file named by ERX_VAULT_CHILD_SOURCE into the
/// same folder and is killed by the parent while it works. Without the
/// environment variable it does nothing.
#[test]
#[ignore = "helper process for the kill test; does nothing without the env variable"]
fn child_encrypt_helper() {
    let Ok(source) = std::env::var("ERX_VAULT_CHILD_SOURCE") else {
        return;
    };
    let source = PathBuf::from(source);
    let dir = source.parent().unwrap().to_path_buf();
    let base = hidden_output_path(&source, &dir).unwrap();
    let _ = encrypt_file(
        &source,
        &base,
        Some(8 * MIB),
        &passphrase(),
        &EncryptSettings::for_tests(false),
        false,
        &mut |_done: u64, _total: u64| true,
    );
}

#[test]
#[ignore = "starts a child process and kills it; takes up to a minute"]
fn killing_the_app_during_encryption_leaves_only_temp_files_that_cleanup_removes() {
    let dir = TestDir::new();
    let source = dir.path().join("kill.bin");
    write_noise_file(&source, 120);

    let exe = std::env::current_exe().unwrap();
    let mut child = std::process::Command::new(exe)
        .args([
            "tools::vault::large_test::child_encrypt_helper",
            "--exact",
            "--ignored",
            "--nocapture",
        ])
        .env("ERX_VAULT_CHILD_SOURCE", &source)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let started = Instant::now();
    let mut seen = false;
    while started.elapsed() < Duration::from_secs(120) {
        if !temp_files(dir.path()).is_empty() {
            seen = true;
            break;
        }
        if child.try_wait().unwrap().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let finished_early = child.try_wait().unwrap().is_some();
    let _ = child.kill();
    let _ = child.wait();
    assert!(seen, "no temporary file appeared");
    assert!(
        !finished_early,
        "the child finished before it could be killed"
    );

    // After the kill: the source and temporary files only; nothing that looks
    // like a finished encrypted file.
    let leftovers = temp_files(dir.path());
    assert!(!leftovers.is_empty());
    let all: Vec<String> = fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        all.iter()
            .all(|name| name == "kill.bin" || name.ends_with(".erxtmp")),
        "unexpected files after the kill: {all:?}"
    );

    // Young temporary files are NOT touched by the real age limit ...
    assert_eq!(cleanup_stale_temp(dir.path(), STALE_TEMP_AGE), 0);
    assert_eq!(temp_files(dir.path()), leftovers);
    // ... and old ones are removed (age zero simulates "an hour later").
    assert_eq!(
        cleanup_stale_temp(dir.path(), Duration::ZERO),
        leftovers.len()
    );
    assert!(temp_files(dir.path()).is_empty());
}

#[test]
fn long_paths_over_260_characters_work_for_files_and_folders() {
    let dir = TestDir::new();
    let deep = dir.path().join("a".repeat(120)).join("b".repeat(120));
    let source = deep.join("Photos");
    let inner = source.join("c".repeat(120));
    fs::create_dir_all(&inner).unwrap();
    fs::write(inner.join("file.txt"), b"deep file").unwrap();
    fs::write(source.join("top.txt"), b"top file").unwrap();
    let single = deep.join("single.txt");
    fs::write(&single, b"single file").unwrap();
    assert!(inner.join("file.txt").as_os_str().len() > 300);

    let out = dir.path().join("x".repeat(120)).join("y".repeat(120));
    fs::create_dir_all(&out).unwrap();
    let settings = EncryptSettings::for_tests(false);

    // A folder with a long path inside, encrypted into a long output path.
    let base = output_path_for(&source, &out).unwrap();
    let summary = encrypt_path(
        &source,
        &base,
        None,
        &passphrase(),
        &settings,
        false,
        &mut |_done: u64, _total: u64| true,
    )
    .unwrap();
    let fallback = default_restore_path(&summary.parts[0], &out).unwrap();
    let restored = decrypt_path(
        &summary.parts[0],
        &fallback,
        &passphrase(),
        false,
        &mut |_done: u64, _total: u64| true,
    )
    .unwrap();
    assert_eq!(
        fs::read(restored.output_path.join("c".repeat(120)).join("file.txt")).unwrap(),
        b"deep file"
    );
    assert_eq!(
        fs::read(restored.output_path.join("top.txt")).unwrap(),
        b"top file"
    );

    // A single file in a long path.
    let base = output_path_for(&single, &out).unwrap();
    let summary = encrypt_path(
        &single,
        &base,
        None,
        &passphrase(),
        &settings,
        false,
        &mut |_done: u64, _total: u64| true,
    )
    .unwrap();
    let fallback = default_restore_path(&summary.parts[0], &out).unwrap();
    let restored = decrypt_path(
        &summary.parts[0],
        &fallback,
        &passphrase(),
        false,
        &mut |_done: u64, _total: u64| true,
    )
    .unwrap();
    assert_eq!(fs::read(&restored.output_path).unwrap(), b"single file");
}
