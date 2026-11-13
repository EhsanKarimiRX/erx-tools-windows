//! The .erx file header: layout, serialization, strict parsing.
//!
//! All integers are little-endian. The header is fixed size (144 bytes):
//!
//!   offset  size  field
//!        0     8  magic "ERXVAULT"
//!        8     2  format version (1)
//!       10     2  flags (bit 0: payload is zstd-compressed)
//!       12    16  file UUID (random)
//!       28     1  KDF id (1 = Argon2id v0x13)
//!       29     4  Argon2 memory cost in KiB
//!       33     4  Argon2 time cost
//!       37     4  Argon2 parallelism
//!       41    16  Argon2 salt (random)
//!       57    19  STREAM nonce prefix (random)
//!       76     4  plaintext chunk size (1 MiB)
//!       80    32  reserved, must be zero (room for a future key slot)
//!      112    32  commitment tag = HMAC-SHA256(commit key, bytes 0..112)
//!
//! SECURITY: the whole 144-byte header is the AAD of every encrypted chunk,
//! and the commitment tag lets a wrong passphrase be detected before any
//! chunk is decrypted. Parsing is strict: every unexpected value is refused.

use super::error::VaultError;
use super::kdf::KdfParams;

pub const MAGIC: [u8; 8] = *b"ERXVAULT";
pub const FORMAT_VERSION: u16 = 1;
/// Flag bit 0: the payload is a zstd frame (otherwise raw bytes).
pub const FLAG_ZSTD: u16 = 1;
const KNOWN_FLAGS: u16 = FLAG_ZSTD;
pub const KDF_ARGON2ID_V13: u8 = 1;
pub const UUID_LEN: usize = 16;
pub const SALT_LEN: usize = 16;
pub const NONCE_PREFIX_LEN: usize = 19;
pub const RESERVED_LEN: usize = 32;
pub const TAG_LEN: usize = 32;
/// Bytes covered by the commitment tag.
pub const HEADER_BODY_LEN: usize = 112;
/// Total header size (body + tag).
pub const HEADER_LEN: usize = 144;
/// Plaintext bytes per encrypted chunk. Fixed for format version 1.
pub const CHUNK_SIZE: u32 = 1_048_576;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub flags: u16,
    pub file_uuid: [u8; UUID_LEN],
    pub kdf: KdfParams,
    pub salt: [u8; SALT_LEN],
    pub nonce_prefix: [u8; NONCE_PREFIX_LEN],
    pub chunk_size: u32,
    pub tag: [u8; TAG_LEN],
}

impl Header {
    /// True when the payload is zstd-compressed.
    pub fn is_compressed(&self) -> bool {
        self.flags & FLAG_ZSTD != 0
    }

    /// The first 112 bytes (everything except the tag): the input of the
    /// commitment tag.
    pub fn body_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_BODY_LEN);
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        out.extend_from_slice(&self.flags.to_le_bytes());
        out.extend_from_slice(&self.file_uuid);
        out.push(KDF_ARGON2ID_V13);
        out.extend_from_slice(&self.kdf.m_cost_kib.to_le_bytes());
        out.extend_from_slice(&self.kdf.t_cost.to_le_bytes());
        out.extend_from_slice(&self.kdf.p_cost.to_le_bytes());
        out.extend_from_slice(&self.salt);
        out.extend_from_slice(&self.nonce_prefix);
        out.extend_from_slice(&self.chunk_size.to_le_bytes());
        out.extend_from_slice(&[0u8; RESERVED_LEN]);
        out
    }

    /// The full 144 header bytes as written to the file.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = self.body_bytes();
        out.extend_from_slice(&self.tag);
        out
    }

    /// Strict parser for exactly 144 header bytes.
    pub fn parse(bytes: &[u8]) -> Result<Self, VaultError> {
        if bytes.get(..MAGIC.len()) != Some(MAGIC.as_slice()) {
            return Err(VaultError::NotAVaultFile);
        }
        if bytes.len() != HEADER_LEN {
            return Err(VaultError::CorruptedHeader);
        }
        let mut reader = Reader::new(bytes);
        let _magic = reader.array::<8>()?;
        let version = reader.u16()?;
        if version == 0 {
            return Err(VaultError::CorruptedHeader);
        }
        if version > FORMAT_VERSION {
            return Err(VaultError::UnsupportedVersion);
        }
        let flags = reader.u16()?;
        if flags & !KNOWN_FLAGS != 0 {
            return Err(VaultError::UnsupportedVersion);
        }
        let file_uuid = reader.array::<UUID_LEN>()?;
        if reader.u8()? != KDF_ARGON2ID_V13 {
            return Err(VaultError::UnsupportedVersion);
        }
        let kdf = KdfParams {
            m_cost_kib: reader.u32()?,
            t_cost: reader.u32()?,
            p_cost: reader.u32()?,
        };
        kdf.validate_for_reading()
            .map_err(|_| VaultError::CorruptedHeader)?;
        let salt = reader.array::<SALT_LEN>()?;
        let nonce_prefix = reader.array::<NONCE_PREFIX_LEN>()?;
        let chunk_size = reader.u32()?;
        if chunk_size != CHUNK_SIZE {
            return Err(VaultError::CorruptedHeader);
        }
        let reserved = reader.array::<RESERVED_LEN>()?;
        if reserved.iter().any(|byte| *byte != 0) {
            return Err(VaultError::CorruptedHeader);
        }
        let tag = reader.array::<TAG_LEN>()?;
        Ok(Self {
            flags,
            file_uuid,
            kdf,
            salt,
            nonce_prefix,
            chunk_size,
            tag,
        })
    }
}

/// Bounds-checked byte reader: never panics, every overrun is an error.
struct Reader<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], VaultError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or(VaultError::CorruptedHeader)?;
        let slice = self
            .data
            .get(self.position..end)
            .ok_or(VaultError::CorruptedHeader)?;
        self.position = end;
        Ok(slice)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], VaultError> {
        <[u8; N]>::try_from(self.take(N)?).map_err(|_| VaultError::CorruptedHeader)
    }

    fn u8(&mut self) -> Result<u8, VaultError> {
        Ok(u8::from_le_bytes(self.array::<1>()?))
    }

    fn u16(&mut self) -> Result<u16, VaultError> {
        Ok(u16::from_le_bytes(self.array::<2>()?))
    }

    fn u32(&mut self) -> Result<u32, VaultError> {
        Ok(u32::from_le_bytes(self.array::<4>()?))
    }
}
