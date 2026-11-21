# ERX Vault file format (version 1) and threat model

This document describes the `.erx` files made by the Quantum Resistance Vault of ERX Tools 0.3.0 and later. The source code in `src-tauri/src/tools/vault/` is the final truth (`format.rs`, `kdf.rs`, `engine.rs`, `meta.rs`, `parts.rs`, `archive.rs`). All integers are little-endian unless noted. The format has not been audited by a security professional.

## 1. Overview

An encrypted file is a header followed by encrypted chunks:

    file      = header (144 bytes) | chunk 1 | chunk 2 | ... | last chunk
    plaintext = payload | trailer

The payload is the original file (or the folder archive, see section 6), optionally compressed with Zstandard. The trailer is 1064 bytes (section 5). The plaintext is cut into 1 MiB pieces and each piece is sealed on its own.

## 2. Header (144 bytes, never encrypted)

| Offset | Size | Field |
|---|---|---|
| 0 | 8 | magic `ERXVAULT` |
| 8 | 2 | format version (1) |
| 10 | 2 | flags: bit 0 = payload is a zstd frame, bit 1 = the data is a folder archive |
| 12 | 16 | file UUID (random) |
| 28 | 1 | KDF id (1 = Argon2id, version 0x13) |
| 29 | 4 | Argon2 memory cost in KiB |
| 33 | 4 | Argon2 time cost (passes) |
| 37 | 4 | Argon2 parallelism (lanes) |
| 41 | 16 | Argon2 salt (random) |
| 57 | 19 | STREAM nonce prefix (random) |
| 76 | 4 | plaintext chunk size (1,048,576) |
| 80 | 32 | reserved, must be zero (room for a future key slot) |
| 112 | 32 | commitment tag, see section 3 |

Parsing is strict. The app refuses a file with a wrong magic, an unknown version, unknown flag bits, non-zero reserved bytes, a chunk size other than 1 MiB, or KDF parameters outside the allowed range.

## 3. Keys

1. **Normalization.** The typed words are converted with Unicode NFKC, lower-cased, converted with NFKC again, split at whitespace and joined with single spaces. Encryption and decryption use the same function, so the same words always give the same bytes.
2. **Argon2id** (version 0x13) turns the normalized words (UTF-8) and the salt from the header into a 32-byte master key. The app uses 256 MiB of memory. On the first encryption of a run it measures the PC and picks the number of passes so that one derivation takes about 0.7 seconds, never fewer than 2. The parameters are stored in the header, so any PC can decrypt. A new encryption refuses less than 64 MiB. When reading, the app accepts only 8 MiB to 512 MiB of memory, 1 to 10 passes and 1 to 8 lanes, so a damaged or hostile header cannot make it allocate gigabytes.
3. **HKDF-SHA256** (no salt) expands the master key into two 32-byte keys. The info input is a fixed label followed by the file UUID: `ERX Vault v1 encryption key` for the encryption key and `ERX Vault v1 commitment key` for the commitment key.
4. **Commitment tag** = HMAC-SHA256(commitment key, header bytes 0 to 111). The app checks it in constant time before it decrypts any chunk. A wrong passphrase and a changed header give the same error; the two cannot be told apart.

## 4. The encrypted stream

- Cipher: **XChaCha20-Poly1305** inside the STREAM construction (big-endian counter variant, crate `aead-stream`).
- Nonce (24 bytes) = the 19-byte nonce prefix from the header + a 4-byte big-endian chunk counter + a 1-byte last-block flag.
- The whole 144-byte header is the associated data of every chunk, so changing any header byte breaks every chunk.
- Every chunk except the last holds exactly 1 MiB of plaintext (plus a 16-byte Poly1305 tag). The last chunk holds 1 byte to 1 MiB and carries the last-block flag, so cutting the file short fails authentication, and so does reordering chunks.
- The payload is one Zstandard frame (level 3) when flag bit 0 is set, otherwise the raw bytes. Files that are almost certainly compressed already (for example zip, jpg, mp4, docx) are stored without compression; this only decides speed.

## 5. Trailer (1064 bytes, at the end of the plaintext, so inside the last chunk)

| Size | Field |
|---|---|
| 1024 | metadata block (below) |
| 8 | length of the original data in bytes |
| 32 | SHA-256 of the original data |

After decrypting, the app compares the length and the SHA-256 with the restored data. A mismatch is reported as a failed integrity check.

Metadata block (1024 bytes, fixed size so the length of the name is hidden): 1 byte kind (0 = one file, 1 = folder archive), 2 bytes name length, the original name in UTF-8 (at most 1021 bytes), then zeros. The name is chosen by whoever encrypted the file, so it is untrusted when decrypting: it is cleaned into a safe Windows file name before it is used.

## 6. Folders

A folder is turned into one byte stream and then encrypted like a file (flag bit 1 set). The stream is a list of entries:

    directory entry : tag 1, path length (2 bytes), path
    file entry      : tag 2, path length (2 bytes), path, size (8 bytes), content
    end             : tag 0 (nothing may follow)

Paths are UTF-8, relative to the folder, with `/` between the parts, sorted by name; a directory always comes before its content. Empty folders and empty files are kept. **File dates, attributes and permissions are not stored.** Links, devices and other special items are refused. Limits: 500,000 items, 64 nested levels, 2048 bytes per path.

When a folder is restored, the stream is treated as untrusted: every part of every path must be a plain Windows file name, the parent of every entry must already exist, nothing is overwritten, links are never created, and nothing can be written outside the target folder.

## 7. Split files

When splitting is on, the encrypted stream is written to files named `name.001.erx`, `name.002.erx`, and so on (at most 999), with parts of 500 MiB, 1 GiB or 2 GiB (all below the 4 GiB limit of FAT32). Without splitting the single file is `name.erx`. Every part starts with a 28-byte part header: magic `ERXPART1`, the 16-byte file UUID, and the part number (4 bytes). The rest of the part is a slice of the stream; a chunk may span two parts.

Older versions (up to 0.4.1) named the parts `name.erx.001`, `name.erx.002`. ERX Tools 0.4.2 and later neither write nor read that style. The format itself did not change, so an old set can be opened by renaming each part by hand (`name.erx.001` becomes `name.001.erx`, `name.erx.002` becomes `name.002.erx`, and so on) and selecting `name.001.erx`. If an old-style first part is selected, the app shows a message that explains this.

**How the app finds the parts.** The user selects the first part. If the selected name ends in `.NNN.erx` (three digits), the app looks in the same folder for the next parts under `name.002.erx`, `name.003.erx` and so on. The digits in a name never decide anything alone, because a normal unsplit file can also be called `scan.001.erx` (made from `scan.001.jpg`). The part header decides: the selected file must have part number 1, and a later part counts only if its header has the same file UUID and the expected part number. A file with another UUID, or a file without a part header, is treated as if it did not exist, so a neighbouring file with a similar name does not disturb the set.

The part header is not a security feature. It only lets the app give a friendly message for a wrong, missing or reordered part. The real protection is the authentication of the stream: a missing, swapped or changed part makes decryption fail. If the last part is missing, the app cannot know that more data was expected and reports a damaged file instead of a missing part.

## 8. Safe writing

- The original file or folder is never changed or deleted. An existing file is never overwritten; a number is added to the name instead.
- The output is first written under a temporary name (`<final name>.<random token>.erxtmp`).
- After writing, the app reads the new file back and decrypts it into nothing to check it (verify-after-writing). Only then are the temporary files renamed to their final names. If the check fails, the new file is removed.
- Cancel removes the temporary files. Temporary files left by a crash or End task are removed when the next encryption or decryption starts in the same output folder, if they are at least one hour old.
- The app names the output after the original file without its type (for example `photo.jpg` becomes `photo.erx`). The real name is stored inside the encryption and comes back when you decrypt.

## 9. Passphrase

- Words come from the EFF Large Wordlist (7776 words, CC BY 3.0 US, dice numbers removed). Each word is worth about 12.9 bits.
- The words are drawn with Windows randomness and rejection sampling (no modulo bias). All words in one passphrase are different. A word position can be replaced at most 3 times; a replaced word is not shown again in the same session. After the user confirms that the words are saved, the passphrase is frozen.
- The words are created and kept in the Rust code. The web page receives them only to display them and never sends them back for encryption.
- Keys and passphrase buffers are wiped from memory when they are no longer needed.

| Words | Entropy (about) |
|---|---|
| 6 (minimum) | 77.6 bits |
| 8 | 103.4 bits |
| 10 (default) | 129.2 bits |
| 12 (maximum) | 155.1 bits |

The app shows a warning for fewer than 8 words.

## 10. Threat model

**What it is designed to protect against:** someone who gets the `.erx` file or files but not the words. Such a person should not be able to read the contents, the original names, the file types or the folder structure, and should not be able to change, shorten, reorder or replace parts of the data without the app noticing. A damaged or hostile file should not make the app allocate huge memory or write outside the chosen folder.

**What an encrypted file still shows:** that it is an ERX file; the format version; whether the data is compressed and whether it is a folder; the Argon2 settings, the salt and the file UUID; its approximate size (and, for compressible data, how well it compressed); the number of parts; and the file name and dates that the file system shows for the `.erx` file itself.

**What it does not protect against:**

- Malware, a keylogger or screen capture on your PC, and anyone who sees or gets your words. While the Vault is open the words are in the app's memory, and Windows may write memory to disk (page file, hibernation); ERX Tools cannot control that.
- The original file or folder, which stays on your disk after encrypting. A normal delete is not a secure erase.
- Loss of the words. There is no recovery, no key slot and no reset.
- Mistakes in the code. The Vault is new and has had no professional security audit.

**Quantum computers.** The design uses only symmetric cryptography and hash functions (no RSA, no elliptic curves). Against a quantum computer the main known attack on such designs is Grover's algorithm, which searches in about the square root of the classical effort. The 256-bit key is therefore not the limit; the entropy of the passphrase is. With the default 10 words (about 129 bits) a Grover search would still need about 2^64 steps, and every guess needs one memory-hungry Argon2id run. This is a design goal, not a proof: nobody can promise that encryption is unbreakable.

## 11. Versioning

The version field is 1. A file with a higher version is refused with the message that a newer ERX Tools is needed. The 32 reserved bytes must be zero in version 1 and leave room for a future key slot.
