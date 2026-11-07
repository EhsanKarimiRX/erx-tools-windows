# Prompt 9 — Name/logo notice + update button: progress
Last updated: 2026-11-07 (9d done; continuing into 9e)

## How to resume
Read docs/PROMPT_9_SPEC.md (full prompt). Continue at the first step not DONE. Never git push.

## Status
| Step | Description | Status | Notes |
|---|---|---|---|
| 9a | Branding notice (docs) | DONE | commit 33f7e43; owner approved wording |
| 9b | Pure logic (semver, JSON, decide, version test) | DONE | commit a97e62b; 405 tests |
| 9c | WinHTTP client, commands, debug switch | DONE | live tests 2/2; STOP for owner |
| 9d | Home screen UpdateBadge | DONE | commit follows; popover text per spec |
| 9e | Docs, audit, final gates | NOT STARTED | |

## Gates (last run: after 9c)
- `npm run build` OK
- `cargo fmt --check` OK
- `cargo clippy -- -D warnings` OK
- `cargo test`: 417 passed, 0 failed, 20 ignored (baseline 374 → +43)
- `cargo check --target aarch64-pc-windows-msvc` OK
- `cargo test update -- --ignored --nocapture`: 2/2 live tests pass

## Verified facts
- Resume check at session start: no PROMPT_9 files existed; git clean at `e01b250` (Prompt 8) → fresh start with 9a.
- **9b-5 (version source):** the home screen (`src/app/HomeScreen.tsx`) calls the `get_app_info` command, which returns `app.package_info().version.to_string()` (`src-tauri/src/tools/app_info.rs`) — single source `tauri.conf.json`'s `version`. NOT hardcoded; no fix needed. `check_for_updates` uses the same `package_info()` call (`current_version` in commands.rs).
- **Module location:** the project convention puts everything under `src-tauri/src/tools/` (`tools/mod.rs` says "Backends for the app's tools, plus app-level commands"; `app_info` lives there). Following the spec's instruction, the module is `src-tauri/src/tools/update_check/` (mod.rs, version.rs, release.rs, result.rs, http.rs, commands.rs).
- Version files checked today: `tauri.conf.json` = 0.1.0, `package.json` = 0.1.0, `Cargo.toml` = 0.1.0. The consistency test compiles the `include_str!` paths, so they resolve.
- **9c-1:** all 10 needed WinHTTP functions exist in vendored windows-sys-0.61.2 (declared via `windows_link::link!("winhttp.dll")`) — no `windows`-crate fallback needed. Constant values verified in the vendored source: ERROR_WINHTTP_TIMEOUT=12002, NAME_NOT_RESOLVED=12007, CANNOT_CONNECT=12029, CONNECTION_ERROR=12030, SECURE_FAILURE=12175, plus the full ERROR_WINHTTP_SECURE_* family (12157, 12037, 12038, 12044, 12045, 12057, 12169, 12170, 12179, 12187, 12188) — all classified as Tls.
- **9c-2 live debugging (verified on the real machine):** the first live run failed BOTH tests with WinHTTP error 87 (ERROR_INVALID_PARAMETER) at exactly `WinHttpSendRequest`. Root cause: `dwHeadersLength` must EXCLUDE the NUL terminator. After the one-line fix: real GitHub answered **HTTP 404** (body `{"message":"Not Found",...,"status":"404"}`) — as the spec predicted (no release published yet); `decide` maps 404 → NoRelease. Unreachable host `127.0.0.1:443` produced WinHTTP error **12029** → classified `CannotReach` (the spec's hint number, confirmed live).
- `html_url` in the GitHub answer is treated as required (missing → `Invalid`).
- serde_json parses a 1000-level-deep nested value inside an ignored field without panicking; test asserts no-panic + release still parsed.
- Tauri 2 rule honored: an `async` command borrowing `State<'_, _>` must return `Result`, so `check_for_updates` returns `Result<UpdateCheckResult, ToolError>` (project's error type); it never returns Err in practice.
- The async blocking pattern: the whole request runs in `tauri::async_runtime::spawn_blocking`; a panicked/failed worker becomes `Failed("GitHub's answer could not be understood.")` via `unwrap_or_else` (no unwrap in non-test code).
- `open_update_page` uses a small dedicated `ShellExecuteW` opener (verb "open") because Quick Launch's helper is private and ShellExecuteExW-based with Quick-Launch-specific semantics (admin verbs, masks).
- Debug diagnostics: failed WinHTTP steps print `update check: WinHTTP error code <n> at step: <name>` (debug builds only, `if cfg!(debug_assertions)`); this is what made the error-87 root cause findable.

## Open questions
(none — everything in the spec was buildable as written)
