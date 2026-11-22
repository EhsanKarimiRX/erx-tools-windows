//! Round-trip, tamper, layout and KDF tests for the Secure Notes container.

use std::time::Duration;

use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use super::crypto::*;
use super::error::NotesError;
use super::format::*;
use crate::tools::vault::kdf::KdfParams;

const WORDS: &str = "alpha bravo charlie delta echo";
const OTHER_WORDS: &str = "zulu yankee xray whiskey victor";
const FROZEN_FILE_SHA256: &str = "eaf422b9007b40e6bcc2dc1257e039ae38175e3882031a15db15a7291a62ad00";

fn test_settings() -> CreateSettings {
    CreateSettings::for_tests()
}

fn frozen_file() -> Vec<u8> {
    let random = Randomness {
        uuid: [0x11; UUID_LEN],
        salt: [0x22; SALT_LEN],
        wrap_nonce: [0x33; NONCE_LEN],
        data_key: Zeroizing::new([0x44; 32]),
        body_nonce: [0x55; BODY_NONCE_LEN],
    };
    let (file, _keys) = create_with(
        WORDS,
        &test_settings(),
        b"ERX frozen vector payload",
        random,
    )
    .unwrap();
    file
}

#[test]
fn round_trip_returns_the_same_payload() {
    let pattern: Vec<u8> = (0..1024 * 1024).map(|index| (index % 251) as u8).collect();
    let persian = "\u{0633}\u{0644}\u{0627}\u{0645} \u{0645}\u{06CC}\u{200C}\u{062E}\u{0648}\u{0627}\u{0647}\u{0645} \u{06F1}\u{06F2}\u{06F3} hello";
    let payloads: Vec<Vec<u8>> = vec![
        Vec::new(),
        vec![0x7F],
        vec![0x5A; 261_904],
        vec![0xA5; 261_905],
        pattern,
        persian.as_bytes().to_vec(),
    ];

    for payload in payloads {
        let (file, keys) = create(WORDS, &test_settings(), &payload).unwrap();
        assert_eq!(file.len(), padded_file_len(payload.len()));
        assert_eq!(file.len() % PAD_BUCKET, 0);
        assert_eq!(keys.header().unwrap().file_uuid.len(), UUID_LEN);

        let (opened_keys, opened_payload) = open(&file, WORDS).unwrap();
        assert_eq!(opened_payload.as_slice(), payload.as_slice());
        assert_eq!(opened_keys.body_key_for_tests(), keys.body_key_for_tests());
    }
}

#[test]
fn wrong_words_are_reported_as_wrong_words_or_header() {
    let (file, _keys) = create(WORDS, &test_settings(), b"wrong words payload").unwrap();

    for words in [
        "Alpha bravo charlie delta echo",
        "alpha  bravo charlie delta echo",
        "alpha bravo charlie delta echo ",
        OTHER_WORDS,
    ] {
        assert_eq!(
            open(&file, words).unwrap_err(),
            NotesError::WrongPassphraseOrHeader
        );
    }
}

#[test]
fn fixed_randomness_produces_the_expected_layout() {
    let file = frozen_file();
    let kdf = KdfParams::for_tests();

    let mut expected = Vec::new();
    expected.extend_from_slice(b"ERXNOTES");
    expected.extend_from_slice(&1u16.to_le_bytes());
    expected.extend_from_slice(&0u16.to_le_bytes());
    expected.extend_from_slice(&[0x11; UUID_LEN]);
    expected.push(1);
    expected.extend_from_slice(&kdf.m_cost_kib.to_le_bytes());
    expected.extend_from_slice(&kdf.t_cost.to_le_bytes());
    expected.extend_from_slice(&kdf.p_cost.to_le_bytes());
    expected.extend_from_slice(&[0x22; SALT_LEN]);
    assert_eq!(&file[..57], expected.as_slice());

    assert!(file[57..81].iter().all(|byte| *byte == 0x33));
    assert!(file[129..160].iter().all(|byte| *byte == 0));
    assert!(file[192..216].iter().all(|byte| *byte == 0x55));
    assert_eq!(file.len(), PAD_BUCKET);
}

#[test]
fn frozen_vector_hash() {
    let file = frozen_file();
    let digest = Sha256::digest(&file);
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    assert_eq!(hex, FROZEN_FILE_SHA256);
}

#[test]
fn every_header_byte_is_authenticated() {
    let (file, _keys) = create(WORDS, &test_settings(), b"header tamper payload").unwrap();

    for index in 0..HEADER_LEN {
        let mut tampered = file.clone();
        tampered[index] ^= 1;
        let error = open(&tampered, WORDS).unwrap_err();

        let must_be_words_or_header = (12..16).contains(&index)
            || (41..57).contains(&index)
            || (57..81).contains(&index)
            || (81..129).contains(&index)
            || (160..192).contains(&index);
        if must_be_words_or_header {
            assert_eq!(error, NotesError::WrongPassphraseOrHeader, "index {index}");
        }
    }
}

#[test]
fn body_changes_and_shortening_are_damage() {
    let (file, _keys) = create(WORDS, &test_settings(), b"body tamper payload").unwrap();

    for index in [
        HEADER_LEN,
        HEADER_LEN + BODY_NONCE_LEN,
        file.len() / 2,
        file.len() - 1,
    ] {
        let mut tampered = file.clone();
        tampered[index] ^= 1;
        assert_eq!(
            open(&tampered, WORDS).unwrap_err(),
            NotesError::DamagedFile,
            "index {index}"
        );
    }

    let mut shortened = file.clone();
    shortened.pop();
    assert_eq!(
        open(&shortened, WORDS).unwrap_err(),
        NotesError::DamagedFile
    );

    let mut extended = file.clone();
    extended.push(0);
    assert_eq!(open(&extended, WORDS).unwrap_err(), NotesError::DamagedFile);

    assert_eq!(open(&[], WORDS).unwrap_err(), NotesError::DamagedFile);
    assert_eq!(
        open(&file[..191], WORDS).unwrap_err(),
        NotesError::DamagedFile
    );
    assert_eq!(
        open(&file[..HEADER_LEN], WORDS).unwrap_err(),
        NotesError::DamagedFile
    );
}

#[test]
fn splicing_a_foreign_header_breaks_the_body() {
    let (file_a, _keys_a) = create(WORDS, &test_settings(), b"splice payload A").unwrap();
    let (file_b, _keys_b) = create(WORDS, &test_settings(), b"splice payload B").unwrap();
    assert_ne!(&file_a[12..28], &file_b[12..28]);

    let mut spliced = file_b[..HEADER_LEN].to_vec();
    spliced.extend_from_slice(&file_a[HEADER_LEN..]);
    assert_eq!(open(&spliced, WORDS).unwrap_err(), NotesError::DamagedFile);
}

#[test]
fn size_limits_are_enforced_before_heavy_work() {
    let too_large_file = vec![0u8; MAX_FILE_BYTES + 1];
    assert_eq!(
        open(&too_large_file, WORDS).unwrap_err(),
        NotesError::FileTooLarge
    );

    let too_large_payload = vec![0u8; MAX_PAYLOAD_BYTES + 1];
    assert_eq!(
        create(WORDS, &test_settings(), &too_large_payload).unwrap_err(),
        NotesError::NotesTooLarge
    );
}

#[test]
fn reseal_keeps_the_header_and_changes_the_body_nonce() {
    let (file1, keys1) = create(WORDS, &test_settings(), b"first payload").unwrap();
    let file2 = keys1.reseal(b"second payload").unwrap();

    assert_eq!(&file2[..HEADER_LEN], &file1[..HEADER_LEN]);
    assert_ne!(
        &file2[HEADER_LEN..HEADER_LEN + BODY_NONCE_LEN],
        &file1[HEADER_LEN..HEADER_LEN + BODY_NONCE_LEN]
    );
    assert_eq!(open(&file2, WORDS).unwrap().1.as_slice(), b"second payload");

    let file3 = keys1.reseal(b"same payload").unwrap();
    let file4 = keys1.reseal(b"same payload").unwrap();
    assert_ne!(file3, file4);
    assert_eq!(open(&file3, WORDS).unwrap().1.as_slice(), b"same payload");
    assert_eq!(open(&file4, WORDS).unwrap().1.as_slice(), b"same payload");

    let (opened_keys, _payload) = open(&file1, WORDS).unwrap();
    let file5 = opened_keys.reseal(b"from open").unwrap();
    assert_eq!(open(&file5, WORDS).unwrap().1.as_slice(), b"from open");
}

#[test]
fn changing_the_words_creates_a_different_file() {
    let payload = b"passphrase change payload";
    let (file1, keys1) = create(WORDS, &test_settings(), payload).unwrap();
    let (file2, keys2) = create(OTHER_WORDS, &test_settings(), payload).unwrap();

    assert_ne!(&file1[12..28], &file2[12..28]);
    assert_ne!(&file1[41..57], &file2[41..57]);
    assert_ne!(keys1.body_key_for_tests(), keys2.body_key_for_tests());

    assert_eq!(
        open(&file2, WORDS).unwrap_err(),
        NotesError::WrongPassphraseOrHeader
    );
    assert_eq!(open(&file2, OTHER_WORDS).unwrap().1.as_slice(), payload);
    assert_eq!(open(&file1, WORDS).unwrap().1.as_slice(), payload);
}

#[test]
fn create_settings_reject_weak_or_out_of_range_values() {
    let weak_memory = KdfParams {
        m_cost_kib: 63 * 1024,
        t_cost: 1,
        p_cost: 1,
    };
    assert_eq!(
        CreateSettings::new(weak_memory).unwrap_err(),
        NotesError::InvalidKdfParams
    );

    let zero_passes = KdfParams {
        m_cost_kib: 64 * 1024,
        t_cost: 0,
        p_cost: 1,
    };
    assert_eq!(
        CreateSettings::new(zero_passes).unwrap_err(),
        NotesError::InvalidKdfParams
    );

    let too_much_memory = KdfParams {
        m_cost_kib: 600 * 1024,
        t_cost: 1,
        p_cost: 1,
    };
    assert_eq!(
        CreateSettings::new(too_much_memory).unwrap_err(),
        NotesError::InvalidKdfParams
    );

    let accepted = KdfParams {
        m_cost_kib: 64 * 1024,
        t_cost: 1,
        p_cost: 1,
    };
    assert!(CreateSettings::new(accepted).is_ok());
}

#[test]
fn params_for_target_picks_the_expected_passes() {
    let cases = [
        (Duration::from_millis(250), 4),
        (Duration::from_millis(1000), 2),
        (Duration::from_millis(400), 3),
        (Duration::from_millis(10), 10),
        (Duration::from_millis(0), 10),
    ];
    for (one_pass, expected) in cases {
        let params = params_for_target(one_pass, Duration::from_secs(1));
        assert_eq!(
            params,
            KdfParams {
                m_cost_kib: 262_144,
                t_cost: expected,
                p_cost: 1,
            }
        );
    }
}

#[test]
fn keys_debug_never_shows_key_material() {
    let (_file, keys) = create(WORDS, &test_settings(), b"debug payload").unwrap();
    assert_eq!(format!("{keys:?}"), "NotesKeys { .. }");
}

#[test]
#[ignore]
fn max_size_payload_round_trips() {
    let payload = vec![0x5Au8; MAX_PAYLOAD_BYTES];
    let (file, _keys) = create(WORDS, &test_settings(), &payload).unwrap();
    assert_eq!(file.len(), MAX_FILE_BYTES);

    let (_opened_keys, opened_payload) = open(&file, WORDS).unwrap();
    assert_eq!(opened_payload.as_slice(), payload.as_slice());
}

#[test]
fn a_wrong_words_attempt_does_not_break_later_use() {
    let (file, keys) = create(WORDS, &test_settings(), b"first attempt").unwrap();

    assert_eq!(
        open(&file, OTHER_WORDS).unwrap_err(),
        NotesError::WrongPassphraseOrHeader
    );
    assert_eq!(open(&file, WORDS).unwrap().1.as_slice(), b"first attempt");

    let resealed = keys.reseal(b"after the attempt").unwrap();
    assert_eq!(
        open(&resealed, WORDS).unwrap().1.as_slice(),
        b"after the attempt"
    );
}
