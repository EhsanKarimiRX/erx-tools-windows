//! Layout and padding tests for the Secure Notes container.

use super::error::NotesError;
use super::format::*;
use crate::tools::vault::kdf::{KEY_LEN, KdfParams};

fn sample_header() -> Header {
    Header {
        flags: 0,
        file_uuid: [0x11; UUID_LEN],
        kdf: KdfParams {
            m_cost_kib: 128 * 1024,
            t_cost: 3,
            p_cost: 1,
        },
        salt: [0x22; SALT_LEN],
        wrap_nonce: [0x33; NONCE_LEN],
        wrapped_key: [0x44; WRAPPED_KEY_LEN],
        tag: [0x55; COMMIT_TAG_LEN],
    }
}

fn write_u16(bytes: &mut [u8; HEADER_LEN], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_u32(bytes: &mut [u8; HEADER_LEN], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

#[test]
fn constants_and_offsets_add_up() {
    assert_eq!(KEY_LEN, 32);
    assert_eq!(PAD_BUCKET, 262_144);
    assert_eq!(HEADER_LEN, 192);
    assert_eq!(BODY_NONCE_LEN, 24);
    assert_eq!(LEN_FIELD, 8);
    assert_eq!(AEAD_TAG_LEN, 16);
    assert_eq!(WRAPPED_KEY_LEN, 48);
    assert_eq!(COMMIT_TAG_LEN, 32);
    assert_eq!(AAD_WRAP_END, 81);
    assert_eq!(TAG_COVERS, 160);

    assert_eq!(OFFSET_MAGIC, 0);
    assert_eq!(OFFSET_VERSION, 8);
    assert_eq!(OFFSET_FLAGS, 10);
    assert_eq!(OFFSET_UUID, 12);
    assert_eq!(OFFSET_KDF_ID, 28);
    assert_eq!(OFFSET_M_COST, 29);
    assert_eq!(OFFSET_T_COST, 33);
    assert_eq!(OFFSET_P_COST, 37);
    assert_eq!(OFFSET_SALT, 41);
    assert_eq!(OFFSET_WRAP_NONCE, 57);
    assert_eq!(OFFSET_WRAPPED_KEY, 81);
    assert_eq!(OFFSET_RESERVED, 129);
    assert_eq!(OFFSET_TAG, 160);

    assert_eq!(OFFSET_MAGIC + 8, OFFSET_VERSION);
    assert_eq!(OFFSET_VERSION + 2, OFFSET_FLAGS);
    assert_eq!(OFFSET_FLAGS + 2, OFFSET_UUID);
    assert_eq!(OFFSET_UUID + UUID_LEN, OFFSET_KDF_ID);
    assert_eq!(OFFSET_KDF_ID + 1, OFFSET_M_COST);
    assert_eq!(OFFSET_M_COST + 4, OFFSET_T_COST);
    assert_eq!(OFFSET_T_COST + 4, OFFSET_P_COST);
    assert_eq!(OFFSET_P_COST + 4, OFFSET_SALT);
    assert_eq!(OFFSET_SALT + SALT_LEN, OFFSET_WRAP_NONCE);
    assert_eq!(OFFSET_WRAP_NONCE + NONCE_LEN, OFFSET_WRAPPED_KEY);
    assert_eq!(OFFSET_WRAPPED_KEY + WRAPPED_KEY_LEN, OFFSET_RESERVED);
    assert_eq!(OFFSET_RESERVED + 31, OFFSET_TAG);
    assert_eq!(OFFSET_TAG + COMMIT_TAG_LEN, HEADER_LEN);
    assert_eq!(OFFSET_WRAPPED_KEY, AAD_WRAP_END);
    assert_eq!(OFFSET_TAG, TAG_COVERS);
}

#[test]
fn header_round_trip_and_field_offsets() {
    let header = sample_header();
    let bytes = header.to_bytes();

    assert_eq!(&bytes[OFFSET_MAGIC..OFFSET_VERSION], &MAGIC);
    assert_eq!(
        u16::from_le_bytes(bytes[OFFSET_VERSION..OFFSET_FLAGS].try_into().unwrap()),
        FORMAT_VERSION
    );
    assert_eq!(
        u16::from_le_bytes(bytes[OFFSET_FLAGS..OFFSET_UUID].try_into().unwrap()),
        header.flags
    );
    assert_eq!(&bytes[OFFSET_UUID..OFFSET_KDF_ID], &header.file_uuid);
    assert_eq!(bytes[OFFSET_KDF_ID], KDF_ARGON2ID_V13);
    assert_eq!(
        u32::from_le_bytes(bytes[OFFSET_M_COST..OFFSET_T_COST].try_into().unwrap()),
        header.kdf.m_cost_kib
    );
    assert_eq!(
        u32::from_le_bytes(bytes[OFFSET_T_COST..OFFSET_P_COST].try_into().unwrap()),
        header.kdf.t_cost
    );
    assert_eq!(
        u32::from_le_bytes(bytes[OFFSET_P_COST..OFFSET_SALT].try_into().unwrap()),
        header.kdf.p_cost
    );
    assert_eq!(&bytes[OFFSET_SALT..OFFSET_WRAP_NONCE], &header.salt);
    assert_eq!(
        &bytes[OFFSET_WRAP_NONCE..OFFSET_WRAPPED_KEY],
        &header.wrap_nonce
    );
    assert_eq!(
        &bytes[OFFSET_WRAPPED_KEY..OFFSET_RESERVED],
        &header.wrapped_key
    );
    assert!(
        bytes[OFFSET_RESERVED..OFFSET_TAG]
            .iter()
            .all(|byte| *byte == 0)
    );
    assert_eq!(&bytes[OFFSET_TAG..HEADER_LEN], &header.tag);

    assert_eq!(Header::parse(&bytes).unwrap(), header);
}

#[test]
fn parse_rejects_bad_headers_in_the_documented_order() {
    let base = sample_header().to_bytes();

    assert_eq!(
        Header::parse(&base[..HEADER_LEN - 1]).unwrap_err(),
        NotesError::DamagedFile
    );
    let mut long = base.to_vec();
    long.push(0);
    assert_eq!(Header::parse(&long).unwrap_err(), NotesError::DamagedFile);

    let mut bytes = base;
    bytes[OFFSET_MAGIC] = b'X';
    assert_eq!(
        Header::parse(&bytes).unwrap_err(),
        NotesError::NotANotesFile
    );

    let mut bytes = base;
    write_u16(&mut bytes, OFFSET_VERSION, 2);
    assert_eq!(
        Header::parse(&bytes).unwrap_err(),
        NotesError::UnsupportedVersion
    );

    let mut bytes = base;
    write_u16(&mut bytes, OFFSET_VERSION, 0);
    assert_eq!(
        Header::parse(&bytes).unwrap_err(),
        NotesError::UnsupportedVersion
    );

    let mut bytes = base;
    write_u16(&mut bytes, OFFSET_FLAGS, 2);
    assert_eq!(
        Header::parse(&bytes).unwrap_err(),
        NotesError::UnsupportedVersion
    );

    let mut bytes = base;
    bytes[OFFSET_KDF_ID] = 2;
    assert_eq!(Header::parse(&bytes).unwrap_err(), NotesError::DamagedFile);

    let mut bytes = base;
    bytes[OFFSET_RESERVED] = 1;
    assert_eq!(Header::parse(&bytes).unwrap_err(), NotesError::DamagedFile);

    for m_cost in [7 * 1024, 513 * 1024] {
        let mut bytes = base;
        write_u32(&mut bytes, OFFSET_M_COST, m_cost);
        assert_eq!(
            Header::parse(&bytes).unwrap_err(),
            NotesError::InvalidKdfParams
        );
    }
    for t_cost in [0, 11] {
        let mut bytes = base;
        write_u32(&mut bytes, OFFSET_T_COST, t_cost);
        assert_eq!(
            Header::parse(&bytes).unwrap_err(),
            NotesError::InvalidKdfParams
        );
    }
    for p_cost in [0, 9] {
        let mut bytes = base;
        write_u32(&mut bytes, OFFSET_P_COST, p_cost);
        assert_eq!(
            Header::parse(&bytes).unwrap_err(),
            NotesError::InvalidKdfParams
        );
    }

    for (m_cost, t_cost, p_cost) in [(8 * 1024, 1, 1), (512 * 1024, 10, 8)] {
        let mut bytes = base;
        write_u32(&mut bytes, OFFSET_M_COST, m_cost);
        write_u32(&mut bytes, OFFSET_T_COST, t_cost);
        write_u32(&mut bytes, OFFSET_P_COST, p_cost);
        assert!(Header::parse(&bytes).is_ok());
    }

    let mut bytes = base;
    bytes[OFFSET_MAGIC] = b'X';
    write_u16(&mut bytes, OFFSET_VERSION, 2);
    assert_eq!(
        Header::parse(&bytes).unwrap_err(),
        NotesError::NotANotesFile
    );
}

#[test]
fn padded_lengths_use_whole_buckets() {
    assert_eq!(padded_file_len(0), PAD_BUCKET);
    assert_eq!(padded_file_len(261_904), PAD_BUCKET);
    assert_eq!(padded_file_len(261_905), 2 * PAD_BUCKET);
    assert_eq!(padded_file_len(524_288 - 240), 2 * PAD_BUCKET);
    assert_eq!(padded_file_len(524_288 - 239), 3 * PAD_BUCKET);
    assert_eq!(padded_file_len(MAX_PAYLOAD_BYTES), MAX_FILE_BYTES);
    assert_eq!(MAX_FILE_BYTES, 67_108_864 + 262_144);
}

#[test]
fn padding_writes_length_payload_and_zeros() {
    for size in [0usize, 1, 1000, 261_904, 261_905] {
        let payload = vec![0xA5u8; size];
        let plaintext = pad_payload(&payload).unwrap();
        assert_eq!(
            plaintext.len() + HEADER_LEN + BODY_NONCE_LEN + AEAD_TAG_LEN,
            padded_file_len(size)
        );
        assert_eq!(&plaintext[..LEN_FIELD], &(size as u64).to_le_bytes());
        assert_eq!(&plaintext[LEN_FIELD..LEN_FIELD + size], payload.as_slice());
        assert!(plaintext[LEN_FIELD + size..].iter().all(|byte| *byte == 0));
    }

    let too_big = vec![0u8; MAX_PAYLOAD_BYTES + 1];
    assert_eq!(
        pad_payload(&too_big).unwrap_err(),
        NotesError::NotesTooLarge
    );
}

#[test]
fn unpadding_reads_the_length_and_checks_the_zeros() {
    for size in [0usize, 1, 1000, 261_904, 261_905] {
        let payload = vec![0x5Au8; size];
        let plaintext = pad_payload(&payload).unwrap();
        assert_eq!(unpad_plaintext(&plaintext).unwrap(), payload.as_slice());
    }

    assert_eq!(
        unpad_plaintext(&[0u8; 7]).unwrap_err(),
        NotesError::DamagedFile
    );

    let mut too_long = vec![0u8; 12];
    too_long[..LEN_FIELD].copy_from_slice(&5u64.to_le_bytes());
    assert_eq!(
        unpad_plaintext(&too_long).unwrap_err(),
        NotesError::DamagedFile
    );

    let mut above_max = vec![0u8; LEN_FIELD];
    above_max[..LEN_FIELD].copy_from_slice(&(MAX_PAYLOAD_BYTES as u64 + 1).to_le_bytes());
    assert_eq!(
        unpad_plaintext(&above_max).unwrap_err(),
        NotesError::DamagedFile
    );

    let mut dirty_padding = pad_payload(&[1, 2, 3]).unwrap();
    *dirty_padding.last_mut().unwrap() = 1;
    assert_eq!(
        unpad_plaintext(&dirty_padding).unwrap_err(),
        NotesError::DamagedFile
    );
}

#[test]
fn parse_accepts_only_known_flag_bits() {
    let base = sample_header().to_bytes();

    let mut bytes = base;
    write_u16(&mut bytes, OFFSET_FLAGS, FLAG_USER_PASSPHRASE);
    assert_eq!(Header::parse(&bytes).unwrap().flags, FLAG_USER_PASSPHRASE);

    for bad in [2u16, 3, 0x8000] {
        let mut bytes = base;
        write_u16(&mut bytes, OFFSET_FLAGS, bad);
        assert_eq!(
            Header::parse(&bytes).unwrap_err(),
            NotesError::UnsupportedVersion
        );
    }
}
