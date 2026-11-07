# Prompt 9 — Name/logo notice + update button: progress
Last updated: 2026-11-07 (9b done; continuing straight into 9c per spec)

## How to resume
Read docs/PROMPT_9_SPEC.md (full prompt). Continue at the first step not DONE. Never git push.

## Status
| Step | Description | Status | Notes |
|---|---|---|---|
| 9a | Branding notice (docs) | DONE | commit 33f7e43; owner approved wording |
| 9b | Pure logic (semver, JSON, decide, version test) | DONE | 405 tests green |
| 9c | WinHTTP client, commands, debug switch | IN PROGRESS | |
| 9d | Home screen UpdateBadge | NOT STARTED | |
| 9e | Docs, audit, final gates | NOT STARTED | |

## Gates (last run: after 9b)
- `npm run build` OK
- `cargo fmt --check` OK
- `cargo clippy -- -D warnings` OK
- `cargo test`: 405 passed, 0 failed, 18 ignored (baseline was 374 → +31 new)
- `cargo check --target aarch64-pc-windows-msvc` OK

## Verified facts
- Resume check at session start: no PROMPT_9 files existed; git clean at `e01b250` (Prompt 8) → fresh start with 9a.
- **9b-5 (version source):** the home screen (`src/app/HomeScreen.tsx`) calls the `get_app_info` command, which returns `app.package_info().version.to_string()` (`src-tauri/src/tools/app_info.rs`) — single source `tauri.conf.json`'s `version`. NOT hardcoded; no fix needed. The 9c command will use the same package-info call.
- **Module location:** the project convention puts everything under `src-tauri/src/tools/` (`tools/mod.rs` says "Backends for the app's tools, plus app-level commands"; `app_info` lives there). Following the spec's instruction, the module is therefore `src-tauri/src/tools/update_check/` (mod.rs, version.rs, release.rs, result.rs), declared in `tools/mod.rs`.
- Version files checked today: `tauri.conf.json` = 0.1.0, `package.json` = 0.1.0, `Cargo.toml` = 0.1.0. The consistency test (`tools::update_check::consistency_tests::version_is_consistent_across_config_files`) compiles the `include_str!` paths, so they resolve.
- `html_url` in the GitHub answer is treated as required (missing → `Invalid`), since a usable release without a URL is malformed.
- serde_json parses a 1000-level-deep nested value inside an ignored field without panicking (no recursion-limit rejection observed at that depth in the vendored version); the test asserts no-panic + release still parsed.
- Clippy `dead_code` fires for the new pure-logic module (nothing consumes it until 9c's commands). Handled with the project's existing pattern (see `folder_icon/probe.rs`): one documented `#![allow(dead_code)]` at `update_check/mod.rs`, to be removed when the commands land in 9c.

## Open questions
(none yet)
