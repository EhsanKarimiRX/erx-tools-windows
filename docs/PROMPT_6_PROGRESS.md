# Prompt 6 — HEIC input: progress

Last updated: 2026-11-07 (start of session).

## How to resume
Read docs/PROMPT_6_SPEC.md (full prompt) and docs/PROMPT_6_DECISIONS.md. Continue at the first step not marked DONE. Never git push.

## Status
| Step | Description | Status | Notes |
|---|---|---|---|
| 6a-1 | Docs | DONE | spec copied verbatim from owner's paste; progress + decisions created |
| 6a-2 | Binding check + dependency | DONE | windows-sys 0.61.2 has NO Imaging module; `windows 0.62` added (already in tree via Tauri, merges cleanly) |
| 6a-3..5 | heic module: sniff + errors | DONE | 15 sniff tests, 9 error tests |
| 6a-6 | WIC decoder + live tests | DONE | 5 live tests pass; fixture decodes (64×64 "Exif" image); error codes calibrated |
| 6a | Gates + commit | DONE | see Gates below |
| 6b | Integration | NOT STARTED | |
| 6c | Real-photo verification | NOT STARTED | (needs owner photos) |
| 6d | UI wording, docs, final | NOT STARTED | |
(Status: NOT STARTED / IN PROGRESS / DONE / BLOCKED — waiting for owner)

## Gates (last run, end of 6a)
- npm run build: PASS (tsc + vite, 687ms)
- cargo fmt --check: PASS
- cargo clippy -- -D warnings: PASS
- cargo test (count): 363 passed, 0 failed, 11 ignored (2 pre-existing live suites + 5 new live HEIC tests); baseline was 337 passing, so +26 new non-live tests
- ARM64 check: PASS (cargo check --target aarch64-pc-windows-msvc)
- Live HEIC tests: 5/5 PASS (cargo test heic -- --ignored)

## Verified facts
- Baseline before any change: git clean at a1d95c2 ("Prompt 5h ... - 337 tests").
- ARM64 check command = `cargo check --target aarch64-pc-windows-msvc` (root DECISIONS.md, per Prompt 5 records; target installed).
- **(6a-2) Binding finding:** windows-sys 0.61.2 contains NO WIC module at all — `src/Windows/Win32/Graphics/` has no Imaging folder, its Cargo.toml has no `Graphics_Imaging` feature, and grepping the whole vendored source for `IWICImagingFactory` / `CreateDecoderFromFilename` finds zero hits. So windows-sys cannot even declare the COM interfaces; the `windows` crate (decision 6) is required, as anticipated.
- **(6a-2) Version choice:** `windows 0.62.2`, `windows-core 0.62.2`, `windows-link 0.2.1` were ALREADY in Cargo.lock before our change (pulled by tauri, tao, wry, webview2-com, tauri-plugin-opener, tauri-winrt-notification). Added `windows = { version = "0.62", default-features = false, features = ["Win32_Graphics_Imaging", "Win32_System_Com", "Win32_Foundation"] }` — features merge into the existing crate; `cargo tree` shows exactly one `windows v0.62.2`, one `windows-core v0.62.2`, one `windows-link v0.2.1` (each only one version). No new crate entered the tree.
- **(6a-2) Build:** `cargo build` green (47.9s dev profile).
- **(6a-2) Duplicate audit:** `cargo tree -d` shows only the pre-existing Tauri-baseline duplicates (e.g. base64 0.22.1 vs 0.23.1). Nothing new from our change.
- **(6a-2) Network-crate audit (baseline vs after, active build graph via `cargo tree`):** before = only `tokio v1.54.1` (via `tauri v2.12.5`); after = identical. `reqwest`, `hyper`, `mio`, `socket2` appear only as Cargo.lock entries for other platforms/optional deps and are NOT in the active graph; `ureq` is absent entirely. Our change added no network-capable crate.
- **(6a-5) windows 0.62.2 exports no `HRESULT_FROM_WIN32` helper** (checked windows + windows-core sources). Implemented the documented FACILITY_WIN32 wrap locally in `heic/error.rs`; the error numbers compared against are the crate's `WIN32_ERROR` constants. Live tests (missing file, locked file) confirmed the wrap matches what WIC actually returns.
- **(6a-6) Live smoke test on the kamadak-exif fixture** (`C:\Users\EhsanRX\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\kamadak-exif-0.6.2\tests\exif.heic`): WIC `dimensions()` = 64×64, `decode_first_frame()` succeeds, `frame_count` ≥ 1, pixels are real (not uniform). The exported PNG (`target/heic-check/fixture.png`) shows a **white 64×64 picture with small black text reading "Exif"** — the crate's synthetic test image. Not garbage, not black: the WIC path genuinely decodes HEIC on this machine.
- **(6a-6) Observed HRESULTs for the error-calibration cases** (from `cargo test heic -- --ignored --nocapture`):
  - locked file (exclusive share) → `-2147024864` = `0x80070020` (ERROR_SHARING_VIOLATION wrapped) → **FileAccess** ✓ as designed
  - missing path → `-2147024894` = `0x80070002` (ERROR_FILE_NOT_FOUND wrapped) → **FileAccess** ✓ as designed
  - truncated fixture (first 40%) → `-1072875842` = `0xC00D36BE` = **`MF_E_INVALID_FILE_FORMAT`** (Media Foundation error surfaced by Microsoft's HEIF decoder) → lands in **CodecOrFile** per the spec's default rule. Kept there deliberately: the message already says "If they are already installed, the file may be damaged", which is the honest wording for this code — and on a machine WITHOUT the codecs we cannot observe which code appears, so claiming "Corrupt" could be wrong. No adjustment to `classify_hresult` needed.
  - valid `ftypheic` header + garbage → same `0xC00D36BE` → **CodecOrFile** (same reasoning)
- **(6a-7) Clippy findings fixed properly, not suppressed:** `chunks_exact(4)` → `as_chunks::<4>()` in sniff.rs; thread_local initializer made `const` in wic.rs.
- **(6a-7) Temporary `#[allow(dead_code)]` on `pub mod heic`** in `image_convert/mod.rs` (with TODO(6b) comment): `mod tools;` is private in lib.rs, so the 6a scaffold is unreachable from the crate root until 6b wires it in — without the allowance `cargo clippy -D warnings` fails on 21 dead-code warnings. Removed in 6b.

## Dependencies / features added
- `src-tauri/Cargo.toml`: `windows = { version = "0.62", default-features = false, features = ["Win32_Graphics_Imaging", "Win32_System_Com", "Win32_Foundation"] }` (more features only if the compiler demands them). No other dependency/feature/permission changes.

## Open questions for the owner
(none yet)
