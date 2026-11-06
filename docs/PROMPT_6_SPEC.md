# PROMPT 6 — HEIC / HEIF INPUT FOR THE IMAGE CONVERTER
## ERX Tools for Windows · steps 6a, 6b, 6c, 6d

> **STATUS: READY TO BUILD.** The investigation is finished (its facts are in section 1). All design decisions are made (section 3). Follow the steps in order. Stop after each lettered step.

---

## 0. READ THIS FIRST — resume protocol

The owner may hit a usage limit at any moment and continue the next day, so the work MUST be resumable.

**At the start of every session:**
1. Check whether `docs/PROMPT_6_PROGRESS.md` exists.
   - **Exists** → read it and `docs/PROMPT_6_DECISIONS.md`, run `git status` and `git log -5 --oneline`, and continue from the **first step not marked DONE**. If a step is IN PROGRESS, inspect `git status` / `git diff` to see what already exists and finish it. Never redo DONE steps.
   - **Does not exist** → fresh start: begin with step 6a.
2. Re-read section 2 (Rules) every session.
3. After EVERY micro-step (6a-1, 6a-2, …) update `docs/PROMPT_6_PROGRESS.md` before continuing.
4. When a lettered step is finished: all gates green → local commit → report (format in Appendix B) → **STOP and wait** for the owner to say "continue".

The owner is a non-programmer. He verifies by hand on his Windows 11 machine and sends you results.

---

## 1. Facts from the investigation (already verified — do not re-investigate)

Project: **ERX Tools for Windows** — Tauri 2, Rust backend (`windows-sys 0.61.2`, `image 0.25.10`), React 19 + TypeScript frontend. Fully OFFLINE. Repo at `C:\dev\erxtools`. Baseline: `cargo test` = **337** passing, all gates green, last commit is the Organize work (`a1d95c2` or later — check `git log`).

Image Converter locations (line numbers are approximate; re-find them yourself):
- Backend folder `src-tauri/src/tools/image_convert/`: `core.rs`, `convert.rs`, `commands.rs`, `naming.rs`, `pipeline.rs`.
  - `core.rs` ~26–33: `enum InputFormat` (JPEG, PNG, WebP, BMP, GIF, TIFF); ~62–72: mapping from the `image` crate format; ~103–196: `inspect_path` (opens the file, sniffs the REAL format from content with `image::ImageReader::with_guessed_format()`, never trusts the extension; returns `Inspection::Supported{..}` or `Inspection::NotSupported{ detected }`); ~199–212: `friendly_format_name` (AVIF is sniffed and reported as "AVIF is not supported in v1"); constants: 256 MB max input file size (~line 13) and 80 MP decompression-bomb guard (~line 18).
  - `convert.rs` ~45–59: the decode branch per input format; ~103–107: EXIF orientation is applied for supported formats (`decoder.orientation()` + `apply_orientation`); ~112–131: `prepare()` (JPEG/BMP flatten transparency onto a background color, PNG/WebP/TIFF keep alpha); ~148–163: encoders (RGB8/RGBA8 pixel buffers); ICC profiles and all metadata are dropped by design.
  - `commands.rs` ~21: `DIALOG_EXTENSIONS` (picker filter: jpg, jpeg, png, webp, bmp, gif, tif, tiff); ~74–88: `inspect_one` maps `NotSupported` to `kind: "unsupported"`, `reason: "not a recognized image"`; ~240–323: batch worker pool (max 4 threads, results streamed in list order, cancel supported).
- Frontend folder `src/tools/image-converter/`: `types.ts` ~5: `SUPPORTED_INPUT = "JPEG, PNG, WebP, BMP, GIF, TIFF"`; `index.tsx` ~78–81, 110–114, 247, 271 (rejection messages and drop-zone hints: "Not added: <name> — <reason>" and "Supported formats: …"); `OptionsPanel.tsx` ~123–126 (honest note: "Outputs contain no metadata (EXIF and GPS are removed). Camera rotation is applied automatically. Originals are never modified.").
- Today a dropped `.heic` file is honestly rejected with "not a recognized image": the `image` crate cannot sniff or decode HEIF at all.
- The owner's PC has Microsoft's **HEIF Image Extension 1.2.62.0** and **HEVC Video Extension 2.5.48.0** (x64, Store-signed), so Windows' own codec (WIC) can decode HEIC here. Stock Windows does NOT include them by default.
- Only HEIC file on this machine: a crate test fixture `~/.cargo/registry/src/index.crates.io-*/kamadak-exif-0.6.2/tests/exif.heic` (not a personal photo). Use it for the first smoke tests.
- Existing converter tests: 29, all generated in code (no image fixtures). No frontend test tooling exists (do not add any).

---

## 2. Rules (apply to ALL steps)

- **R1 Never `git push`.** The owner pushes personally. Local commits only, style as in `git log` (e.g. `Prompt 6a: HEIC sniff + WIC decoder`).
- **R2 Honest UI.** No silent failures, no unconditional claim of HEIC support, error messages in plain English that say what the user can do.
- **R3 Offline.** The app makes no network calls. Do not loosen the CSP. Do not add Tauri permissions/capabilities (the work is Rust-side only). Any new dependency, feature, or permission must be listed in your report.
- **R4 Nothing destructive.** Originals are never modified. Never overwrite output files (existing naming code already guarantees this — do not bypass it).
- **R5 Windows 10 AND 11, x64 AND ARM64.** Run the same ARM64 check used in earlier prompts (see `docs/`, `package.json`, or `git log`) at the end of every step.
- **R6 English only** in UI, errors and comments.
- **R7 Do not invent facts.** Anything marked **[VERIFY]** must be checked on the machine (compile, run, observe). HRESULT numbers in this prompt are hints, not truth: take names from the `windows` crate constants and confirm them by running real failing cases (6a-6).
- **R8 Protect the owner's data.** Never commit any photo. Never copy photos into the repo. Test photos supplied by the owner live OUTSIDE the repo in `C:\dev\erxtools-testdata\` and are read-only for you. Scratch outputs go into the git-ignored `target/` folder (e.g. `target/heic-check/`). Real-photo tests are `#[ignore]` and read the folder from the environment variable `ERX_HEIC_SAMPLE_DIR`.
- **R9 Gates after every lettered step** (all must be green; fix, do not suppress): `npm run build`; in `src-tauri`: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test` (≥ 337 + new tests); the ARM64 check.
- **R10 Code style.** Mirror existing conventions of the converter. No `unwrap()`/`expect()` in non-test code. Keep pure logic (sniffing, error mapping, size guards, alpha handling) separate from Windows calls so it is unit-testable. Do not refactor unrelated code. No UI libraries.
- **R11 Stop and ask.** If something is not covered here and changes behavior the owner will see, or a step seems to require breaking a rule, STOP and ask in your report instead of guessing.

---

## 3. Decisions (already made — do not reopen)

1. **Approach: decode HEIC/HEIF through Windows' own WIC codec** (the user's installed Microsoft HEIF + HEVC extensions). We ship NO decoder. Reason: no LGPL/AGPL obligations, no HEVC patent exposure for us, zero size impact, fully offline.
2. **Rejected:** bundling libheif/libde265; the pure-Rust `heic` crate (AGPL); Media Foundation. Do not use them.
3. **Scope:** HEIC/HEIF **input only**. Outputs stay JPEG, PNG, WebP, BMP, TIFF. Only the **first (primary) image** of a file is converted (same convention as animated GIF/WebP today). Depth maps, gain maps, Live Photo video: ignored.
4. **No color management in v1.** Output pixels are treated as sRGB; ICC is dropped as for all other formats. iPhone photos are usually Display P3, so colors may look slightly less vivid. This is stated honestly in the UI (6d). Do NOT implement ICC/color transforms in this prompt.
5. **10-bit HEIC is reduced to 8-bit** (the pipeline is RGB8/RGBA8). Accepted for v1.
6. **COM binding crate:** `windows-sys` normally provides only raw function declarations and opaque COM pointers, with NO callable interface methods. The investigation assumed only a feature flag was needed — that is probably wrong. Pre-approved by the owner: add the official **`windows`** crate with `default-features = false` and ONLY the features required for WIC/COM (expected: `Win32_Graphics_Imaging`, `Win32_System_Com`, `Win32_Foundation`; add others only if the compiler demands them). **Hand-writing COM vtables is forbidden** (too error-prone). Step 6a-2 verifies this first.
7. **Missing codecs are an expected situation, not a bug.** The app must say so honestly (wording in 6b-6 and 6d).

---

## 4. THE STEPS

---

# 6a — Foundation: docs, dependency, pure logic, WIC decoder, live smoke test

**6a-1. Documents.**
- Save the complete text of THIS prompt, verbatim, to `docs/PROMPT_6_SPEC.md` (if the owner already placed it there, leave it).
- Create `docs/PROMPT_6_PROGRESS.md` from Appendix A.
- Create `docs/PROMPT_6_DECISIONS.md` containing section 3 above (copy the 7 items) under the title "Prompt 6 — HEIC input: decisions".

**6a-2. Verify the binding question and add the dependency.**
1. Look at the vendored `windows-sys` source (`~/.cargo/registry/src/*/windows-sys-0.61*/src/Windows/Win32/Graphics/Imaging/mod.rs`) and confirm whether callable methods for `IWICImagingFactory` exist. Write one line with your finding into the progress file ("Verified facts").
2. Unless you found callable methods in windows-sys (then use windows-sys and skip step 3), add to `src-tauri/Cargo.toml`: `windows = { version = "<see below>", default-features = false, features = ["Win32_Graphics_Imaging", "Win32_System_Com", "Win32_Foundation"] }`. Version: choose the newest `windows` release whose `windows-link` / `windows-core` dependency versions match what `windows-sys 0.61.x` already pulls into `Cargo.lock`, so no big duplicate crates appear. Check with `cargo tree -d` after adding. If duplicates of large crates appear, report them (do not hide).
3. Run `cargo build` (in `src-tauri`) to make sure it compiles. Then prove no network-capable crate entered the tree: `cargo tree` must contain none of `reqwest`, `hyper`, `tokio`, `ureq`, `socket2`, `mio` (it is fine if `windows` itself appears). Record the result in the progress file.

**6a-3. Create the module** `src-tauri/src/tools/image_convert/heic/` with files `mod.rs`, `sniff.rs`, `error.rs`, `wic.rs`, and declare it from the converter's module file (follow how `naming.rs` etc. are declared). Unit tests live in each file under `#[cfg(test)]`, as elsewhere in the converter.

**6a-4. `sniff.rs` — pure content sniffing (no I/O).**
```rust
pub enum HeifKind { Heic, Avif, OtherHeif, NotHeif }
pub fn sniff_heif(head: &[u8]) -> HeifKind
```
HEIF files are ISO-BMFF: the file starts with an `ftyp` box.
- Need at least 16 bytes, else `NotHeif`.
- Bytes 0..4 = box size (big-endian u32). Bytes 4..8 must equal `b"ftyp"`, else `NotHeif`.
- Header length = 8. If the size field is `1`, a 64-bit size follows at bytes 8..16 and the header length is 16. If the size is `0`, the box extends to the end of the slice. If the box size is smaller than header length + 8 → `NotHeif`.
- `major` = the 4 bytes right after the header; then 4 bytes minor version; then the compatible brands = consecutive 4-byte entries from (header + 8) up to `min(box_end, head.len())`; ignore a trailing partial entry.
- HEIC brand set: `heic heix heim heis hevc hevx hevm hevs`. AVIF set: `avif avis`.
- Decision order: if `major` ∈ AVIF set → `Avif`. Else if `major` ∈ HEIC set → `Heic`. Else if `major` ∈ {`mif1`, `msf1`}: if any compatible brand ∈ HEIC set → `Heic`; else if any ∈ AVIF set → `Avif`; else `OtherHeif`. Anything else (for example `mp42`, `isom`, `qt  `) → `NotHeif`.
- Never panic on short or garbage input.
Tests (minimum): a synthetic `ftypheic` header → `Heic`; `ftypmif1` with compatible `heic` → `Heic`; `ftypmif1` with compatible `avif` → `Avif`; `ftypavif` → `Avif`; `ftypmif1` with only `mif1` → `OtherHeif`; an MP4 (`ftypmp42`) → `NotHeif`; JPEG/PNG starting bytes → `NotHeif`; empty and 15-byte slices → `NotHeif`; 64-bit size form; size `0`; size larger than the slice; truncated compatible-brand list.

**6a-5. `error.rs` — pure error classification + messages.**
```rust
pub enum Stage { CreateDecoder, DecodeFrame, ConvertPixels }
pub enum HeicFailure { CodecOrFile { code: i32 }, Corrupt { code: i32 }, FileAccess { code: i32 }, OutOfMemory, TooLarge, Other { code: i32 } }
pub fn classify_hresult(code: i32, stage: Stage) -> HeicFailure
pub fn user_message(f: &HeicFailure) -> String
```
Rules: we only call WIC for files our sniffer already proved to be HEIC, so a failure most likely means "a required Windows codec is missing" or "the file is damaged".
- `E_OUTOFMEMORY` → `OutOfMemory`.
- Win32 file errors wrapped as HRESULT (file not found, access denied, sharing violation, lock violation) → `FileAccess`.
- WIC codes for a damaged image (names to look up in the `windows` crate, e.g. `WINCODEC_ERR_BADIMAGE`, `WINCODEC_ERR_BADHEADER`, `WINCODEC_ERR_BADSTREAMDATA`, `WINCODEC_ERR_STREAMREAD`, `WINCODEC_ERR_UNEXPECTEDSIZE`) → `Corrupt` **[VERIFY each name exists; use the crate's constants, not hand-typed numbers]**.
- Everything else at any stage → `CodecOrFile { code }` (this includes `WINCODEC_ERR_COMPONENTNOTFOUND` and `WINCODEC_ERR_UNKNOWNIMAGEFORMAT`).
- `TooLarge` is produced by our own size guards, never by an HRESULT.
User messages (exact text, English; `0x…` = the code in 8-digit uppercase hex):
- `CodecOrFile`: "Windows could not open this HEIC photo. iPhone photos need Microsoft's HEIF Image Extensions and HEVC Video Extensions (get them from the Microsoft Store). If they are already installed, the file may be damaged. (Windows error 0x…)"
- `Corrupt`: "This HEIC file looks damaged or incomplete. (Windows error 0x…)"
- `FileAccess`: "Couldn't read this file. It may be in use by another program or you may not have permission. (Windows error 0x…)"
- `OutOfMemory`: "Not enough memory to open this photo."
- `TooLarge`: reuse the converter's existing wording for oversized images/files if it has one; otherwise: "This photo is too large to convert safely."
- `Other`: "Windows could not decode this HEIC photo. (Windows error 0x…)"
Tests (minimum): classification of one representative code per class, including at least two WIC constants taken from the crate (not typed numbers); stage default → `CodecOrFile`; message text contains the hex code, formatted as 8 uppercase hex digits, for negative i32 values; every variant produces a non-empty message.

**6a-6. `wic.rs` — the Windows decoder (the only file with unsafe/COM code).**
Public API:
```rust
pub struct DecodedRgba { pub width: u32, pub height: u32, pub rgba: Vec<u8>, pub frame_count: u32 }
pub fn dimensions(path: &Path) -> Result<(u32, u32), HeicFailure>      // cheap: container only
pub fn decode_first_frame(path: &Path) -> Result<DecodedRgba, HeicFailure>
```
Implementation requirements:
1. **COM initialization per thread.** The batch runs on plain worker threads, so each decode must make sure COM is initialized on the calling thread. Use a `thread_local!` guard struct: on first use call `CoInitializeEx(None, COINIT_MULTITHREADED)`; `S_OK` and `S_FALSE` are success (remember to call `CoUninitialize` in the guard's `Drop` only when we initialized); `RPC_E_CHANGED_MODE` (thread already initialized differently) is NOT an error — continue without uninitializing; any other failure → `HeicFailure::Other`.
2. Create the factory with `CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)` for each call (cheap and thread-safe; do not share factories across threads).
3. Convert the path to a wide string (`OsStr::encode_wide`, NUL-terminated) and call `CreateDecoderFromFilename(path, None, GENERIC_READ, WICDecodeMetadataCacheOnDemand)`. Stage = `CreateDecoder` on failure.
4. `GetFrameCount` → remember it; use frame **0** only (primary image). `GetFrame(0)` then `GetSize`.
5. **Size guards BEFORE allocating pixel memory:** reject with `HeicFailure::TooLarge` if width or height is 0, if `width * height` (use `u64`) exceeds the converter's existing 80 MP constant (import it from `core.rs`; do NOT duplicate the number), or if `width * 4` overflows `u32`. The 256 MB file-size limit is checked by the caller (6b) using the existing constant.
6. Convert pixels with `IWICFormatConverter`: `factory.CreateFormatConverter()`, then `Initialize(&frame, &GUID_WICPixelFormat32bppRGBA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)` (straight, non-premultiplied alpha). Allocate `vec![0u8; width*height*4]` using checked arithmetic and `try_reserve_exact`-style failure handling (allocation failure → `OutOfMemory`, never abort). `CopyPixels(None, stride = width*4, &mut buffer)`. Stage = `DecodeFrame` for GetFrame/GetSize failures and `ConvertPixels` for Initialize/CopyPixels failures.
7. `dimensions()` does steps 1–5 only (no pixel buffer).
8. No `unwrap`/`expect`. Every `unsafe` block gets a one-line `// SAFETY:` comment. Signatures differ between `windows` crate versions: follow the compiler errors and the crate docs; do not guess.

**Live tests (`#[ignore]`, run with `cargo test -- --ignored`):**
- Find the kamadak fixture (`~/.cargo/registry/src/index.crates.io-*/kamadak-exif-0.6.2/tests/exif.heic`; locate it with a glob, do not hardcode the hash folder). Assert: `dimensions()` and `decode_first_frame()` succeed, width/height > 0, `rgba.len() == w*h*4`, not all pixels identical. Save the pixels as a PNG into `target/heic-check/fixture.png` (use the `image` crate's PNG encoder) and **open that PNG with the Read tool to look at it**; describe in the progress file what you see (a real picture? garbage? black?).
- **Calibrate the error mapping with real failures** (record the exact HRESULTs you observe in "Verified facts"): (1) a truncated copy of the fixture (first 40% of its bytes) written to a scratch folder; (2) a file that begins with a valid-looking `ftypheic` header followed by random/garbage bytes; (3) a path that does not exist; (4) a file locked with exclusive sharing by your own test (hold it open without sharing). Assert each failure maps to a sensible `HeicFailure` class and a non-empty message. If a real code lands in an unexpected class, adjust `classify_hresult` (and its tests) and write down why.

**6a-7. Gates, local commit, report, STOP.**

**Done when:** sniffing + error mapping + WIC decoder exist with all tests green; live tests pass on the owner's machine; the dependency audit is in the report; nothing in the app's behavior has changed yet (the converter still rejects HEIC until 6b).

**Owner test checklist 6a:** none needed (internal foundation). The agent reports what it saw in the fixture picture.

---

# 6b — Integration into the converter

**6b-1. New input format.** Add `InputFormat::Heic` (display name **"HEIC"**) in `core.rs`. Follow the compiler: update every `match` (naming, friendly names, tests). `friendly_format_name` for HEIC returns "HEIC". Make sure AVIF still reports "AVIF is not supported in v1" and is NEVER routed to the HEIC path.

**6b-2. Detection in `inspect_path`.** Keep the existing logic first. Only when the `image` crate sniff returns `None`:
1. Read the first **256 bytes** of the file (read-only, `std::fs::File` + `Read::take`) and call `sniff_heif`.
2. `Heic` → continue below. `Avif` → existing AVIF "not supported in v1" behavior. `OtherHeif` → unsupported with reason "unsupported HEIF variant". `NotHeif` → the existing "not a recognized image".
3. For `Heic`: enforce the existing 256 MB file-size limit; then call `wic::dimensions(path)`; apply the existing 80 MP guard (already inside `dimensions`); on success return `Inspection::Supported { format: InputFormat::Heic, width, height, … }` filled the same way as other formats. On failure return an unsupported/failed inspection **carrying the message from `user_message`** so the UI shows the honest reason when the file is dropped (for example "Not added: photo.heic — Windows could not open this HEIC photo. …"). Extend the `Inspection` enum / `inspect_one` mapping as needed so a custom reason string can travel to the frontend; keep every existing reason string for other formats unchanged.
4. Content decides, never the extension: a HEIC renamed to `.jpg` must still be detected as HEIC, and a `.heic` file that is really a JPEG must still go through the normal JPEG path.

**6b-3. Decode branch in `convert.rs`.** Where the decode happens per input format, add the HEIC branch: call `wic::decode_first_frame(path)`; map failure with `user_message`; build `image::RgbaImage::from_raw(w, h, rgba)` (failure → `Other`); **if every alpha byte is 255, convert to `DynamicImage::ImageRgb8`** (avoids needless alpha channels in TIFF/PNG/WebP output; iPhone photos are opaque); otherwise keep `ImageRgba8`. Then continue into the existing `prepare()` and encode path unchanged. **Do NOT apply EXIF orientation for HEIC in this step** (unknown whether WIC already rotates; decided in 6c). Put a `// TODO(6c): orientation` comment there. The existing re-check of dimensions/limits must also run for HEIC.

**6b-4. Testability seam.** Introduce `pub trait HeicDecoder { fn decode(&self, path: &Path) -> Result<DecodedRgba, HeicFailure>; }` implemented by the real `WicDecoder`; the conversion code receives the decoder through a small function parameter or a `static` default so tests can pass a fake. Keep this small — no big refactor.

**6b-5. Tests (minimum, all non-ignored unless stated):**
- Fake decoder returning a 4×2 opaque RGBA image → converts to every output format (JPEG, PNG, WebP, BMP, TIFF) with correct dimensions; output naming works (`photo.heic` → `photo.jpg`, collision `(1)`).
- Fake decoder returning an image with transparency → PNG/WebP/TIFF keep alpha, JPEG/BMP flatten onto the chosen background.
- All-opaque RGBA is collapsed to RGB (assert the in-memory image type).
- Fake decoder failures (`CodecOrFile`, `Corrupt`, `FileAccess`, `OutOfMemory`, `TooLarge`) become per-file errors with the exact messages from 6a-5 and never stop the rest of a batch (mix HEIC failures with good PNGs in one batch; results stay in list order).
- Detection: a file with a synthetic `ftypheic` header is classified `Heic` by `inspect_path` (use the fake/seam or stop at sniffing if WIC is needed); `ftypavif` still gives the AVIF message; content-over-extension test for HEIC renamed `.jpg` and JPEG renamed `.heic`.
- `#[ignore]` live test: the kamadak fixture through the full real pipeline → JPEG, PNG, WebP, TIFF, BMP outputs in a scratch folder; assert files exist and decode back with the `image` crate to the same dimensions.

**6b-6. Gates, local commit, report, STOP.** Note in the report: "HEIC rotation not yet verified (6c)". Do not advertise HEIC in the UI yet (6d).

**Owner test checklist 6b:** none (the owner should NOT test yet; orientation is unverified).

---

# 6c — Real-photo verification (needs the owner's photos)

**6c-0. Required input.** Check whether the folder `C:\dev\erxtools-testdata\` contains `.heic` files. If NOT: stop and ask the owner exactly this (and mark 6c as BLOCKED — waiting for owner in the progress file):
> "Please copy 2–3 iPhone photos into `C:\dev\erxtools-testdata\` (create the folder). Ideally: one taken holding the phone upright (portrait), one taken sideways (landscape), and optionally one with a person or text so rotation is obvious. Use photos you are happy for me to look at. They stay outside the repo and are never committed. Then tell me to continue."

**6c-1. Prepare.** `ERX_HEIC_SAMPLE_DIR=C:\dev\erxtools-testdata`. Write `#[ignore]` live tests that, for every `.heic`/`.heif` in that folder, run the real pipeline to JPEG and PNG into `target/heic-check/` and log: file name, WIC frame count, decoded width × height, output width × height, elapsed milliseconds.

**6c-2. Look at the results.** Open each produced PNG/JPEG with the Read tool and describe, per photo: is it upright (sky up, text readable)? Is it mirrored? Are colors plausible (not green/pink/black)? Is the full picture present (not cropped, not tiled/grid seams visible)? Put the findings in the progress file.

**6c-3. Orientation decision (this is the important part).** iPhone photos are usually stored in the sensor's landscape orientation with a rotation flag. Determine empirically whether WIC already rotates them:
1. If the portrait photo comes out upright and with portrait dimensions (height > width) → **WIC applies rotation already.** Do NOT apply any orientation yourself (that would rotate twice). Write a code comment documenting this fact and the Windows build you observed it on.
2. If the portrait photo comes out sideways (landscape dimensions, content rotated) → WIC does NOT apply rotation. Then read the orientation from WIC metadata with `frame.GetMetadataQueryReader()` and try these queries in order, recording which works: `/ifd/{ushort=274}`, `/app1/ifd/{ushort=274}`; map the value 1–8 to the `image` crate's `Orientation` and apply it through the same code path as other formats (`apply_orientation`). Verify again by looking at the output (6c-2). If no query yields the value and the image is not upright → STOP and ask the owner (R11); do not guess.
3. Either way add a regression test: a pure function test for the orientation-value mapping if you wrote one, and an `#[ignore]` live test asserting the portrait sample's output has height > width.
4. Check the landscape sample too (must stay landscape and upright).

**6c-4. More live checks.** (a) Convert each sample to all five output formats; confirm each file opens (decode it back with the `image` crate) and dimensions match. (b) Batch order: put 2 HEIC + 2 PNG + 1 JPEG in one batch (copy the PNG/JPEG from generated test data) — results must come back in list order. (c) Timing: one photo, and the same photo copied 20 times in a scratch folder as one batch; report seconds and, if you can measure it cheaply (for example by `Get-Process` working set during the run), the approximate peak memory. If peak memory looks dangerous (> ~1.5 GB for 20 ordinary photos), report it instead of fixing it. (d) Copy a sample to a scratch folder with a non-ASCII and a long path (for example `Ünïcode ファイル` and a path longer than 200 characters) and confirm decode works; report.

**6c-5. Gates, local commit, report, STOP.** Include your descriptions from 6c-2 and the decision from 6c-3 in the report.

**Owner test checklist 6c:** none yet; the owner sees the results in 6d.

---

# 6d — UI wording, docs, final verification

**6d-1. Frontend.**
- `types.ts`: `SUPPORTED_INPUT` becomes `"JPEG, PNG, WebP, BMP, GIF, TIFF, HEIC"`. Check every place that prints it (drop-zone hints, "Supported formats: …" rejection message) so the text reads naturally.
- Picker filter in `commands.rs` `DIALOG_EXTENSIONS`: add `"heic"`, `"heif"`, `"hif"`.
- Add a small gray note (use the project's existing note styling) in the converter UI, visible always, under the drop zone or in the options area (choose where it looks best, consistent with existing notes): **"HEIC/HEIF (iPhone photos) is read with the codecs installed in Windows. If a HEIC photo cannot be opened, install Microsoft's “HEIF Image Extensions” and “HEVC Video Extensions” from the Microsoft Store."**
- Extend the existing honest options note (`OptionsPanel.tsx` ~123–126) with one more sentence: **"Wide-color (Display P3) photos, such as iPhone photos, are saved without a color profile and may look slightly less vivid."** Keep the existing sentences about metadata/rotation (they must still be true after 6c — if rotation behaves differently from "applied automatically" for HEIC, STOP and ask).
- Make sure long per-file error messages (the HEIC ones) wrap correctly in the file list without breaking the layout. Check by reading the CSS and, if you can, by triggering the message with a fake HEIC (a text file with an `ftypheic` header).

**6d-2. Docs.** Update `README.md` and any converter docs (look for `docs/` files about the Image Converter) to list HEIC/HEIF input with the honest dependency on Microsoft's extensions and the Display P3 note. Do not claim anything the tests did not prove. Mark Prompt 6 steps DONE in `docs/PROMPT_6_PROGRESS.md`.

**6d-3. Audit (put in the report):** new Cargo dependencies (expect: `windows` only), its enabled features, whether any duplicate large crates appeared, any change to capabilities/permissions (expect none), CSP (expect unchanged), network-capable crates in `cargo tree` (expect none).

**6d-4. Final gates** (R9) including the ARM64 check, then local commit (`Prompt 6d: HEIC input`), report, STOP. Never push.

**Owner test checklist 6d (include in your report, adjusted to reality):**
1. Open Image Converter: the supported list and the gray HEIC note mention HEIC.
2. Drag one of your iPhone photos in → it appears in the list (no red "Not added" message).
3. Convert it to JPEG → open the result: upright, normal colors (slightly less vivid than on the phone is expected).
4. Try PNG and WebP outputs too.
5. Drag in a mix of JPG, PNG and HEIC (10–20 files) → all convert, in order.
6. Drag in a photo you saved from WhatsApp/Telegram (usually JPG) → still works as before.
7. (Optional, for friends without the codecs) The honest message appears instead of a crash.

---

## APPENDIX A — template for `docs/PROMPT_6_PROGRESS.md`

```markdown
# Prompt 6 — HEIC input: progress

Last updated: <date/time>.

## How to resume
Read docs/PROMPT_6_SPEC.md (full prompt) and docs/PROMPT_6_DECISIONS.md. Continue at the first step not marked DONE. Never git push.

## Status
| Step | Description | Status | Notes |
|---|---|---|---|
| 6a-1 | Docs | NOT STARTED | |
| 6a-2 | Binding check + dependency | NOT STARTED | |
| 6a-3..5 | heic module: sniff + errors | NOT STARTED | |
| 6a-6 | WIC decoder + live tests | NOT STARTED | |
| 6a | Gates + commit | NOT STARTED | |
| 6b | Integration | NOT STARTED | |
| 6c | Real-photo verification | NOT STARTED | (needs owner photos) |
| 6d | UI wording, docs, final | NOT STARTED | |
(Status: NOT STARTED / IN PROGRESS / DONE / BLOCKED — waiting for owner)

## Gates (last run)
- npm run build:
- cargo fmt --check:
- cargo clippy -- -D warnings:
- cargo test (count):
- ARM64 check:

## Verified facts
(binding crate finding, observed HRESULTs for each failure case, what the fixture looks like, WIC rotation behavior and Windows build, timings/memory...)

## Dependencies / features added

## Open questions for the owner
```

## APPENDIX B — report format after every lettered step

1. **What was done** (3–6 lines, plain English).
2. **Gates:** build / fmt / clippy / test (count) / ARM64.
3. **Files created/changed.**
4. **New dependencies / features / permissions** (or "none").
5. **Verified facts and anything that could NOT be verified.**
6. **Owner test checklist** (or "none for this step").
7. **Known limits / honest caveats.**
8. **Questions for the owner** (only if needed).
9. **Progress file updated:** yes. **Local commit:** hash + message. **Pushed:** NO (never).
10. "Waiting for the owner to say continue."

---

## FINAL REMINDERS

- Start with section 0, then do 6a only. Stop after each lettered step.
- Do not use libheif, `heic` (AGPL) or Media Foundation. Do not hand-write COM vtables.
- Never commit photos. Never push. Never apply orientation twice.
- Verify every [VERIFY] item on the real machine, and say plainly what you could not verify.