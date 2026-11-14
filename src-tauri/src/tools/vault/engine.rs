//! The encryption engine: one logical stream in, one logical stream out.
//! Splitting into parts, temp files and renaming live in `parts` / `files`.
//!
//! SECURITY-CRITICAL MODULE. Layout of the encrypted stream:
//!
//!   file       = header (144 bytes) | chunk 1 | chunk 2 | ... | last chunk
//!   plaintext  = payload | trailer
//!   payload    = one zstd frame (flag set) or the raw file bytes
//!   trailer    = metadata block (1024 bytes, see `meta`)
//!                | original length (u64 LE) | SHA-256 of the original (32)
//!
//! The plaintext is cut into 1 MiB pieces and each piece is sealed with
//! XChaCha20-Poly1305 inside the STREAM (BE32) construction. Every piece
//! except the last is exactly 1 MiB (+16 byte tag); the last piece is
//! 1..=1 MiB (+16). A full piece is only sealed once more data arrives, so
//! the last piece always exists and always ends with the trailer. The
//! header is the AAD of every piece. The STREAM last-block flag makes
//! truncation fail authentication. The original file name lives in the
//! metadata block, i.e. inside the encryption.

use std::io::{self, Read, Write};
use std::path::Path;

use aead_stream::{DecryptorBE32, EncryptorBE32, StreamBE32};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, Zeroizing};

use super::error::VaultError;
use super::format::FLAG_FOLDER;
use super::format::{
    CHUNK_SIZE, FLAG_ZSTD, HEADER_LEN, Header, NONCE_PREFIX_LEN, SALT_LEN, TAG_LEN, UUID_LEN,
};
use super::kdf::{
    KEY_LEN, KdfParams, MIN_M_COST_KIB_FOR_ENCRYPTION, commitment_tag, derive_keys,
    verify_commitment,
};
use super::meta::{FileMeta, KIND_FOLDER, META_LEN};
use super::passphrase::{normalize_passphrase, validate_typed_words};

type Cipher = XChaCha20Poly1305;
type StreamEncryptor = EncryptorBE32<Cipher>;
type StreamDecryptor = DecryptorBE32<Cipher>;
type StreamNonce = aead_stream::Nonce<Cipher, StreamBE32<Cipher>>;

/// Poly1305 tag appended to every sealed chunk.
const TAG_SIZE: usize = 16;
/// Integrity block: original length (8) + SHA-256 (32).
const DIGEST_BLOCK_LEN: usize = 40;
/// Everything after the payload: metadata block + integrity block.
const TRAILER_LEN: usize = META_LEN + DIGEST_BLOCK_LEN;
/// Bytes read from the source per step.
const READ_BUFFER_LEN: usize = 256 * 1024;
/// zstd compression level (decision: level 3).
const ZSTD_LEVEL: i32 = 3;

/// Extensions of formats that are already compressed. Compressing them
/// again wastes time, so they are stored without zstd. This only decides
/// SPEED; it has no security meaning.
const ALREADY_COMPRESSED: &[&str] = &[
    "zip", "7z", "rar", "gz", "bz2", "xz", "zst", "lz4", "cab", "docx", "xlsx", "pptx", "jar",
    "apk", "jpg", "jpeg", "png", "gif", "webp", "heic", "heif", "avif", "mp3", "aac", "flac",
    "ogg", "opus", "m4a", "mp4", "mkv", "avi", "mov", "webm", "wmv", "erx",
];

/// False for files that are almost certainly compressed already.
pub fn should_compress(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
        return true;
    };
    let lower = extension.to_ascii_lowercase();
    !ALREADY_COMPRESSED.contains(&lower.as_str())
}

/// Validated settings for one encryption.
#[derive(Debug, Clone, Copy)]
pub struct EncryptSettings {
    compress: bool,
    kdf: KdfParams,
}

impl EncryptSettings {
    /// Refuses Argon2 memory below 64 MiB and out-of-range values.
    pub fn new(compress: bool, kdf: KdfParams) -> Result<Self, VaultError> {
        if kdf.m_cost_kib < MIN_M_COST_KIB_FOR_ENCRYPTION {
            return Err(VaultError::InvalidKdfParams);
        }
        kdf.validate_for_reading()?;
        Ok(Self { compress, kdf })
    }

    /// Weak and fast settings for tests only.
    #[cfg(test)]
    pub fn for_tests(compress: bool) -> Self {
        Self {
            compress,
            kdf: KdfParams::for_tests(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncryptSummary {
    /// Bytes read from the source file.
    pub input_bytes: u64,
    /// Bytes written (header + all chunks).
    pub output_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecryptSummary {
    /// Bytes of the restored file.
    pub output_bytes: u64,
    /// The metadata block: the ORIGINAL file name chosen by whoever made
    /// the file. Untrusted: pass it through `names::safe_file_name`.
    pub meta: FileMeta,
}

fn internal_io() -> io::Error {
    io::Error::other(VaultError::Internal)
}

fn fill_random(buffer: &mut [u8]) -> Result<(), VaultError> {
    getrandom::fill(buffer).map_err(|_| VaultError::RandomUnavailable)
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

fn build_nonce(prefix: &[u8; NONCE_PREFIX_LEN]) -> Result<StreamNonce, VaultError> {
    let mut nonce = StreamNonce::default();
    if nonce.len() != NONCE_PREFIX_LEN {
        return Err(VaultError::Internal);
    }
    for (target, source) in nonce.iter_mut().zip(prefix.iter()) {
        *target = *source;
    }
    Ok(nonce)
}

/// Collects plaintext into 1 MiB chunks, seals and writes them.
struct ChunkSink<'a, W: Write> {
    out: &'a mut W,
    encryptor: Option<StreamEncryptor>,
    aad: &'a [u8],
    buffer: Zeroizing<Vec<u8>>,
    chunk_size: usize,
    written: u64,
}

impl<'a, W: Write> ChunkSink<'a, W> {
    fn new(out: &'a mut W, encryptor: StreamEncryptor, aad: &'a [u8]) -> Self {
        let chunk_size = CHUNK_SIZE as usize;
        Self {
            out,
            encryptor: Some(encryptor),
            aad,
            buffer: Zeroizing::new(Vec::with_capacity(chunk_size + TAG_SIZE)),
            chunk_size,
            written: 0,
        }
    }

    /// Seals the full buffer as a NON-last chunk and writes it.
    fn flush_full_chunk(&mut self) -> io::Result<()> {
        let encryptor = self.encryptor.as_mut().ok_or_else(internal_io)?;
        let buffer: &mut Vec<u8> = &mut self.buffer;
        encryptor
            .encrypt_next_in_place(self.aad, &mut *buffer)
            .map_err(|_| internal_io())?;
        self.out.write_all(buffer)?;
        self.written = self.written.saturating_add(buffer.len() as u64);
        buffer.clear();
        Ok(())
    }

    /// Seals what is left as the LAST chunk, writes it, returns the number
    /// of chunk bytes written in total.
    fn finish(mut self) -> Result<u64, VaultError> {
        let encryptor = self.encryptor.take().ok_or(VaultError::Internal)?;
        let buffer: &mut Vec<u8> = &mut self.buffer;
        encryptor
            .encrypt_last_in_place(self.aad, &mut *buffer)
            .map_err(|_| VaultError::Internal)?;
        self.out.write_all(buffer)?;
        self.out.flush()?;
        Ok(self.written.saturating_add(buffer.len() as u64))
    }
}

impl<W: Write> Write for ChunkSink<'_, W> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let mut rest = data;
        while !rest.is_empty() {
            // A full chunk is sealed only when more data arrives, so the
            // last chunk is never empty and always holds the trailer.
            if self.buffer.len() >= self.chunk_size {
                self.flush_full_chunk()?;
            }
            let room = self.chunk_size.saturating_sub(self.buffer.len());
            let take = room.min(rest.len());
            let Some((head, tail)) = rest.split_at_checked(take) else {
                return Err(internal_io());
            };
            self.buffer.extend_from_slice(head);
            rest = tail;
        }
        Ok(data.len())
    }

    /// Never emits a partial chunk (chunk boundaries are part of the
    /// format).
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Reads the source to the end, hashing the original bytes and pushing
/// them into `sink`. Returns the number of bytes read.
fn pump<R: Read, S: Write>(
    source: &mut R,
    sink: &mut S,
    hasher: &mut Sha256,
    progress: &mut dyn FnMut(u64) -> bool,
) -> Result<u64, VaultError> {
    let mut buffer = Zeroizing::new(vec![0u8; READ_BUFFER_LEN]);
    let mut total: u64 = 0;
    loop {
        let read = loop {
            match source.read(&mut buffer) {
                Ok(count) => break count,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error.into()),
            }
        };
        if read == 0 {
            break;
        }
        let Some(filled) = buffer.get(..read) else {
            return Err(VaultError::Internal);
        };
        hasher.update(filled);
        sink.write_all(filled)?;
        total = total.saturating_add(read as u64);
        if !progress(total) {
            return Err(VaultError::Cancelled);
        }
    }
    Ok(total)
}

/// Encrypts `source` into `output` and stores `original_name` (may be
/// empty) inside the encryption.
///
/// `passphrase` is normalized and validated here (it must consist of
/// words from the built-in list). `progress` gets the number of source
/// bytes read so far; returning false cancels with `Cancelled`. On any
/// error the caller must delete the incomplete output.
pub fn encrypt_stream_meta<R: Read, W: Write>(
    source: &mut R,
    output: &mut W,
    passphrase: &str,
    settings: &EncryptSettings,
    meta: &FileMeta,
    progress: &mut dyn FnMut(u64) -> bool,
) -> Result<EncryptSummary, VaultError> {
    let normalized = normalize_passphrase(passphrase);
    validate_typed_words(&normalized)?;
    let meta_block = meta.encode()?;

    let mut file_uuid = [0u8; UUID_LEN];
    let mut salt = [0u8; SALT_LEN];
    let mut nonce_prefix = [0u8; NONCE_PREFIX_LEN];
    fill_random(&mut file_uuid)?;
    fill_random(&mut salt)?;
    fill_random(&mut nonce_prefix)?;

    let mut header = Header {
        flags: header_flags(settings.compress, meta.kind),
        file_uuid,
        kdf: settings.kdf,
        salt,
        nonce_prefix,
        chunk_size: CHUNK_SIZE,
        tag: [0u8; TAG_LEN],
    };
    let keys = derive_keys(&normalized, &header.salt, &header.file_uuid, &header.kdf)?;
    header.tag = commitment_tag(&keys.commitment, &header.body_bytes())?;
    let header_bytes = header.to_bytes();
    output.write_all(&header_bytes)?;

    let cipher = build_cipher(&keys.encryption);
    let nonce = build_nonce(&header.nonce_prefix)?;
    let encryptor = StreamEncryptor::from_aead(cipher, &nonce);
    let sink = ChunkSink::new(output, encryptor, &header_bytes);

    let mut hasher = Sha256::new();
    let (total, mut sink) = if settings.compress {
        let mut encoder = zstd::stream::write::Encoder::new(sink, ZSTD_LEVEL)?;
        let total = pump(source, &mut encoder, &mut hasher, progress)?;
        (total, encoder.finish()?)
    } else {
        let mut sink = sink;
        let total = pump(source, &mut sink, &mut hasher, progress)?;
        (total, sink)
    };

    let digest = hasher.finalize();
    let mut trailer = Vec::with_capacity(TRAILER_LEN);
    trailer.extend_from_slice(&meta_block);
    trailer.extend_from_slice(&total.to_le_bytes());
    trailer.extend_from_slice(&digest);
    sink.write_all(&trailer)?;
    let chunk_bytes = sink.finish()?;

    Ok(EncryptSummary {
        input_bytes: total,
        output_bytes: (HEADER_LEN as u64).saturating_add(chunk_bytes),
    })
}

/// Encrypts a single file stream and stores its original name (may be
/// empty) inside the encryption.
pub fn encrypt_stream_named<R: Read, W: Write>(
    source: &mut R,
    output: &mut W,
    passphrase: &str,
    settings: &EncryptSettings,
    original_name: &str,
    progress: &mut dyn FnMut(u64) -> bool,
) -> Result<EncryptSummary, VaultError> {
    let meta = FileMeta::file(original_name)?;
    encrypt_stream_meta(source, output, passphrase, settings, &meta, progress)
}

/// Header flags for the compression setting and the kind of data.
fn header_flags(compress: bool, kind: u8) -> u16 {
    let mut flags = 0;
    if compress {
        flags |= FLAG_ZSTD;
    }
    if kind == KIND_FOLDER {
        flags |= FLAG_FOLDER;
    }
    flags
}

/// Same as `encrypt_stream_named` with an empty original name.
pub fn encrypt_stream<R: Read, W: Write>(
    source: &mut R,
    output: &mut W,
    passphrase: &str,
    settings: &EncryptSettings,
    progress: &mut dyn FnMut(u64) -> bool,
) -> Result<EncryptSummary, VaultError> {
    encrypt_stream_named(source, output, passphrase, settings, "", progress)
}

/// Hashes and counts everything written through it (the restored file).
struct HashingWriter<'a, W: Write> {
    inner: &'a mut W,
    hasher: Sha256,
    count: u64,
}

impl<W: Write> Write for HashingWriter<'_, W> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(data)?;
        if let Some(done) = data.get(..written) {
            self.hasher.update(done);
        }
        self.count = self.count.saturating_add(written as u64);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Where decrypted payload bytes go: straight to the output, or through
/// the zstd decoder first.
enum Sink<'a, W: Write> {
    Plain(HashingWriter<'a, W>),
    Zstd(Box<zstd::stream::write::Decoder<'static, HashingWriter<'a, W>>>),
}

impl<'a, W: Write> Sink<'a, W> {
    fn push(&mut self, data: &[u8]) -> io::Result<()> {
        match self {
            Sink::Plain(writer) => writer.write_all(data),
            Sink::Zstd(decoder) => decoder.write_all(data),
        }
    }

    fn finish(self) -> io::Result<HashingWriter<'a, W>> {
        match self {
            Sink::Plain(writer) => Ok(writer),
            Sink::Zstd(mut decoder) => {
                decoder.flush()?;
                Ok((*decoder).into_inner())
            }
        }
    }
}

/// Adds decrypted plaintext to the tail buffer and releases everything
/// except the last `TRAILER_LEN` bytes (the trailer) to the sink.
fn feed<W: Write>(
    tail: &mut Vec<u8>,
    plain: &[u8],
    sink: &mut Sink<'_, W>,
) -> Result<(), VaultError> {
    tail.extend_from_slice(plain);
    if let Some(release) = tail.len().checked_sub(TRAILER_LEN)
        && release > 0
    {
        let Some(head) = tail.get(..release) else {
            return Err(VaultError::Internal);
        };
        sink.push(head)?;
        drop(tail.drain(..release));
    }
    Ok(())
}

/// Reads up to `size` bytes (fewer only at the end of the input).
fn read_block<R: Read>(input: &mut R, block: &mut Vec<u8>, size: usize) -> Result<(), VaultError> {
    block.clear();
    input.by_ref().take(size as u64).read_to_end(block)?;
    Ok(())
}

/// Decrypts `input` into `output`.
///
/// `typed_passphrase` is what the user typed; it is normalized and checked
/// against the word list here. `progress` gets the number of input bytes
/// consumed so far; returning false cancels with `Cancelled`. On any
/// error the caller must delete the incomplete output. The summary carries
/// the (untrusted) original file name.
pub fn decrypt_stream<R: Read, W: Write>(
    input: &mut R,
    output: &mut W,
    typed_passphrase: &str,
    progress: &mut dyn FnMut(u64) -> bool,
) -> Result<DecryptSummary, VaultError> {
    let mut head = Vec::with_capacity(HEADER_LEN);
    input
        .by_ref()
        .take(HEADER_LEN as u64)
        .read_to_end(&mut head)?;
    let header = Header::parse(&head)?;

    let normalized = normalize_passphrase(typed_passphrase);
    validate_typed_words(&normalized)?;
    let keys = derive_keys(&normalized, &header.salt, &header.file_uuid, &header.kdf)?;
    // SECURITY: wrong passphrase or changed header is detected HERE, before
    // any chunk is touched.
    verify_commitment(&keys.commitment, &header.body_bytes(), &header.tag)?;

    let cipher = build_cipher(&keys.encryption);
    let nonce = build_nonce(&header.nonce_prefix)?;
    let mut decryptor: Option<StreamDecryptor> = Some(StreamDecryptor::from_aead(cipher, &nonce));

    let hashing = HashingWriter {
        inner: output,
        hasher: Sha256::new(),
        count: 0,
    };
    let mut sink = if header.is_compressed() {
        Sink::Zstd(Box::new(zstd::stream::write::Decoder::new(hashing)?))
    } else {
        Sink::Plain(hashing)
    };

    let full_block = CHUNK_SIZE as usize + TAG_SIZE;
    let mut current: Zeroizing<Vec<u8>> = Zeroizing::new(Vec::with_capacity(full_block));
    let mut ahead: Zeroizing<Vec<u8>> = Zeroizing::new(Vec::with_capacity(full_block));
    let mut tail: Zeroizing<Vec<u8>> = Zeroizing::new(Vec::with_capacity(full_block + TRAILER_LEN));
    let mut consumed = HEADER_LEN as u64;

    read_block(input, &mut current, full_block)?;
    consumed = consumed.saturating_add(current.len() as u64);
    loop {
        if current.len() < TAG_SIZE {
            return Err(VaultError::CorruptedData);
        }
        // A full block is the last one only if nothing follows it.
        let is_last = if current.len() == full_block {
            read_block(input, &mut ahead, full_block)?;
            consumed = consumed.saturating_add(ahead.len() as u64);
            ahead.is_empty()
        } else {
            true
        };
        if is_last {
            let last = decryptor.take().ok_or(VaultError::Internal)?;
            let buffer: &mut Vec<u8> = &mut current;
            last.decrypt_last_in_place(&head, &mut *buffer)
                .map_err(|_| VaultError::CorruptedData)?;
            feed(&mut tail, buffer, &mut sink)?;
            break;
        }
        {
            let next = decryptor.as_mut().ok_or(VaultError::Internal)?;
            let buffer: &mut Vec<u8> = &mut current;
            next.decrypt_next_in_place(&head, &mut *buffer)
                .map_err(|_| VaultError::CorruptedData)?;
            feed(&mut tail, buffer, &mut sink)?;
        }
        if !progress(consumed) {
            return Err(VaultError::Cancelled);
        }
        std::mem::swap(&mut current, &mut ahead);
    }

    if tail.len() != TRAILER_LEN {
        return Err(VaultError::CorruptedData);
    }
    let writer = sink.finish()?;
    let (meta_bytes, digest_block) = tail
        .split_at_checked(META_LEN)
        .ok_or(VaultError::Internal)?;
    let (length_bytes, hash_bytes) = digest_block
        .split_at_checked(8)
        .ok_or(VaultError::Internal)?;
    let expected_length =
        u64::from_le_bytes(<[u8; 8]>::try_from(length_bytes).map_err(|_| VaultError::Internal)?);
    let HashingWriter {
        inner,
        hasher,
        count,
    } = writer;
    let actual = hasher.finalize();
    let actual_bytes: &[u8] = &actual;
    if count != expected_length || actual_bytes != hash_bytes {
        return Err(VaultError::IntegrityMismatch);
    }
    let meta = FileMeta::decode(meta_bytes)?;
    inner.flush()?;
    Ok(DecryptSummary {
        output_bytes: count,
        meta,
    })
}
