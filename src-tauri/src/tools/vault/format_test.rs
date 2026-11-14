use super::error::VaultError;
use super::format::{
    CHUNK_SIZE, FLAG_ZSTD, HEADER_BODY_LEN, HEADER_LEN, Header, NONCE_PREFIX_LEN, SALT_LEN,
    TAG_LEN, UUID_LEN,
};
use super::kdf::KdfParams;

// Byte offsets inside the 144-byte header (see the table in format.rs).
const VERSION_AT: usize = 8;
const FLAGS_AT: usize = 10;
const KDF_ID_AT: usize = 28;
const M_COST_AT: usize = 29;
const CHUNK_AT: usize = 76;
const RESERVED_AT: usize = 80;

fn sample() -> Header {
    Header {
        flags: FLAG_ZSTD,
        file_uuid: [1u8; UUID_LEN],
        kdf: KdfParams::for_tests(),
        salt: [2u8; SALT_LEN],
        nonce_prefix: [3u8; NONCE_PREFIX_LEN],
        chunk_size: CHUNK_SIZE,
        tag: [4u8; TAG_LEN],
    }
}

fn with_bytes(offset: usize, replacement: &[u8]) -> Vec<u8> {
    let mut bytes = sample().to_bytes();
    for (index, value) in replacement.iter().enumerate() {
        bytes[offset + index] = *value;
    }
    bytes
}

#[test]
fn lengths_match_the_constants() {
    assert_eq!(sample().body_bytes().len(), HEADER_BODY_LEN);
    assert_eq!(sample().to_bytes().len(), HEADER_LEN);
}

#[test]
fn serialize_then_parse_round_trips() {
    let header = sample();
    assert_eq!(Header::parse(&header.to_bytes()), Ok(header));
}

#[test]
fn wrong_magic_or_empty_input_is_not_a_vault_file() {
    assert_eq!(Header::parse(&[]), Err(VaultError::NotAVaultFile));
    assert_eq!(
        Header::parse(&with_bytes(0, b"NOTAVAULT")),
        Err(VaultError::NotAVaultFile)
    );
}

#[test]
fn wrong_length_is_a_damaged_header() {
    let bytes = sample().to_bytes();
    assert_eq!(
        Header::parse(&bytes[..50]),
        Err(VaultError::CorruptedHeader)
    );
    let mut longer = bytes.clone();
    longer.push(0);
    assert_eq!(Header::parse(&longer), Err(VaultError::CorruptedHeader));
}

#[test]
fn version_flags_and_kdf_id_are_checked() {
    assert_eq!(
        Header::parse(&with_bytes(VERSION_AT, &[2, 0])),
        Err(VaultError::UnsupportedVersion)
    );
    assert_eq!(
        Header::parse(&with_bytes(VERSION_AT, &[0, 0])),
        Err(VaultError::CorruptedHeader)
    );
    assert_eq!(
        Header::parse(&with_bytes(FLAGS_AT, &[4, 0])),
        Err(VaultError::UnsupportedVersion)
    );
    assert_eq!(
        Header::parse(&with_bytes(KDF_ID_AT, &[2])),
        Err(VaultError::UnsupportedVersion)
    );
}

#[test]
fn reserved_chunk_size_and_kdf_ranges_are_checked() {
    assert_eq!(
        Header::parse(&with_bytes(RESERVED_AT, &[1])),
        Err(VaultError::CorruptedHeader)
    );
    assert_eq!(
        Header::parse(&with_bytes(CHUNK_AT, &[0, 0, 1, 0])),
        Err(VaultError::CorruptedHeader)
    );
    // Memory below 8 MiB and above 512 MiB are refused.
    assert_eq!(
        Header::parse(&with_bytes(M_COST_AT, &1024u32.to_le_bytes())),
        Err(VaultError::CorruptedHeader)
    );
    assert_eq!(
        Header::parse(&with_bytes(M_COST_AT, &(600u32 * 1024).to_le_bytes())),
        Err(VaultError::CorruptedHeader)
    );
}

#[test]
fn compressed_flag_is_reported() {
    let mut header = sample();
    assert!(header.is_compressed());
    header.flags = 0;
    assert!(!header.is_compressed());
}
