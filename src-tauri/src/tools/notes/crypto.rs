//! Keys, encryption and the create / open / reseal operations.
//!
//! SECURITY-CRITICAL MODULE. Pipeline:
//!   normalized words --Argon2id(salt, params)--> master key
//!   master key --HKDF-SHA256(label + uuid)--> wrap key and commitment key
//!   random data key --HKDF-SHA256(label + uuid)--> body key
//! Every secret buffer is wrapped in `Zeroizing`.

use std::time::{Duration, Instant};

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use zeroize::{Zeroize, Zeroizing};

use super::error::NotesError;
use super::format::{
    AAD_WRAP_END, AEAD_TAG_LEN, BODY_NONCE_LEN, COMMIT_TAG_LEN, FLAG_USER_PASSPHRASE, HEADER_LEN,
    Header, KDF_ARGON2ID_V13, LEN_FIELD, MAX_FILE_BYTES, MAX_PAYLOAD_BYTES, NONCE_LEN,
    OFFSET_RESERVED, OFFSET_TAG, OFFSET_WRAPPED_KEY, PAD_BUCKET, SALT_LEN, TAG_COVERS, UUID_LEN,
    WRAPPED_KEY_LEN, pad_payload, padded_file_len, unpad_plaintext,
};
use crate::tools::vault::kdf::{KEY_LEN, KdfParams, MAX_T_COST, MIN_M_COST_KIB_FOR_ENCRYPTION};
use crate::tools::vault::runtime::KDF_MEMORY_KIB;

type Cipher = XChaCha20Poly1305;

const LABEL_WRAP: &[u8] = b"ERX Notes v1 wrap key";
const LABEL_COMMIT: &[u8] = b"ERX Notes v1 commitment key";
const LABEL_BODY: &[u8] = b"ERX Notes v1 body key";

/// Validated settings for one new notes file.
#[derive(Debug, Clone, Copy)]
pub struct CreateSettings {
    kdf: KdfParams,
}

impl CreateSettings {
    /// Refuses Argon2 memory below 64 MiB and out-of-range values.
    pub fn new(kdf: KdfParams) -> Result<Self, NotesError> {
        if kdf.m_cost_kib < MIN_M_COST_KIB_FOR_ENCRYPTION {
            return Err(NotesError::InvalidKdfParams);
        }
        kdf.validate_for_reading()
            .map_err(|_| NotesError::InvalidKdfParams)?;
        Ok(Self { kdf })
    }

    /// Weak and fast settings for tests only.
    #[cfg(test)]
    pub fn for_tests() -> Self {
        Self {
            kdf: KdfParams::for_tests(),
        }
    }

    /// Builds settings with an explicit KDF, skipping the 64 MiB minimum so
    /// tests can use small values.
    #[cfg(test)]
    pub fn with_kdf_for_tests(kdf: KdfParams) -> Self {
        Self { kdf }
    }
}

/// The header of a created or opened file and its body key. Wiped on drop.
pub struct NotesKeys {
    header_bytes: [u8; HEADER_LEN],
    body_key: Zeroizing<[u8; KEY_LEN]>,
}

impl std::fmt::Debug for NotesKeys {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("NotesKeys { .. }")
    }
}

impl NotesKeys {
    /// The parsed header of this file.
    #[allow(dead_code)] // used by tests only
    pub fn header(&self) -> Result<Header, NotesError> {
        Header::parse(&self.header_bytes)
    }

    /// The body key, for tests only.
    #[cfg(test)]
    pub fn body_key_for_tests(&self) -> [u8; KEY_LEN] {
        *self.body_key
    }

    /// Re-encrypts a new payload with the same header and body key, using a
    /// fresh random body nonce. No Argon2 work is done here.
    pub fn reseal(&self, payload: &[u8]) -> Result<Vec<u8>, NotesError> {
        let plaintext = pad_payload(payload)?;
        let mut body_nonce = [0u8; BODY_NONCE_LEN];
        fill_random(&mut body_nonce)?;
        let body = seal_body(&self.header_bytes, &self.body_key, &plaintext, &body_nonce)?;

        let mut file = Vec::with_capacity(HEADER_LEN + BODY_NONCE_LEN + body.len());
        file.extend_from_slice(&self.header_bytes);
        file.extend_from_slice(&body_nonce);
        file.extend_from_slice(&body);
        if file.len() != padded_file_len(payload.len()) {
            return Err(NotesError::Internal);
        }
        Ok(file)
    }

    /// Reads the payload back with the stored body key only. This is the fast
    /// check after a save: no Argon2 work and no words are involved.
    pub fn open_body(&self, file: &[u8]) -> Result<Zeroizing<Vec<u8>>, NotesError> {
        if file.len() > MAX_FILE_BYTES {
            return Err(NotesError::FileTooLarge);
        }
        if file.len() < HEADER_LEN + BODY_NONCE_LEN + AEAD_TAG_LEN + LEN_FIELD {
            return Err(NotesError::DamagedFile);
        }
        if !file.len().is_multiple_of(PAD_BUCKET) {
            return Err(NotesError::DamagedFile);
        }
        if file[..HEADER_LEN] != self.header_bytes[..] {
            return Err(NotesError::DamagedFile);
        }
        open_body_with(&self.header_bytes, &self.body_key, file)
    }
}

/// Fresh random values for one new file.
pub struct Randomness {
    pub uuid: [u8; UUID_LEN],
    pub salt: [u8; SALT_LEN],
    pub wrap_nonce: [u8; NONCE_LEN],
    pub data_key: Zeroizing<[u8; KEY_LEN]>,
    pub body_nonce: [u8; BODY_NONCE_LEN],
}

impl Randomness {
    /// Reads every value from the operating system random source.
    pub fn random() -> Result<Self, NotesError> {
        let mut uuid = [0u8; UUID_LEN];
        let mut salt = [0u8; SALT_LEN];
        let mut wrap_nonce = [0u8; NONCE_LEN];
        let mut data_key = Zeroizing::new([0u8; KEY_LEN]);
        let mut body_nonce = [0u8; BODY_NONCE_LEN];
        fill_random(&mut uuid)?;
        fill_random(&mut salt)?;
        fill_random(&mut wrap_nonce)?;
        fill_random(data_key.as_mut_slice())?;
        fill_random(&mut body_nonce)?;
        Ok(Self {
            uuid,
            salt,
            wrap_nonce,
            data_key,
            body_nonce,
        })
    }
}

fn fill_random(buffer: &mut [u8]) -> Result<(), NotesError> {
    getrandom::fill(buffer).map_err(|_| NotesError::RandomUnavailable)
}

/// Builds the cipher and wipes the temporary key array.
fn build_cipher(key: &[u8; KEY_LEN]) -> Cipher {
    let mut array = chacha20poly1305::Key::default();
    for (target, source) in array.iter_mut().zip(key.iter()) {
        *target = *source;
    }
    let cipher = Cipher::new(&array);
    let wipe: &mut [u8] = &mut array;
    wipe.zeroize();
    cipher
}

/// Argon2id (version 0x13) over the already-normalized words.
fn derive_master(
    normalized_words: &str,
    salt: &[u8; SALT_LEN],
    kdf: KdfParams,
) -> Result<Zeroizing<[u8; KEY_LEN]>, NotesError> {
    kdf.validate_for_reading()
        .map_err(|_| NotesError::InvalidKdfParams)?;
    let params = Params::new(kdf.m_cost_kib, kdf.t_cost, kdf.p_cost, Some(KEY_LEN))
        .map_err(|_| NotesError::InvalidKdfParams)?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut master = Zeroizing::new([0u8; KEY_LEN]);
    argon
        .hash_password_into(normalized_words.as_bytes(), salt, master.as_mut_slice())
        .map_err(|_| NotesError::KeyDerivationFailed)?;
    Ok(master)
}

/// HKDF-SHA256 with info = label followed by the file UUID.
fn hkdf_key(
    ikm: &[u8],
    label: &[u8],
    file_uuid: &[u8; UUID_LEN],
) -> Result<Zeroizing<[u8; KEY_LEN]>, NotesError> {
    let hkdf = Hkdf::<Sha256>::new(None, ikm);
    let mut info = Vec::with_capacity(label.len() + UUID_LEN);
    info.extend_from_slice(label);
    info.extend_from_slice(file_uuid);
    let mut out = Zeroizing::new([0u8; KEY_LEN]);
    hkdf.expand(&info, out.as_mut_slice())
        .map_err(|_| NotesError::Internal)?;
    Ok(out)
}

/// HMAC-SHA256 of the header bytes under the commitment key.
fn commitment(
    commit_key: &[u8; KEY_LEN],
    header: &[u8],
) -> Result<[u8; COMMIT_TAG_LEN], NotesError> {
    let mut mac =
        <Hmac<Sha256> as KeyInit>::new_from_slice(commit_key).map_err(|_| NotesError::Internal)?;
    mac.update(header);
    let output = mac.finalize().into_bytes();
    let bytes: &[u8] = &output;
    <[u8; COMMIT_TAG_LEN]>::try_from(bytes).map_err(|_| NotesError::Internal)
}

/// Checks the commitment tag in constant time. A mismatch means: wrong words
/// OR a changed header (the two cannot be told apart).
fn verify_commitment(
    commit_key: &[u8; KEY_LEN],
    header: &[u8],
    stored_tag: &[u8; COMMIT_TAG_LEN],
) -> Result<(), NotesError> {
    let mut mac =
        <Hmac<Sha256> as KeyInit>::new_from_slice(commit_key).map_err(|_| NotesError::Internal)?;
    mac.update(header);
    mac.verify_slice(stored_tag.as_slice())
        .map_err(|_| NotesError::WrongPassphraseOrHeader)
}

fn seal_body(
    header: &[u8; HEADER_LEN],
    body_key: &[u8; KEY_LEN],
    plaintext: &[u8],
    body_nonce: &[u8; BODY_NONCE_LEN],
) -> Result<Vec<u8>, NotesError> {
    let cipher = build_cipher(body_key);
    cipher
        .encrypt(
            &XNonce::from(*body_nonce),
            Payload {
                msg: plaintext,
                aad: header.as_slice(),
            },
        )
        .map_err(|_| NotesError::Internal)
}

/// Decrypts the body with a known header and body key, then removes padding.
fn open_body_with(
    header_bytes: &[u8; HEADER_LEN],
    body_key: &[u8; KEY_LEN],
    file: &[u8],
) -> Result<Zeroizing<Vec<u8>>, NotesError> {
    let body_nonce: [u8; BODY_NONCE_LEN] = file[HEADER_LEN..HEADER_LEN + BODY_NONCE_LEN]
        .try_into()
        .map_err(|_| NotesError::DamagedFile)?;
    let body_cipher = build_cipher(body_key);
    let plaintext = Zeroizing::new(
        body_cipher
            .decrypt(
                &XNonce::from(body_nonce),
                Payload {
                    msg: &file[HEADER_LEN + BODY_NONCE_LEN..],
                    aad: header_bytes,
                },
            )
            .map_err(|_| NotesError::DamagedFile)?,
    );
    let payload = unpad_plaintext(plaintext.as_slice())?;
    Ok(Zeroizing::new(payload.to_vec()))
}

/// Creates a file from fixed randomness. Tests use this to pin the layout.
pub(crate) fn create_with_flags(
    normalized_words: &str,
    settings: &CreateSettings,
    payload: &[u8],
    random: Randomness,
    flags: u16,
) -> Result<(Vec<u8>, NotesKeys), NotesError> {
    let plaintext = pad_payload(payload)?;

    let master = derive_master(normalized_words, &random.salt, settings.kdf)?;
    let wrap_key = hkdf_key(master.as_slice(), LABEL_WRAP, &random.uuid)?;
    let commit_key = hkdf_key(master.as_slice(), LABEL_COMMIT, &random.uuid)?;

    let header = Header {
        flags,
        file_uuid: random.uuid,
        kdf: settings.kdf,
        salt: random.salt,
        wrap_nonce: random.wrap_nonce,
        wrapped_key: [0u8; WRAPPED_KEY_LEN],
        tag: [0u8; COMMIT_TAG_LEN],
    };
    let mut header_bytes = header.to_bytes();

    let wrap_cipher = build_cipher(&wrap_key);
    let wrapped_key = wrap_cipher
        .encrypt(
            &XNonce::from(random.wrap_nonce),
            Payload {
                msg: random.data_key.as_slice(),
                aad: &header_bytes[..AAD_WRAP_END],
            },
        )
        .map_err(|_| NotesError::Internal)?;
    if wrapped_key.len() != WRAPPED_KEY_LEN {
        return Err(NotesError::Internal);
    }
    header_bytes[OFFSET_WRAPPED_KEY..OFFSET_RESERVED].copy_from_slice(&wrapped_key);
    // Bytes OFFSET_RESERVED..OFFSET_TAG stay zero.
    let tag = commitment(&commit_key, &header_bytes[..TAG_COVERS])?;
    header_bytes[OFFSET_TAG..HEADER_LEN].copy_from_slice(&tag);

    let body_key = hkdf_key(random.data_key.as_slice(), LABEL_BODY, &random.uuid)?;
    let body = seal_body(&header_bytes, &body_key, &plaintext, &random.body_nonce)?;

    let mut file = Vec::with_capacity(HEADER_LEN + BODY_NONCE_LEN + body.len());
    file.extend_from_slice(&header_bytes);
    file.extend_from_slice(&random.body_nonce);
    file.extend_from_slice(&body);
    if file.len() != padded_file_len(payload.len()) {
        return Err(NotesError::Internal);
    }
    Ok((
        file,
        NotesKeys {
            header_bytes,
            body_key,
        },
    ))
}

/// Layout-pinning entry point for notebook files: flags are always 0.
pub(crate) fn create_with(
    normalized_words: &str,
    settings: &CreateSettings,
    payload: &[u8],
    random: Randomness,
) -> Result<(Vec<u8>, NotesKeys), NotesError> {
    create_with_flags(normalized_words, settings, payload, random, 0)
}

/// Creates a new notes file with fresh randomness.
pub fn create(
    normalized_words: &str,
    settings: &CreateSettings,
    payload: &[u8],
) -> Result<(Vec<u8>, NotesKeys), NotesError> {
    create_with(normalized_words, settings, payload, Randomness::random()?)
}

/// Creates an export file: fresh randomness, FLAG_USER_PASSPHRASE set.
pub fn create_export(
    normalized_words: &str,
    settings: &CreateSettings,
    payload: &[u8],
) -> Result<(Vec<u8>, NotesKeys), NotesError> {
    create_with_flags(
        normalized_words,
        settings,
        payload,
        Randomness::random()?,
        FLAG_USER_PASSPHRASE,
    )
}

/// Opens a notes file. The returned keys can re-seal a new payload.
pub fn open(
    file: &[u8],
    normalized_words: &str,
) -> Result<(NotesKeys, Zeroizing<Vec<u8>>), NotesError> {
    if file.len() > MAX_FILE_BYTES {
        return Err(NotesError::FileTooLarge);
    }
    if file.len() < HEADER_LEN + BODY_NONCE_LEN + AEAD_TAG_LEN + LEN_FIELD {
        return Err(NotesError::DamagedFile);
    }
    let header_bytes: [u8; HEADER_LEN] = file[..HEADER_LEN]
        .try_into()
        .map_err(|_| NotesError::DamagedFile)?;
    let header = Header::parse(&header_bytes)?;
    if !file.len().is_multiple_of(PAD_BUCKET) {
        return Err(NotesError::DamagedFile);
    }

    let master = derive_master(normalized_words, &header.salt, header.kdf)?;
    let commit_key = hkdf_key(master.as_slice(), LABEL_COMMIT, &header.file_uuid)?;
    verify_commitment(&commit_key, &header_bytes[..TAG_COVERS], &header.tag)?;

    let wrap_key = hkdf_key(master.as_slice(), LABEL_WRAP, &header.file_uuid)?;
    let wrap_cipher = build_cipher(&wrap_key);
    let data_key_bytes = Zeroizing::new(
        wrap_cipher
            .decrypt(
                &XNonce::from(header.wrap_nonce),
                Payload {
                    msg: &header.wrapped_key,
                    aad: &header_bytes[..AAD_WRAP_END],
                },
            )
            .map_err(|_| NotesError::DamagedFile)?,
    );
    let data_key: Zeroizing<[u8; KEY_LEN]> = Zeroizing::new(
        <[u8; KEY_LEN]>::try_from(data_key_bytes.as_slice())
            .map_err(|_| NotesError::DamagedFile)?,
    );

    let body_key = hkdf_key(data_key.as_slice(), LABEL_BODY, &header.file_uuid)?;
    let payload = open_body_with(&header_bytes, &body_key, file)?;
    Ok((
        NotesKeys {
            header_bytes,
            body_key,
        },
        payload,
    ))
}

/// How long one Argon2 derivation should take for a new notes file.
pub const NOTES_KDF_TARGET: Duration = Duration::from_secs(1);
/// Never fewer passes than this, however fast the machine is.
pub const NOTES_KDF_MIN_PASSES: u32 = 2;

/// Number of passes that makes one derivation take about `target`, given the
/// measured time of a single pass.
pub fn params_for_target(one_pass: Duration, target: Duration) -> KdfParams {
    let micros = one_pass.as_micros().max(1);
    let wanted = (target.as_micros() + micros / 2) / micros;
    let passes = u32::try_from(wanted)
        .unwrap_or(MAX_T_COST)
        .clamp(NOTES_KDF_MIN_PASSES, MAX_T_COST);
    KdfParams {
        m_cost_kib: KDF_MEMORY_KIB,
        t_cost: passes,
        p_cost: 1,
    }
}

/// Runs one Argon2id pass with the production memory size and derives the
/// parameters for this machine from its duration. The benchmark password and
/// salt are public constants: nothing secret is involved.
pub fn tune_kdf(target: Duration) -> Result<KdfParams, NotesError> {
    let params = Params::new(KDF_MEMORY_KIB, 1, 1, Some(KEY_LEN))
        .map_err(|_| NotesError::InvalidKdfParams)?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut output = Zeroizing::new([0u8; KEY_LEN]);
    let started = Instant::now();
    argon
        .hash_password_into(
            b"erx-notes-benchmark",
            &[0u8; SALT_LEN],
            output.as_mut_slice(),
        )
        .map_err(|_| NotesError::KeyDerivationFailed)?;
    Ok(params_for_target(started.elapsed(), target))
}

// The body nonce starts exactly after the header.
const _: () = assert!(HEADER_LEN == 192);
// KDF id 1 is the only one this code writes.
const _: () = assert!(KDF_ARGON2ID_V13 == 1);
// The maximum file size is the padded maximum payload.
const _: () = assert!(MAX_FILE_BYTES == MAX_PAYLOAD_BYTES + PAD_BUCKET);
