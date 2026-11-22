# ERX Secure Notes container format (version 1)

Work in progress for ERX Tools 0.5.0. This document describes the encrypted container only (header and encrypted body). The notes data inside it, the place of the file and how it is written to disk are added when they exist.

The source code in `src-tauri/src/tools/notes/` is the final truth (`format.rs`, `crypto.rs`, `error.rs`). All integers are little-endian. The format has not been audited by a security professional.

## 1. Overview

A notes file is a header followed by a nonce and one encrypted body:

    file      = header (192 bytes) | body nonce (24 bytes) | encrypted body
    plaintext = payload length (8 bytes) | payload | zero padding

The payload is the notes content as an opaque byte slice. The encrypted body is the plaintext plus a 16-byte Poly1305 tag. The whole file is padded to a multiple of 262,144 bytes (256 KiB), so its length only shows the size in 256 KiB steps. The smallest file is 262,144 bytes.

## 2. Header (192 bytes, never encrypted)

| Offset | Size | Field |
|---|---|---|
| 0 | 8 | magic `ERXNOTES` |
| 8 | 2 | format version (1) |
| 10 | 2 | flags, must be zero |
| 12 | 16 | file UUID (random) |
| 28 | 1 | KDF id (1 = Argon2id, version 0x13) |
| 29 | 4 | Argon2 memory cost in KiB |
| 33 | 4 | Argon2 time cost (passes) |
| 37 | 4 | Argon2 parallelism (lanes) |
| 41 | 16 | Argon2 salt (random) |
| 57 | 24 | nonce for the wrapped data key (random) |
| 81 | 48 | wrapped data key: 32 key bytes plus a 16-byte tag |
| 129 | 31 | reserved, must be zero |
| 160 | 32 | commitment tag, see section 3 |

Parsing is strict. The checks run in this order: length, magic, version, flags, KDF id, KDF parameters, reserved bytes. A file with a wrong length, a wrong KDF id or non-zero reserved bytes is damaged; a wrong magic means it is not a notes file; a different version or any flag bit is refused.

## 3. Keys

1. **Normalization.** The words are normalized by the Vault's `normalize_passphrase` function: Unicode NFKC, lower-case, NFKC again, split at whitespace and joined with single spaces. The notes crypto functions receive the already-normalized string; they do not check the word list.
2. **Argon2id** (version 0x13) turns the normalized words (UTF-8) and the salt from the header into a 32-byte master key. A new file created through `CreateSettings::new` refuses less than 64 MiB of memory. When reading, only 8 MiB to 512 MiB of memory, 1 to 10 passes and 1 to 8 lanes are accepted, so a damaged or hostile header cannot make the app allocate gigabytes. The core provides `tune_kdf`, which measures one pass with 256 MiB of memory and picks the number of passes for a 1-second target, never fewer than 2 and never more than 10. The parameters are stored in the header, so a file can be opened on another PC.
3. **HKDF-SHA256** (no salt) derives three 32-byte keys. The info input is a fixed label followed by the 16-byte file UUID: `ERX Notes v1 wrap key`, `ERX Notes v1 commitment key` and `ERX Notes v1 body key`.
4. **Random data key.** Every new file gets a random 32-byte data key. The body key is derived from this data key with the `ERX Notes v1 body key` label and the file UUID.
5. **Commitment tag** = HMAC-SHA256(commitment key, header bytes 0 to 160). The app checks it in constant time before it unwraps the data key. A wrong passphrase and a changed UUID, salt, wrap nonce, wrapped key or commitment tag give the same error, because the two cases cannot be told apart. Structural damage in the earlier header fields is reported as its own error first.
6. **Key wrapping.** The data key is encrypted with XChaCha20-Poly1305 under the wrap key. The nonce is the 24-byte wrap nonce from the header, the message is the 32-byte data key, and the associated data is header bytes 0 to 81. The result is the 48-byte wrapped key.

## 4. Body

- Cipher: **XChaCha20-Poly1305**.
- Every save uses a fresh random 24-byte body nonce. The nonce is stored in front of the encrypted body.
- The associated data is the whole 192-byte header, so changing any header byte breaks the body.
- The plaintext starts with the payload length as a 64-bit little-endian number, followed by the payload and then zeros up to the padded length. The padding is checked to be all zero when the file is opened.
- Argon2 runs only when a file is created or opened. Saving again with `NotesKeys::reseal` re-encrypts the plaintext with the same body key and a new body nonce; it does no Argon2 work and does not change the header.

## 5. Changing the words

Changing the words is not a separate operation. The caller creates a new file with the new words, which makes a new UUID, salt, data key and body key. An old file and the old words cannot open the new file.

## 6. Errors

| Condition | Error | Message |
|---|---|---|
| Wrong magic | `NotANotesFile` | This file is not an ERX Secure Notes file. |
| Version other than 1, or any flag bit set | `UnsupportedVersion` | This notes file needs a different version of ERX Tools. |
| Header length not 192, KDF id not 1, reserved bytes not zero, file too short, file length not a multiple of 262,144, or the wrapped key or body fails authentication | `DamagedFile` | The notes file is damaged. |
| Commitment tag mismatch: wrong words or a changed UUID, salt, wrap nonce, wrapped key or commitment tag | `WrongPassphraseOrHeader` | The words are wrong, or the notes file is damaged. |
| Argon2 parameters outside the allowed range | `InvalidKdfParams` | The notes file has invalid key settings. |
| Argon2 itself fails | `KeyDerivationFailed` | Could not derive the key from the words. |
| File larger than 67,371,008 bytes when opening | `FileTooLarge` | The notes file is too large. |
| Payload larger than 67,108,864 bytes when saving | `NotesTooLarge` | The notes are too large to save. |
| Windows random source fails | `RandomUnavailable` | Could not get secure random numbers from Windows. |
| Internal length check fails | `Internal` | Internal error in Secure Notes. |

A wrong passphrase and a changed header that passes the structural checks give the same error: `WrongPassphraseOrHeader`.

## 7. Limits

| Item | Value |
|---|---|
| Largest payload | 67,108,864 bytes (64 MiB) |
| Largest file | 67,371,008 bytes |
| Smallest file | 262,144 bytes |
| Padding bucket | 262,144 bytes (256 KiB) |
| Header | 192 bytes |
| Body nonce | 24 bytes |
| Length field | 8 bytes |
| Poly1305 tag | 16 bytes |

Every file length is a multiple of 262,144 bytes.

## 8. What an attacker with only the file can see

- That it is an ERX notes file, and the format version.
- The Argon2 settings, the salt and the file UUID.
- The wrap nonce, the wrapped data key and the commitment tag. These are needed to open the file but reveal nothing without the words.
- The file size in steps of 256 KiB, so the real payload size is only known roughly.
- The file date that the file system shows.

## 9. What the container does not protect against

- Malware, a keylogger or screen capture on the PC, and anyone who sees or gets the words.
- Memory written to disk by Windows (page file, hibernation) while the file is open.
- Loss of the words. There is no recovery and no reset.
- Mistakes in the code. The container is new and has had no professional security audit.

Strength depends on the words. The passphrase code accepts 5 to 12 words. With the 7776-word list a word is about 12.9 bits, so 5 words are about 64.6 bits and 8 words are about 103.4 bits.

## 10. Versioning

The version field is 1. A different version or any flag bit is refused with `UnsupportedVersion`. The 31 reserved header bytes must be zero; anything else is `DamagedFile`.

## 11. Tests

The tests in `format_test.rs` and `crypto_test.rs` check:

- The header offsets and sizes add up to 192 bytes, and a header survives a write and parse round trip.
- Strict parsing rejects wrong lengths, magic, versions, flags, KDF ids, reserved bytes and out-of-range KDF parameters, in the documented order.
- Padding rounds the file up to whole 256 KiB buckets, writes the little-endian length, and leaves zeros after the payload.
- Unpadding rejects short input, a length larger than the data, a length above the maximum and non-zero padding.
- Create and open return the same payload for empty, small, bucket-boundary, 1 MiB and Persian text with a half-space.
- Wrong words, different case and extra spaces give `WrongPassphraseOrHeader`.
- Every one of the 192 header bytes is flipped and must make `open` fail; the UUID, salt, wrap nonce, wrapped key and commitment tag ranges must give `WrongPassphraseOrHeader`.
- Body nonce and ciphertext changes, truncation, appending and a foreign header spliced onto a body are rejected.
- Size limits are enforced, reseal keeps the header and changes the nonce, and changing the words makes a new UUID, salt and body key.
- The KDF tuning formula picks the expected number of passes, and the `Debug` output of the key holder prints only `NotesKeys { .. }`.

The fixed SHA-256 hash test in `crypto_test.rs` only guards against accidental changes of the format. The hash was produced by this code, so it is not an independent proof. No known-answer vectors from an independent implementation exist yet.
