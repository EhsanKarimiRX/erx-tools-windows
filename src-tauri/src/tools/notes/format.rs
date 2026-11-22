//! Layout of the Secure Notes container: constants, header and padding.
//!
//! SECURITY-CRITICAL MODULE. Layout:
//!
//!   file      = header (192 bytes) | body nonce (24) | encrypted body
//!   plaintext = payload length (8, LE) | payload | zero padding
//!
//! All integers are little-endian. The whole file is padded to a multiple of
//! `PAD_BUCKET`, so its length only shows the size in 256 KiB steps.

use zeroize::Zeroizing;

use super::error::NotesError;
use crate::tools::vault::kdf::{KEY_LEN, KdfParams};

/// Magic at the start of every notes file.
pub const MAGIC: [u8; 8] = *b"ERXNOTES";
/// Container format version written by this code.
pub const FORMAT_VERSION: u16 = 1;
/// Size of the header in bytes.
pub const HEADER_LEN: usize = 192;
/// KDF id for Argon2id, version 0x13.
pub const KDF_ARGON2ID_V13: u8 = 1;

pub const UUID_LEN: usize = 16;
pub const SALT_LEN: usize = 16;
pub const NONCE_LEN: usize = 24;
/// Wrapped data key: 32 key bytes plus the 16-byte AEAD tag.
pub const WRAPPED_KEY_LEN: usize = 48;
pub const COMMIT_TAG_LEN: usize = 32;
pub const AEAD_TAG_LEN: usize = 16;

pub const OFFSET_MAGIC: usize = 0;
pub const OFFSET_VERSION: usize = 8;
pub const OFFSET_FLAGS: usize = 10;
pub const OFFSET_UUID: usize = 12;
pub const OFFSET_KDF_ID: usize = 28;
pub const OFFSET_M_COST: usize = 29;
pub const OFFSET_T_COST: usize = 33;
pub const OFFSET_P_COST: usize = 37;
pub const OFFSET_SALT: usize = 41;
pub const OFFSET_WRAP_NONCE: usize = 57;
pub const OFFSET_WRAPPED_KEY: usize = 81;
pub const OFFSET_RESERVED: usize = 129;
pub const OFFSET_TAG: usize = 160;

/// The wrap AEAD authenticates header bytes 0..81.
pub const AAD_WRAP_END: usize = 81;
/// The commitment tag covers header bytes 0..160.
pub const TAG_COVERS: usize = 160;

/// Size of one padding bucket (256 KiB).
pub const PAD_BUCKET: usize = 262_144;
/// Largest payload a notes file may hold (64 MiB).
pub const MAX_PAYLOAD_BYTES: usize = 67_108_864;
/// Nonce in front of the encrypted body.
pub const BODY_NONCE_LEN: usize = 24;
/// Length field at the start of the plaintext.
pub const LEN_FIELD: usize = 8;

/// The parsed header. It holds no secrets, so `Debug` is safe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub flags: u16,
    pub file_uuid: [u8; UUID_LEN],
    pub kdf: KdfParams,
    pub salt: [u8; SALT_LEN],
    pub wrap_nonce: [u8; NONCE_LEN],
    pub wrapped_key: [u8; WRAPPED_KEY_LEN],
    pub tag: [u8; COMMIT_TAG_LEN],
}

impl Header {
    /// Serializes the header into exactly `HEADER_LEN` bytes.
    pub fn to_bytes(&self) -> [u8; HEADER_LEN] {
        let mut out = [0u8; HEADER_LEN];
        out[OFFSET_MAGIC..OFFSET_VERSION].copy_from_slice(&MAGIC);
        out[OFFSET_VERSION..OFFSET_FLAGS].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
        out[OFFSET_FLAGS..OFFSET_UUID].copy_from_slice(&self.flags.to_le_bytes());
        out[OFFSET_UUID..OFFSET_KDF_ID].copy_from_slice(&self.file_uuid);
        out[OFFSET_KDF_ID] = KDF_ARGON2ID_V13;
        out[OFFSET_M_COST..OFFSET_T_COST].copy_from_slice(&self.kdf.m_cost_kib.to_le_bytes());
        out[OFFSET_T_COST..OFFSET_P_COST].copy_from_slice(&self.kdf.t_cost.to_le_bytes());
        out[OFFSET_P_COST..OFFSET_SALT].copy_from_slice(&self.kdf.p_cost.to_le_bytes());
        out[OFFSET_SALT..OFFSET_WRAP_NONCE].copy_from_slice(&self.salt);
        out[OFFSET_WRAP_NONCE..OFFSET_WRAPPED_KEY].copy_from_slice(&self.wrap_nonce);
        out[OFFSET_WRAPPED_KEY..OFFSET_RESERVED].copy_from_slice(&self.wrapped_key);
        // Bytes OFFSET_RESERVED..OFFSET_TAG stay zero.
        out[OFFSET_TAG..HEADER_LEN].copy_from_slice(&self.tag);
        out
    }

    /// Parses a header strictly. Checks run in this order: length, magic,
    /// version, flags, KDF id, KDF parameters, reserved bytes.
    pub fn parse(bytes: &[u8]) -> Result<Header, NotesError> {
        if bytes.len() != HEADER_LEN {
            return Err(NotesError::DamagedFile);
        }
        if bytes[OFFSET_MAGIC..OFFSET_VERSION] != MAGIC {
            return Err(NotesError::NotANotesFile);
        }
        let version = u16::from_le_bytes(read_array(bytes, OFFSET_VERSION));
        if version != FORMAT_VERSION {
            return Err(NotesError::UnsupportedVersion);
        }
        let flags = u16::from_le_bytes(read_array(bytes, OFFSET_FLAGS));
        if flags != 0 {
            return Err(NotesError::UnsupportedVersion);
        }
        if bytes[OFFSET_KDF_ID] != KDF_ARGON2ID_V13 {
            return Err(NotesError::DamagedFile);
        }
        let kdf = KdfParams {
            m_cost_kib: u32::from_le_bytes(read_array(bytes, OFFSET_M_COST)),
            t_cost: u32::from_le_bytes(read_array(bytes, OFFSET_T_COST)),
            p_cost: u32::from_le_bytes(read_array(bytes, OFFSET_P_COST)),
        };
        kdf.validate_for_reading()
            .map_err(|_| NotesError::InvalidKdfParams)?;
        if bytes[OFFSET_RESERVED..OFFSET_TAG]
            .iter()
            .any(|byte| *byte != 0)
        {
            return Err(NotesError::DamagedFile);
        }
        Ok(Header {
            flags,
            file_uuid: read_array(bytes, OFFSET_UUID),
            kdf,
            salt: read_array(bytes, OFFSET_SALT),
            wrap_nonce: read_array(bytes, OFFSET_WRAP_NONCE),
            wrapped_key: read_array(bytes, OFFSET_WRAPPED_KEY),
            tag: read_array(bytes, OFFSET_TAG),
        })
    }
}

fn read_array<const N: usize>(bytes: &[u8], offset: usize) -> [u8; N] {
    let mut out = [0u8; N];
    out.copy_from_slice(&bytes[offset..offset + N]);
    out
}

/// Total file length for a payload, rounded up to a whole number of padding
/// buckets. The smallest possible file is one bucket.
pub const fn padded_file_len(payload_len: usize) -> usize {
    let raw = HEADER_LEN + BODY_NONCE_LEN + LEN_FIELD + payload_len + AEAD_TAG_LEN;
    let rounded = raw.div_ceil(PAD_BUCKET) * PAD_BUCKET;
    if rounded < PAD_BUCKET {
        PAD_BUCKET
    } else {
        rounded
    }
}

/// Largest possible notes file for `MAX_PAYLOAD_BYTES`.
pub const MAX_FILE_BYTES: usize = padded_file_len(MAX_PAYLOAD_BYTES);

/// Builds the padded plaintext: length, payload, then zeros.
pub fn pad_payload(payload: &[u8]) -> Result<Zeroizing<Vec<u8>>, NotesError> {
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(NotesError::NotesTooLarge);
    }
    let total = padded_file_len(payload.len()) - HEADER_LEN - BODY_NONCE_LEN - AEAD_TAG_LEN;
    let mut plaintext = Zeroizing::new(vec![0u8; total]);
    plaintext[..LEN_FIELD].copy_from_slice(&(payload.len() as u64).to_le_bytes());
    plaintext[LEN_FIELD..LEN_FIELD + payload.len()].copy_from_slice(payload);
    Ok(plaintext)
}

/// Reads the length field, returns the payload and rejects non-zero padding.
pub fn unpad_plaintext(plaintext: &[u8]) -> Result<&[u8], NotesError> {
    if plaintext.len() < LEN_FIELD {
        return Err(NotesError::DamagedFile);
    }
    let length = u64::from_le_bytes(read_array(plaintext, 0));
    if length > (plaintext.len() - LEN_FIELD) as u64 || length > MAX_PAYLOAD_BYTES as u64 {
        return Err(NotesError::DamagedFile);
    }
    let length = length as usize;
    let end = LEN_FIELD + length;
    if plaintext[end..].iter().any(|byte| *byte != 0) {
        return Err(NotesError::DamagedFile);
    }
    Ok(&plaintext[LEN_FIELD..end])
}

// The vault key length is the same 32 bytes; the test module asserts it.
const _: () = assert!(KEY_LEN == 32);
