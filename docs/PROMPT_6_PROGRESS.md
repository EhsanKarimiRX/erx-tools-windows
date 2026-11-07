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
| 6b | Integration | DONE | 374 tests (+11), live 6/6; HEIC rotation not yet verified (6c) |
| 6c | Real-photo verification | DONE | WIC applies rotation itself (verified visually); 12/12 live tests |
| 6d | UI wording, docs, final | DONE | HEIC advertised in UI with codec note; README updated; all gates green |

**PROMPT 6 COMPLETE.** Last updated: end of 6d session, 2026-11-07.
(Status: NOT STARTED / IN PROGRESS / DONE / BLOCKED — waiting for owner)

## Gates (last run, end of 6d — final for Prompt 6)
- npm run build: PASS (450ms)
- cargo fmt --check: PASS
- cargo clippy -- -D warnings: PASS
- cargo test (count): 374 passed, 0 failed, 18 ignored (6 pre-existing live + 12 live HEIC)
- ARM64 check: PASS (cargo check --target aarch64-pc-windows-msvc)
- Live HEIC tests: 12/12 PASS (re-run after all 6d changes)

## 6d changes
- `types.ts`: SUPPORTED_INPUT and SUPPORTED_FORMATS_MESSAGE now include HEIC.
- `commands.rs`: DIALOG_EXTENSIONS + `heic`, `heif`, `hif` (Windows matches extensions case-insensitively, so the owner's `.HEIC` files are covered).
- `index.tsx`: gray HEIC-codec note under the empty-state drop zone; `OptionsPanel.tsx`: the honest metadata note gained the Display P3 sentence, plus a second note naming the two Store extensions (rotation sentence stays true — WIC applies it, verified in 6c).
- Long HEIC error messages wrap: `.ic-name` cell already has `max-width: 360px` + `word-break: break-all`, and `word-break` inherits into `.ic-file-note`; the warning block wraps at word boundaries. No CSS change needed.
- README.md: Image Converter entry lists HEIC/HEIF input with the Store-extension dependency and the P3 note.

## 6d audit (R3)
- New Cargo dependencies for the whole of Prompt 6: `windows 0.62` (default-features off; features Win32_Graphics_Imaging, Win32_System_Com, Win32_Foundation) — merges with the same version Tauri already pulls. windows-sys features added: Win32_System_Threading + Win32_System_ProcessStatus (test-only, peak-memory measurement).
- Duplicate crates: none introduced by Prompt 6. `windows 0.62.2` / `windows-core 0.62.2` / `windows-link 0.2.1` each remain a single node. windows-sys 0.61.2 appears as two nodes because cargo keeps build-dependencies (tauri-build/walkdir) separate from normal dependencies — that split predates Prompt 6; the other duplicate pairs (serde, base64, …) are the pre-existing Tauri baseline.
- Tauri capabilities/permissions: untouched since the Prompt 5 baseline (`git diff a1d95c2..HEAD -- tauri.conf.json capabilities/` is empty). CSP: unchanged. Network-capable crates in the active build graph: none added (only the pre-existing `tokio` via tauri).

## Verified facts
- Baseline before any change: git clean at a1d95c2 ("Prompt 5h ... - 337 tests").
- ARM64 check command = `cargo check --target aarch64-pc-windows-msvc` (root DECISIONS.md, per Prompt 5 records; target installed).
- **(6a-2) Binding finding:** windows-sys 0.61.2 contains NO WIC module at all — `src/Windows/Win32/Graphics/` has no Imaging folder, its Cargo.toml has no `Graphics_Imaging` feature, and grepping the whole vendored source for `IWICImagingFactory` / `CreateDecoderFromFilename` finds zero hits. So windows-sys cannot even declare the COM interfaces; the `windows` crate (decision 6) is required, as anticipated.
- **(6a-2) Version choice:** `windows 0.62.2`, `windows-core 0.62.2`, `windows-link 0.2.1` were ALREADY in Cargo.lock before our change (pulled by tauri, tao, wry, webview2-com, tauri-plugin-opener, tauri-winrt-notification). Added `windows = { version = "0.62", default-features = false, features = ["Win32_Graphics_Imaging", "Win32_System_Com", "Win32_Foundation"] }` — features merge into the existing crate; `cargo tree` shows exactly one `windows v0.62.2`, one `windows-core v0.62.2`, one `windows-link v0.2.1` (each only one version). No new crate entered the tree.
- **(6a-2) Build:** `cargo build` green (47.9s dev profile).
- **(6a-2) Duplicate audit:** `cargo tree -d` shows only the pre-existing Tauri-baseline duplicates (e.g. base64 0.22.1 vs 0.23.1). Nothing new from our change.
- **(6a-2) Network-crate audit (baseline vs after, active build graph via `cargo tree`):** before = only `tokio v1.54.1` (via `tauri v2.12.5`); after = identical. `reqwest`, `hyper`, `mio`, `socket2` appear only as Cargo.lock entries for other platforms/optional deps and are NOT in the active graph; `ureq` is absent entirely. Our change added no network-capable crate.
- **(6a-5) windows 0.62.2 exports no `HRESULT_FROM_WIN32` helper** (checked windows + windows-core sources). Implemented the documented FACILITY_WIN32 wrap locally in `heic/error.rs`; the error numbers compared against are the crate's `WIN32_ERROR` constants. Live tests (missing file, locked file) confirmed the wrap matches what WIC actually returns.
- **(6a-6) Live smoke test on the kamadak-exif fixture** (`C:\Users\<user>\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\kamadak-exif-0.6.2\tests\exif.heic`): WIC `dimensions()` = 64×64, `decode_first_frame()` succeeds, `frame_count` ≥ 1, pixels are real (not uniform). The exported PNG (`target/heic-check/fixture.png`) shows a **white 64×64 picture with small black text reading "Exif"** — the crate's synthetic test image. Not garbage, not black: the WIC path genuinely decodes HEIC on this machine.
- **(6a-6) Observed HRESULTs for the error-calibration cases** (from `cargo test heic -- --ignored --nocapture`):
  - locked file (exclusive share) → `-2147024864` = `0x80070020` (ERROR_SHARING_VIOLATION wrapped) → **FileAccess** ✓ as designed
  - missing path → `-2147024894` = `0x80070002` (ERROR_FILE_NOT_FOUND wrapped) → **FileAccess** ✓ as designed
  - truncated fixture (first 40%) → `-1072875842` = `0xC00D36BE` = **`MF_E_INVALID_FILE_FORMAT`** (Media Foundation error surfaced by Microsoft's HEIF decoder) → lands in **CodecOrFile** per the spec's default rule. Kept there deliberately: the message already says "If they are already installed, the file may be damaged", which is the honest wording for this code — and on a machine WITHOUT the codecs we cannot observe which code appears, so claiming "Corrupt" could be wrong. No adjustment to `classify_hresult` needed.
  - valid `ftypheic` header + garbage → same `0xC00D36BE` → **CodecOrFile** (same reasoning)
- **(6a-7) Clippy findings fixed properly, not suppressed:** `chunks_exact(4)` → `as_chunks::<4>()` in sniff.rs; thread_local initializer made `const` in wic.rs.
- **(6a-7) Temporary `#[allow(dead_code)]` on `pub mod heic`** in `image_convert/mod.rs` (with TODO(6b) comment): `mod tools;` is private in lib.rs, so the 6a scaffold is unreachable from the crate root until 6b wires it in — without the allowance `cargo clippy -D warnings` fails on 21 dead-code warnings. Removed in 6b.
- **(6b-1)** `InputFormat::Heic` added (label "HEIC"); `matches_output` intentionally has no HEIC arm (input-only, never re-compresses). `from_image_format` unchanged: the `image` crate has no HEIF variant, so HEIC detection comes only from the new sniff fallback (6b-2). Spec note: "friendly_format_name for HEIC returns HEIC" cannot literally apply — that function maps `image::ImageFormat` values, which include no HEIC variant; the HEIC display name flows through `InputFormat::label()`. Temporary dead-code allowance REMOVED in this step.
- **(6b-2)** `Inspection::Failed { reason }` added (new variant) and mapped in both consumers: `commands.rs inspect_one` → kind "unsupported" with the custom reason; `pipeline.rs convert_one_file` → per-file Err(reason). `inspect_path` now falls back to `heic_or_unsupported` ONLY when the `image` crate sniff returns None: reads the first 256 bytes (`File::take`), sniffs, and routes: Heic → `wic::dimensions` (80 MP guard inside) → `Image{Heic}` or `Failed{user_message}`; Avif → existing `NotSupported{Some("AVIF")}` ("AVIF is not supported in v1" preserved); OtherHeif → `Failed{"unsupported HEIF variant"}`; NotHeif → existing `NotSupported{None}` ("not a recognized image"). The 256 MB limit is already enforced before this point; content decides, never the extension.
- **(6b-3/4)** `convert.rs` gained the `HeicDecoder` trait seam (`Send + Sync`), the production `WicDecoder` (static default via `default_heic_decoder()`), `heic_image` (re-checks the 80 MP guard, collapses all-opaque RGBA to RGB, keeps alpha otherwise; `// TODO(6c): orientation` comment in place — NO orientation applied yet), `convert_heic_with(decoder, source, options)` and `convert_heic(source, options)`. `pipeline.rs convert_one_file` routes `InputFormat::Heic` to `convert_heic` (path-based WIC) and everything else to the existing stream path. `DecodedRgba` got `Clone` (fake-decoder tests) and a documented `#[allow(dead_code)]` on `frame_count` (spec-mandated field, read by live tests; production consumers come in 6c/6d). Deviation note: `RgbaImage::from_raw` failure (unreachable decoder-contract bug) uses a plain message "Windows returned HEIC pixels in an unexpected size" instead of `HeicFailure::Other` — an `Other` message would print a meaningless "(Windows error 0x00000000)".
- **(6c-0)** Owner's photos found: `C:\dev\erxtools-testdata\200.HEIC`, `201.HEIC`, `202.HEIC` (uppercase extensions; sample lister is case-insensitive). Used `ERX_HEIC_SAMPLE_DIR=C:\dev\erxtools-testdata`.
- **(6c-1/6c-2) Findings on the owner's three iPhone photos** (all 12 MP HEVC iPhone photos; debug-build timings):
  - `200.HEIC`: container 3024×4032 portrait, 1 WIC frame; JPEG out 3024×4032 in ~3994 ms (debug), PNG ~4857 ms. **Seen with the Read tool: car badge (Peugeot lion emblem) — upright, portrait, natural colors, full frame, no grid seams, no mirroring.**
  - `201.HEIC`: 3024×4032, 1 frame; JPEG ~3973 ms. **Seen: three knurled metal knobs — upright, natural colors, no artifacts.**
  - `202.HEIC`: 3024×4032, 1 frame; JPEG ~4046 ms. **Seen: folded suede/leather pieces (red/black/blue) — upright, vivid natural colors, no artifacts.**
- **(6c-3) ROTATION DECISION — WIC APPLIES ROTATION ITSELF (case 1 of the spec).** iPhone photos are stored in sensor (landscape) orientation with a rotation flag; WIC reported the rotated portrait dimensions (3024×4032, height > width) and the content is upright and not mirrored in every converted output. Therefore NO EXIF-orientation code was added (applying it would rotate twice). Documented in `heic_image`'s doc comment in convert.rs with the observed Windows build (26300, HEIF Image Extension 1.2.62.0 + HEVC Video Extension 2.5.48.0). Regression test `live_sample_rotation_is_applied_by_wic` locks this in. Honest gap: the owner provided three PORTRAIT samples only — no landscape sample existed to check the same flag in the other direction; the mechanism (irot/imir) is the same either way.
- **(6c-4a)** All three samples converted to all five outputs (JPEG/PNG/WebP/BMP/TIFF); every output decoded back at exactly 3024×4032.
- **(6c-4b)** Mixed batch of 5 (2 HEIC + 2 generated PNG + 1 generated JPEG) through a 4-worker mini-pool mirroring the batch command: every result landed in its own list slot with the correct output name. One test-expectation fix along the way: the generated JPEG converted to JPEG output is a same-format re-compression and correctly takes the `-converted` name (app behavior right; test expectation fixed).
- **(6c-4c) Timing (DEBUG build — the release app is much faster):** single photo ≈ 4.0 s JPEG; 20-copy batch with 4 workers ≈ 26.2 s total. **Peak memory:** process peak working set 1364 MiB via `K32GetProcessMemoryInfo` — caveat: cargo test runs live tests in parallel in ONE process, so this peak includes concurrent other tests; the 20-photo batch itself did not push it higher. Under the 1.5 GB danger line from the spec.
- **(6c-4d)** Non-ASCII path (`Ünïcode ファイル\写真 テスト.HEIC`) converts with the unicode stem preserved (`写真 テスト.jpg`); a 247-character path decodes fine.
- **(6c) New windows-sys features:** `Win32_System_Threading` + `Win32_System_ProcessStatus` (test-only use: peak-memory measurement in live tests). Listed per R3.

## Dependencies / features added
- `src-tauri/Cargo.toml`: `windows = { version = "0.62", default-features = false, features = ["Win32_Graphics_Imaging", "Win32_System_Com", "Win32_Foundation"] }` (more features only if the compiler demands them). No other dependency/feature/permission changes.

## Open questions for the owner
(none yet)
