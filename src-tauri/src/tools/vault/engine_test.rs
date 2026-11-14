use std::io::Cursor;
use std::path::Path;

use super::engine::{EncryptSettings, decrypt_stream, encrypt_stream, should_compress};
use super::error::VaultError;
use super::format::{HEADER_LEN, Header};
use super::kdf::KdfParams;
use super::passphrase::PassphraseError;
use super::wordlist;

const CHUNK: usize = 1_048_576;
const TRAILER: usize = 1064;

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

fn encrypt_with(data: &[u8], compress: bool) -> Vec<u8> {
    let mut source = Cursor::new(data);
    let mut output = Vec::new();
    encrypt_stream(
        &mut source,
        &mut output,
        &passphrase(),
        &EncryptSettings::for_tests(compress),
        &mut |_bytes: u64| true,
    )
    .unwrap();
    output
}

fn decrypt_with(file: &[u8], phrase: &str) -> Result<Vec<u8>, VaultError> {
    let mut input = Cursor::new(file);
    let mut output = Vec::new();
    decrypt_stream(&mut input, &mut output, phrase, &mut |_bytes: u64| true)?;
    Ok(output)
}

#[test]
fn round_trip_empty_and_tiny_files() {
    for compress in [false, true] {
        for data in [&b""[..], &b"a"[..], &b"hello vault"[..]] {
            let file = encrypt_with(data, compress);
            assert_eq!(decrypt_with(&file, &passphrase()).unwrap(), data);
        }
    }
}

#[test]
fn round_trip_around_every_chunk_boundary() {
    let sizes = [
        39,
        40,
        41,
        CHUNK - TRAILER - 1,
        CHUNK - TRAILER,
        CHUNK - TRAILER + 1,
        CHUNK,
        CHUNK + 1,
        2 * CHUNK - TRAILER - 1,
        2 * CHUNK - TRAILER,
        2 * CHUNK - TRAILER + 1,
        3 * CHUNK + 5,
    ];
    for compress in [false, true] {
        for (index, size) in sizes.iter().enumerate() {
            let data = noise(*size, index as u64 + 11);
            let file = encrypt_with(&data, compress);
            assert_eq!(
                decrypt_with(&file, &passphrase()).unwrap(),
                data,
                "size {size}, compress {compress}"
            );
        }
    }
}

#[test]
fn framing_matches_the_documented_layout() {
    for size in [0, 5, CHUNK - TRAILER, CHUNK - TRAILER + 1, 2 * CHUNK] {
        let file = encrypt_with(&noise(size, 3), false);
        let payload = size + TRAILER;
        let expected = HEADER_LEN + payload + 16 * payload.div_ceil(CHUNK);
        assert_eq!(file.len(), expected, "size {size}");
    }
}

#[test]
fn compression_shrinks_compressible_data_and_sets_the_flag() {
    let data = vec![7u8; 3 * CHUNK];
    let packed = encrypt_with(&data, true);
    let plain = encrypt_with(&data, false);
    assert!(packed.len() < data.len() / 10);
    assert!(plain.len() > data.len());
    assert!(
        Header::parse(&packed[..HEADER_LEN])
            .unwrap()
            .is_compressed()
    );
    assert!(!Header::parse(&plain[..HEADER_LEN]).unwrap().is_compressed());
    assert_eq!(decrypt_with(&packed, &passphrase()).unwrap(), data);
}

#[test]
fn typing_style_does_not_matter() {
    let file = encrypt_with(b"case and spacing", true);
    let messy = format!("  {}  ", passphrase().to_uppercase().replace(' ', "   "));
    assert_eq!(decrypt_with(&file, &messy).unwrap(), b"case and spacing");
}

#[test]
fn wrong_passphrase_is_rejected_before_decrypting() {
    let file = encrypt_with(b"secret data", true);
    assert_eq!(
        decrypt_with(&file, &phrase_from(100)),
        Err(VaultError::WrongPassphraseOrHeader)
    );
}

#[test]
fn words_outside_the_list_are_rejected_without_naming_them() {
    let file = encrypt_with(b"x", false);
    let mut words: Vec<String> = passphrase().split(' ').map(str::to_owned).collect();
    words[1] = "zzzznotaword".to_owned();
    let typed = words.join(" ");
    assert_eq!(
        decrypt_with(&file, &typed),
        Err(VaultError::Passphrase(PassphraseError::UnknownWord {
            position: 2
        }))
    );
    let mut source = Cursor::new(b"x".as_slice());
    let mut output = Vec::new();
    let result = encrypt_stream(
        &mut source,
        &mut output,
        &typed,
        &EncryptSettings::for_tests(false),
        &mut |_bytes: u64| true,
    );
    assert_eq!(
        result.map(|_| ()),
        Err(VaultError::Passphrase(PassphraseError::UnknownWord {
            position: 2
        }))
    );
    assert!(output.is_empty());
}

#[test]
fn every_header_byte_is_protected() {
    let file = encrypt_with(b"hello vault", true);
    for index in 0..HEADER_LEN {
        let mut bad = file.clone();
        bad[index] ^= 0x01;
        assert!(
            decrypt_with(&bad, &passphrase()).is_err(),
            "header byte {index} was not protected"
        );
    }
}

#[test]
fn every_body_byte_is_protected() {
    let file = encrypt_with(b"twenty bytes of data", false);
    for index in HEADER_LEN..file.len() {
        let mut bad = file.clone();
        bad[index] ^= 0x01;
        assert_eq!(
            decrypt_with(&bad, &passphrase()),
            Err(VaultError::CorruptedData),
            "body byte {index} was not protected"
        );
    }
}

#[test]
fn tampering_deep_inside_a_big_file_is_detected() {
    let file = encrypt_with(&noise(2 * CHUNK + 100, 5), false);
    for index in [HEADER_LEN, HEADER_LEN + CHUNK, file.len() - 1] {
        let mut bad = file.clone();
        bad[index] ^= 0x80;
        assert_eq!(
            decrypt_with(&bad, &passphrase()),
            Err(VaultError::CorruptedData)
        );
    }
}

#[test]
fn truncation_is_detected_everywhere() {
    let small = encrypt_with(b"truncate me please", true);
    for cut in [
        small.len() - 1,
        small.len() - 16,
        HEADER_LEN + 1,
        HEADER_LEN,
        10,
        0,
    ] {
        assert!(
            decrypt_with(&small[..cut], &passphrase()).is_err(),
            "cut at {cut}"
        );
    }
    // Cutting exactly at chunk boundaries must fail too (STREAM last flag).
    let big = encrypt_with(&noise(2 * CHUNK, 9), false);
    for chunks_kept in [1, 2] {
        let cut = HEADER_LEN + chunks_kept * (CHUNK + 16);
        assert_eq!(
            decrypt_with(&big[..cut], &passphrase()),
            Err(VaultError::CorruptedData),
            "kept {chunks_kept} chunks"
        );
    }
}

#[test]
fn appended_bytes_are_detected() {
    let mut file = encrypt_with(b"no extras allowed", true);
    file.push(0);
    assert!(decrypt_with(&file, &passphrase()).is_err());
}

#[test]
fn same_file_twice_gives_different_output() {
    let first = encrypt_with(b"identical input", true);
    let second = encrypt_with(b"identical input", true);
    assert_ne!(first, second);
    assert_eq!(
        decrypt_with(&first, &passphrase()).unwrap(),
        b"identical input"
    );
    assert_eq!(
        decrypt_with(&second, &passphrase()).unwrap(),
        b"identical input"
    );
}

#[test]
fn cancel_stops_encryption_and_decryption() {
    let data = noise(2 * CHUNK + 500, 7);
    let mut source = Cursor::new(data.as_slice());
    let mut output = Vec::new();
    let result = encrypt_stream(
        &mut source,
        &mut output,
        &passphrase(),
        &EncryptSettings::for_tests(false),
        &mut |_bytes: u64| false,
    );
    assert_eq!(result.map(|_| ()), Err(VaultError::Cancelled));

    let file = encrypt_with(&data, false);
    let mut input = Cursor::new(file.as_slice());
    let mut restored = Vec::new();
    let result = decrypt_stream(
        &mut input,
        &mut restored,
        &passphrase(),
        &mut |_bytes: u64| false,
    );
    assert_eq!(result.map(|_| ()), Err(VaultError::Cancelled));
}

#[test]
fn progress_counts_input_bytes() {
    let data = noise(CHUNK + 1000, 13);
    let mut source = Cursor::new(data.as_slice());
    let mut output = Vec::new();
    let mut last = 0u64;
    let summary = encrypt_stream(
        &mut source,
        &mut output,
        &passphrase(),
        &EncryptSettings::for_tests(false),
        &mut |bytes: u64| {
            last = bytes;
            true
        },
    )
    .unwrap();
    assert_eq!(last, data.len() as u64);
    assert_eq!(summary.input_bytes, data.len() as u64);
    assert_eq!(summary.output_bytes, output.len() as u64);
}

#[test]
fn production_settings_refuse_weak_key_derivation() {
    assert!(EncryptSettings::new(true, KdfParams::for_tests()).is_err());
    assert!(EncryptSettings::new(true, KdfParams::DEFAULT).is_ok());
}

#[test]
fn already_compressed_formats_are_not_compressed_again() {
    assert!(!should_compress(Path::new("holiday.ZIP")));
    assert!(!should_compress(Path::new("photo.jpg")));
    assert!(!should_compress(Path::new("report.docx")));
    assert!(should_compress(Path::new("notes.txt")));
    assert!(should_compress(Path::new("database.sql")));
    assert!(should_compress(Path::new("no_extension")));
}
