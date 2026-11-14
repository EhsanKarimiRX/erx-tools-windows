//! The metadata block stored (encrypted) at the end of the stream.
//!
//! It records what the encrypted data IS, so the name of the .erx file does
//! not have to: the original file name (with its type) travels inside the
//! encryption and is restored when decrypting. The block has a FIXED size,
//! so the length of the name is hidden as well.
//!
//! Layout (1024 bytes, little-endian):
//!   0     1  kind (0 = one file, 1 = folder archive)
//!   1     2  length of the name in bytes
//!   3     n  the name, UTF-8
//!   3+n   -  zeros up to the end of the block
//!
//! SECURITY: the block is part of the last STREAM chunk, so it is encrypted
//! and authenticated like everything else. The name is chosen by whoever
//! encrypted the file, so it is UNTRUSTED when decrypting: it must go
//! through `names::safe_file_name` before it is used.

use super::error::VaultError;

/// Size of the metadata block in bytes.
pub const META_LEN: usize = 1024;
/// Kind value for a single file.
pub const KIND_FILE: u8 = 0;
/// Kind value for a folder (stored as an archive, see `archive`).
pub const KIND_FOLDER: u8 = 1;
const KIND_LEN: usize = 1;
const NAME_LEN_LEN: usize = 2;
const NAME_AT: usize = KIND_LEN + NAME_LEN_LEN;
/// Longest name that fits (a Windows name is at most 765 bytes in UTF-8).
pub const MAX_NAME_BYTES: usize = META_LEN - NAME_AT;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMeta {
    pub kind: u8,
    /// Original file name; may be empty.
    pub name: String,
}

impl FileMeta {
    /// Metadata for a single file with the given original name.
    pub fn file(name: &str) -> Result<Self, VaultError> {
        if name.len() > MAX_NAME_BYTES {
            return Err(VaultError::Internal);
        }
        Ok(Self {
            kind: KIND_FILE,
            name: name.to_owned(),
        })
    }

    /// Metadata for a folder with the given original name.
    pub fn folder(name: &str) -> Result<Self, VaultError> {
        let mut meta = Self::file(name)?;
        meta.kind = KIND_FOLDER;
        Ok(meta)
    }

    /// The fixed-size block.
    pub fn encode(&self) -> Result<[u8; META_LEN], VaultError> {
        let name = self.name.as_bytes();
        if name.len() > MAX_NAME_BYTES {
            return Err(VaultError::Internal);
        }
        let length = u16::try_from(name.len()).map_err(|_| VaultError::Internal)?;
        let mut out = Vec::with_capacity(META_LEN);
        out.push(self.kind);
        out.extend_from_slice(&length.to_le_bytes());
        out.extend_from_slice(name);
        out.resize(META_LEN, 0);
        <[u8; META_LEN]>::try_from(out).map_err(|_| VaultError::Internal)
    }

    /// Strict parser: unknown kinds, bad lengths, non-zero padding and
    /// invalid UTF-8 are all refused.
    pub fn decode(bytes: &[u8]) -> Result<Self, VaultError> {
        if bytes.len() != META_LEN {
            return Err(VaultError::CorruptedData);
        }
        let kind = bytes.first().copied().ok_or(VaultError::Internal)?;
        if kind != KIND_FILE && kind != KIND_FOLDER {
            return Err(VaultError::UnsupportedVersion);
        }
        let length_bytes = bytes.get(KIND_LEN..NAME_AT).ok_or(VaultError::Internal)?;
        let length = usize::from(u16::from_le_bytes(
            <[u8; 2]>::try_from(length_bytes).map_err(|_| VaultError::Internal)?,
        ));
        if length > MAX_NAME_BYTES {
            return Err(VaultError::CorruptedData);
        }
        let end = NAME_AT + length;
        let name_bytes = bytes.get(NAME_AT..end).ok_or(VaultError::CorruptedData)?;
        let padding = bytes.get(end..).ok_or(VaultError::CorruptedData)?;
        if padding.iter().any(|byte| *byte != 0) {
            return Err(VaultError::CorruptedData);
        }
        let name = std::str::from_utf8(name_bytes).map_err(|_| VaultError::CorruptedData)?;
        Ok(Self {
            kind,
            name: name.to_owned(),
        })
    }
}
