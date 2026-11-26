# ERX Secure Notes container format (version 1)

Work in progress for ERX Tools 0.5.1 (first written for 0.5.0). This document describes the encrypted container (header and encrypted body), the notes data (the payload) inside it, how the file is saved to disk, and the exported `.erxnote` files (section 12). The folder is chosen by the app.

The source code in `src-tauri/src/tools/notes/` is the final truth (`format.rs`, `crypto.rs`, `model.rs`, `search.rs`, `error.rs`). All integers are little-endian unless a section says otherwise. The format has not been audited by a security professional.

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
| 10 | 2 | flags: bit 0 (`0x0001`) is `FLAG_USER_PASSPHRASE`, every other bit must be zero; notebook files always have 0 (see section 12) |
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

Parsing is strict. The checks run in this order: length, magic, version, flags, KDF id, KDF parameters, reserved bytes. A file with a wrong length, a wrong KDF id or non-zero reserved bytes is damaged; a wrong magic means it is not a notes file; a different version or any unknown flag bit is refused.

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

Changing the words is not a separate operation. The caller creates a new file with the new words, which makes a new UUID, salt, data key and body key. An old file and the old words cannot open the new file. The file store wraps this as a passphrase change; see section 6.

## 6. Files and saving

The file store uses two names in one folder:

- `secure-notes.erxnotes` is the current notes file.
- `secure-notes.erxnotes.bak` is the backup.

A temporary file is named `secure-notes.erxnotes.<8 lower-case hex characters>.erxtmp` and is written in the same folder. The folder itself is chosen by the app; the store only receives it.

A save does these steps:

1. Write the new file to a new temporary name.
2. Flush and sync the temporary file to disk.
3. Read it back and compare it byte for byte with what was written.
4. Decrypt the body with the stored body key and compare the payload. This is a fast check: it uses no words and no Argon2.
5. Move the old main file to the backup name, replacing the old backup, and then move the temporary file to the main name.

For the first file (`create_new`) there is no old main file, so the backup is not touched. A rename or a remove that fails because Windows briefly keeps a file busy is retried up to five times, 100 ms apart.

For a normal save the backup is the previous version of the file: it has the same words and the same keys as the main file had, and it opens with the same words. When a passphrase change succeeds, the backup is a copy of the new file instead.

Loading tries the main file first. The backup is used when the main file is missing or damaged. A file made by a newer version is never replaced by the backup: `UnsupportedVersion` and `DataTooNew` are returned at once. Wrong words and a damaged header give the same error, so both files are tried. Loading never writes.

After a load from the backup the next save does not copy the damaged main file over the good backup. The main file is replaced and the backup is left alone; after one successful save the main file is good again.

A crash between the two moves leaves the backup as the last good version, so only the last edit can be lost. If the old main file was already moved and the new file was not moved in yet, the main file is missing and loading uses the backup. A temporary file left by a crash is removed at a later save once it is at least one hour old; only files with the temporary name pattern are touched.

A passphrase change writes a new file with the new words and new keys: a new UUID, salt, data key and body key. The new file is moved to the main name, and the backup is replaced by a copy of the same new file, so both files open with the new words. If writing the new backup fails, the store removes the old backup. If that fails too, the save reports `old_backup_gone = false`, because the old backup would still open with the old words.

What this does not do:

- A deleted note stays in the backup until the next save.
- Deleting a file does not wipe the disk blocks.
- `sync_all` depends on the disk and its driver.
- The file date and the file size (in 256 KiB steps) stay visible.
- A crash can lose the last edit.

### Resetting after forgotten words

There is no recovery and no password reset. To start over with an empty notebook, close the app completely and delete both `secure-notes.erxnotes` and `secure-notes.erxnotes.bak` (and any `secure-notes.erxnotes.<8 hex>.erxtmp`) from the notes folder. Deleting only the main file is not enough, because the store falls back to the backup. After the next start the Create flow runs again. This permanently deletes all notes; there is no way to get them back.

## 7. Notes data (the payload)

The payload is UTF-8 JSON. With default settings and one note it looks exactly like this (one line, no spaces added):

```json
{"version":1,"settings":{"preview":"titles","autoLockMinutes":10},"notes":[{"id":"0123456789abcdef0123456789abcdef","title":"t","body":"b","format":"text","created":1700000000,"modified":1700000001}]}
```

| Field | Type | Rule |
|---|---|---|
| `version` | number | Payload version, currently `1`. |
| `settings.preview` | string | `"titles"` or `"first-line"`. |
| `settings.autoLockMinutes` | number | Whole minutes, `1` to `240`; default `10`. |
| `notes[].id` | string | Exactly 32 lower-case hex characters (`0-9`, `a-f`), unique within the file. |
| `notes[].title` | string | At most 1024 bytes of UTF-8. |
| `notes[].body` | string | At most 4 MiB (4,194,304 bytes) of UTF-8. |
| `notes[].format` | string | Must be `"text"`. |
| `notes[].created` | number | Unix seconds, not negative. |
| `notes[].modified` | number | Unix seconds, not negative. |

The limits are 10,000 notes and 64 MiB (67,108,864 bytes) for the whole payload. A payload outside these limits is refused.

A file with a newer payload version is refused with the message that a newer ERX Tools is needed. Unknown fields are refused too, so a new field always comes with a new version number. Older files stay readable.

An empty title gets the creation date as its default title: this is done by the app, not by the file format.

Note text is stored exactly as typed: the half-space and Persian digits are not changed.

Search looks only at titles, never at bodies. The letters Arabic yeh and Arabic kaf match Persian yeh and Persian kaf, and case is ignored; Persian digits and Latin digits are not treated as equal.

While the notes are open, the text is kept in the memory of the app. When a note is removed or the notes are locked, the app overwrites the note's id, title and body text in place (zeroize). This is best effort only: copies made by the JSON parser, the allocator, or by Windows (page file, hibernation) are not controlled, and the `format` field and timestamps are not wiped.

## 8. Errors

| Condition | Error | Message |
|---|---|---|
Container and key errors (raised while opening or saving a file):

| Condition | Error | Message |
|---|---|---|
| Wrong magic | `NotANotesFile` | This file is not an ERX Secure Notes file. |
| Version other than 1, or an unknown flag bit set | `UnsupportedVersion` | This notes file needs a different version of ERX Tools. |
| A newer payload version than this code knows | `DataTooNew` | These notes were saved by a newer version of ERX Tools. |
| Header length not 192, KDF id not 1, reserved bytes not zero, file too short, file length not a multiple of 262,144, or the wrapped key or body fails authentication | `DamagedFile` | The notes file is damaged. |
| Commitment tag mismatch: wrong words or a changed UUID, salt, wrap nonce, wrapped key or commitment tag | `WrongPassphraseOrHeader` | The words are wrong, or the notes file is damaged. |
| Argon2 parameters outside the allowed range | `InvalidKdfParams` | The notes file has invalid key settings. |
| Argon2 itself fails | `KeyDerivationFailed` | Could not derive the key from the words. |
| File larger than 67,371,008 bytes when opening | `FileTooLarge` | The notes file is too large. |
| Payload larger than 67,108,864 bytes when saving | `NotesTooLarge` | The notes are too large to save. |
| Windows random source fails | `RandomUnavailable` | Could not get secure random numbers from Windows. |
| Internal length check fails | `Internal` | Internal error in Secure Notes. |
| Payload does not match the schema or limits | `DataInvalid` | The notes data is not valid. |

Data errors (raised while validating the payload):

| Condition | Error | Message |
|---|---|---|
| More than 10,000 notes | `TooManyNotes` | There are too many notes. |
| Note body larger than 4,194,304 bytes | `NoteTooLarge` | This note is too large. |
| Title larger than 1,024 bytes | `TitleTooLong` | The title is too long. |
| Requested note id not found | `NoteNotFound` | That note was not found. |

Store and session errors (raised by the file store and the open session):

| Condition | Error | Message |
|---|---|---|
| The notes file could not be read or written | `Io` | The notes file could not be read or written. |
| No notes file exists in the folder | `NoNotesFile` | No notes file was found. |
| A notes file already exists | `AlreadyExists` | A notes file already exists. |
| The saved file did not pass the read-back safety check | `VerifyFailed` | The saved notes did not pass the safety check, so they were not used. |
| The notes are locked | `NotUnlocked` | The notes are locked. |
| The notes are already unlocked | `AlreadyUnlocked` | The notes are already unlocked. |
| The words were not confirmed as saved | `WordsNotConfirmed` | Confirm that you saved the words first. |
| The words were not retyped correctly | `WordsNotRetyped` | Type the words again to confirm that you saved them. |
| A words-validation failure (count or list) | `Words(String)` | The message carried by the error. |

A wrong passphrase and a changed header that passes the structural checks give the same error: `WrongPassphraseOrHeader`.

## 9. Limits

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

## 10. What an attacker with only the file can see

- That it is an ERX notes file, and the format version.
- The Argon2 settings, the salt and the file UUID.
- The wrap nonce, the wrapped data key and the commitment tag. These are needed to open the file but reveal nothing without the words.
- The file size in steps of 256 KiB, so the real payload size is only known roughly.
- The file date that the file system shows.

## 11. What the container does not protect against

- Malware, a keylogger or screen capture on the PC, and anyone who sees or gets the words.
- Memory written to disk by Windows (page file, hibernation) while the file is open.
- Loss of the words. There is no recovery and no reset.
- Mistakes in the code. The container is new and has had no professional security audit.

Strength depends on the words. The passphrase code accepts 5 to 12 words. With the 7776-word list a word is about 12.9 bits, so 5 words are about 64.6 bits and 8 words are about 103.4 bits. An exported file is only as strong as the words it was made with (section 12).

## 12. Exported files (`.erxnote`)

An exported note or a backup of all notes is a normal notes container (sections 1 to 4) with its own keys. It is not the notebook file and shares no keys with it.

**Words and keys.** Every export gets its own random file UUID, Argon2 salt, data key and body key, so two exports of the same notes never share keys. The words come from one of two sources:

- new export words that the app generates (5 to 12 words; the user never types a custom set), or
- the current notebook words, which the user must type again. The app checks them against the notebook file before it uses them. The app does not keep the notebook words in memory after the notebook is unlocked, which is why they must be typed again.

The Argon2 settings are stored in the header, as for notebook files, so an export can be opened on another PC.

**Flag.** Exports made by 0.5.1 or later set bit 0 of the flags field (`FLAG_USER_PASSPHRASE`, `0x0001`). The flag is only a hint. The commitment tag covers header bytes 0 to 160, which include the flags, so changing the flag makes the file fail to open; it can never make a wrong passphrase succeed. Notebook files always write flags 0.

**Payload.** Two shapes, both UTF-8 JSON:

- A backup of all notes: the same JSON as section 7, including the settings.
- A single note: a JSON object `{"note": ...}` whose value has the fields of the note view: `id`, `title`, `body`, `created`, `modified`.

Reading tries the section 7 shape first. A payload from a newer version is refused with `DataTooNew`. Otherwise the single-note shape is read, and the note gets the format `text`.

**Opening (import).**

1. The header is parsed strictly (section 2).
2. If the flag bit is NOT set, the app tries the old built-in passphrase (see below). If the flag is set, it does not.
3. In 0.5.1 the notebook words are not tried automatically. A file that does not open with the built-in passphrase always asks the user for the words.
4. Typed words are normalized as in section 3 and are never tried against the built-in passphrase. Wrong words give no plaintext. The file stays waiting in memory so the user can try again. There is no attempt limit; every guess costs one Argon2 run (about 1 second), and the file is offline.
5. Import only adds notes. A note whose id already exists gets a new id; the timestamps are kept. If any note is invalid or the notebook would exceed its limits, nothing is added.
6. Size limits: a file larger than 67,371,008 bytes is `FileTooLarge`; a plain-text file larger than 4 MiB is `NoteTooLarge`.

**Files made by 0.5.0.** 0.5.0 encrypted every export with one fixed passphrase built into the program (`erx-notes-export-default`) and wrote flags 0. Anyone who has such a file can open it, so these files are effectively unprotected. 0.5.1 can still open them and tells the user to export again and delete the old file. The constant is kept in the code only for reading (`LEGACY_EXPORT_PASSPHRASE`); it is never used to write.

**0.5.0 cannot open 0.5.1 exports.** 0.5.0 refuses any flag bit, so a new export shows "needs a different version of ERX Tools" there.

**Limits.**

- Losing the export words loses the file. There is no recovery.
- Changing the notebook passphrase does not change older exports.
- An export made with the current notebook words is only as strong as those words (5 words are about 64.6 bits).
- Export words of 5 to 7 words are allowed, with a warning in the app.
- Plain-text exports (`txt`, `md`) are not encrypted at all.
- A crash during an export can leave a temporary `*.erxtmp` file next to the target.

## 13. Versioning

The version field is 1. A different version or any flag bit other than `FLAG_USER_PASSPHRASE` is refused with `UnsupportedVersion`. Version 1 stays: the flag bit was added in 0.5.1 without a new version number (see section 12). The 31 reserved header bytes must be zero; anything else is `DamagedFile`.

## 14. Tests
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
