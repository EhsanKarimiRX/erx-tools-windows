//! Key derivation and the header commitment tag.
//!
//! SECURITY-CRITICAL MODULE. Pipeline:
//!   normalized passphrase --Argon2id(salt, params)--> 32-byte master key
//!   master key --HKDF-SHA256--> encryption key and commitment key
//!   commitment key --HMAC-SHA256 over the header body--> commitment tag
//! Every secret buffer is wrapped in `Zeroizing`.

use argon2::{Algorithm, Argon2, Params, Version};
use hkdf::Hkdf;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use zeroize::Zeroizing;

use super::error::VaultError;
use super::format::{SALT_LEN, TAG_LEN, UUID_LEN};

/// Length of every derived key in bytes.
pub const KEY_LEN: usize = 32;
/// Smallest Argon2 memory a NEW encryption may use (64 MiB).
pub const MIN_M_COST_KIB_FOR_ENCRYPTION: u32 = 64 * 1024;
/// Smallest Argon2 memory a file may declare when it is READ.
pub const MIN_M_COST_KIB_FOR_READING: u32 = 8 * 1024;
/// Largest Argon2 memory a file may declare (512 MiB). A damaged or
/// hostile header must not be able to request gigabytes.
pub const MAX_M_COST_KIB: u32 = 512 * 1024;
pub const MAX_T_COST: u32 = 10;
pub const MAX_P_COST: u32 = 8;

const LABEL_ENCRYPTION: &[u8] = b"ERX Vault v1 encryption key";
const LABEL_COMMITMENT: &[u8] = b"ERX Vault v1 commitment key";

/// Argon2id cost parameters. They are stored in every file header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfParams {
    /// Memory in KiB.
    pub m_cost_kib: u32,
    /// Number of passes.
    pub t_cost: u32,
    /// Parallel lanes.
    pub p_cost: u32,
}

impl KdfParams {
    /// Default until the one-time benchmark exists (plan step V5):
    /// 128 MiB, 3 passes, 1 lane.
    pub const DEFAULT: Self = Self {
        m_cost_kib: 128 * 1024,
        t_cost: 3,
        p_cost: 1,
    };

    /// Weak, fast parameters. Tests only; they can never reach a release
    /// build because `EncryptSettings::new` refuses them.
    #[cfg(test)]
    pub fn for_tests() -> Self {
        Self {
            m_cost_kib: MIN_M_COST_KIB_FOR_READING,
            t_cost: 1,
            p_cost: 1,
        }
    }

    /// Range check applied to every parameter set, including those read
    /// from a file header.
    pub fn validate_for_reading(&self) -> Result<(), VaultError> {
        let memory_ok = (MIN_M_COST_KIB_FOR_READING..=MAX_M_COST_KIB).contains(&self.m_cost_kib);
        let time_ok = (1..=MAX_T_COST).contains(&self.t_cost);
        let lanes_ok = (1..=MAX_P_COST).contains(&self.p_cost);
        if memory_ok && time_ok && lanes_ok {
            Ok(())
        } else {
            Err(VaultError::InvalidKdfParams)
        }
    }
}

/// The two keys derived from one passphrase. Wiped on drop.
pub struct VaultKeys {
    pub encryption: Zeroizing<[u8; KEY_LEN]>,
    pub commitment: Zeroizing<[u8; KEY_LEN]>,
}

/// HKDF info = fixed label followed by the file UUID (domain separation).
fn info(label: &[u8], file_uuid: &[u8; UUID_LEN]) -> Vec<u8> {
    let mut out = Vec::with_capacity(label.len() + UUID_LEN);
    out.extend_from_slice(label);
    out.extend_from_slice(file_uuid);
    out
}

/// Derives the encryption key and the commitment key.
///
/// `passphrase` MUST be the output of `normalize_passphrase`.
pub fn derive_keys(
    passphrase: &str,
    salt: &[u8; SALT_LEN],
    file_uuid: &[u8; UUID_LEN],
    params: &KdfParams,
) -> Result<VaultKeys, VaultError> {
    params.validate_for_reading()?;
    let argon_params = Params::new(
        params.m_cost_kib,
        params.t_cost,
        params.p_cost,
        Some(KEY_LEN),
    )
    .map_err(|_| VaultError::InvalidKdfParams)?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon_params);

    let mut master = Zeroizing::new([0u8; KEY_LEN]);
    argon
        .hash_password_into(passphrase.as_bytes(), salt, master.as_mut_slice())
        .map_err(|_| VaultError::KeyDerivationFailed)?;

    let hkdf = Hkdf::<Sha256>::new(None, master.as_slice());
    let mut encryption = Zeroizing::new([0u8; KEY_LEN]);
    let mut commitment = Zeroizing::new([0u8; KEY_LEN]);
    hkdf.expand(
        &info(LABEL_ENCRYPTION, file_uuid),
        encryption.as_mut_slice(),
    )
    .map_err(|_| VaultError::Internal)?;
    hkdf.expand(
        &info(LABEL_COMMITMENT, file_uuid),
        commitment.as_mut_slice(),
    )
    .map_err(|_| VaultError::Internal)?;
    Ok(VaultKeys {
        encryption,
        commitment,
    })
}

/// HMAC-SHA256 of the header body under the commitment key.
pub fn commitment_tag(
    commitment_key: &[u8; KEY_LEN],
    header_body: &[u8],
) -> Result<[u8; TAG_LEN], VaultError> {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(commitment_key.as_slice())
        .map_err(|_| VaultError::Internal)?;
    mac.update(header_body);
    let output = mac.finalize().into_bytes();
    let bytes: &[u8] = &output;
    <[u8; TAG_LEN]>::try_from(bytes).map_err(|_| VaultError::Internal)
}

/// Checks the stored tag in constant time. A mismatch means: wrong
/// passphrase OR a changed header (the two cannot be told apart).
pub fn verify_commitment(
    commitment_key: &[u8; KEY_LEN],
    header_body: &[u8],
    stored_tag: &[u8; TAG_LEN],
) -> Result<(), VaultError> {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(commitment_key.as_slice())
        .map_err(|_| VaultError::Internal)?;
    mac.update(header_body);
    mac.verify_slice(stored_tag.as_slice())
        .map_err(|_| VaultError::WrongPassphraseOrHeader)
}
