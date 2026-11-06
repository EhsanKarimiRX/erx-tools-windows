# Prompt 5 — System Manager: progress

Last updated: 2026-11-06 by the agent.

## How to resume
Read docs/PROMPT_5_SPEC.md (full prompt) and docs/PROMPT_5_DECISIONS.md. Continue at the first step not marked DONE. Never git push.

## Status
| Step | Description | Status | Notes |
|---|---|---|---|
| 5a | Foundation (tile, docs) | DONE | gates green; committed locally |
| 5b-1 | Awake: pure logic + state file | NOT STARTED | |
| 5b-2 | Awake: Windows power backend | NOT STARTED | |
| 5b-3 | Awake: runtime, commands, thread, exit | NOT STARTED | |
| 5b-4 | Awake: UI | NOT STARTED | |
| 5b-5 | Awake: live verification | NOT STARTED | |
| 5c | Desktop Icons | NOT STARTED | |
| 5d-1 | Organize: pure logic | NOT STARTED | |
| 5d-2 | Organize: scan | NOT STARTED | |
| 5d-3 | Organize: execute, manifest, undo | NOT STARTED | |
| 5d-4 | Organize: UI | NOT STARTED | |
| 5e | Quick Launch | NOT STARTED | |
| 5f | Command Tips | NOT STARTED | |
| 5g | Final polish + verification | NOT STARTED | |
(Status values: NOT STARTED / IN PROGRESS / DONE / BLOCKED — waiting for owner)

## Gates (last run)
- npm run build: PASS (2026-11-06, step 5a)
- cargo fmt --check: PASS
- cargo clippy -- -D warnings: PASS
- cargo test (count): 202 passed, 0 failed
- ARM64 check: PASS (cargo check --target aarch64-pc-windows-msvc)

## Conventions found (filled in 5a-1)
1. Frontend tool = `src/tools/<kebab-id>/index.tsx`, default export renders **content only** — the Back+title header comes from `ToolPage` via `src/shared/ToolLayout.tsx`; lazy loading is automatic from the registry entry.
2. Registration = one entry in `src/tools/registry.ts` (id, name, description, lucide icon, `load: () => import("./<id>")`, status); the home screen is generated from that list; no routes.
3. Never import from another tool; shared UI lives in `src/shared/` (currently only DropZone, ProgressBar, ToolLayout); each tool has its own CSS file next to the components, styled with the CSS variables from `src/styles/theme.css` (automatic light/dark via `prefers-color-scheme`).
4. Backend tool = `src-tauri/src/tools/<snake_id>/` folder with `mod.rs` listing submodules; pure logic files (unit-tested, no Tauri types) + a thin `commands.rs` with `#[tauri::command]` fns returning `Result<T, crate::tools::ToolError>`.
5. Commands are registered in exactly ONE place: `generate_handler!` in `src-tauri/src/lib.rs`; the module is listed in `src-tauri/src/tools/mod.rs`. Frontend calls `invoke("<command>", { camelCaseArgs })`.
6. Errors: `ToolError::Internal(String)` carries the user-facing English sentence; it serializes to a plain string for the UI.
7. Plugins (dialog, opener) are initialized in `lib.rs` and used from Rust commands only, so the webview capability stays `core:default` only (`src-tauri/capabilities/default.json`).
8. Progress events: Tauri `ipc::Channel<T>` as a command parameter + frontend `new Channel()` with `onmessage` (Image Converter pattern); long-running batch state is managed via `.manage(Mutex<...>)`.
9. Tests: unit tests in-module (`#[cfg(test)]`); live Windows tests in a separate file (folder_icon/live_probe_test.rs pattern) behind `#[ignore]`, run only on purpose.
10. Gates: `npm run build`; in `src-tauri`: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test` (baseline 202), ARM64 = `cargo check --target aarch64-pc-windows-msvc` (recorded in root DECISIONS.md; target installed). windows-sys features get added to Cargo.toml as compile errors demand. No tool writes app-data files yet — 5b's state file will use Tauri's `app.path().app_data_dir()`.

## Owner snapshots — do not lose
(Original power values, desktop icon registry values, etc. recorded BEFORE live tests.)

## Verified facts ([VERIFY] items)
- ARM64 check command = `cargo check --target aarch64-pc-windows-msvc` (root DECISIONS.md, commit fbb413d; target present in rustup).
- `lucide-react` 1.66.0 has `MonitorCog` (icon for the System Manager tile).

## Open issues / questions for the owner

## Dependencies / permissions / windows-sys features added so far
None yet.
