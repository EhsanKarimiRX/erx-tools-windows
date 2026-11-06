# Prompt 5 — System Manager: progress

Last updated: 2026-11-06 by the agent.

## How to resume
Read docs/PROMPT_5_SPEC.md (full prompt) and docs/PROMPT_5_DECISIONS.md. Continue at the first step not marked DONE. Never git push.

## Status
| Step | Description | Status | Notes |
|---|---|---|---|
| 5a | Foundation (tile, docs) | DONE | gates green; committed locally |
| 5b-1 | Awake: pure logic + state file | DONE | 43 new tests; 245 total pass |
| 5b-2 | Awake: Windows power backend | DONE | WindowsPower; GUIDs verified via powercfg -aliases |
| 5b-3 | Awake: runtime, commands, thread, exit | DONE | commands wired in lib.rs; close interception + 5s thread; notification plugin added (Rust-side only) |
| 5b-4 | Awake: UI | DONE | toggle/status/warn/drawer/banners/close dialog; chunk lazy-loads |
| 5b-5 | Awake: live verification | DONE | 3 live tests pass; G9 snapshot verified unchanged afterwards |
| 5c | Desktop Icons | DONE | 5 toggles + refresh; live test passed; snapshot intact; owner mirror test pending |
| 5d-1 | Organize: pure logic | DONE | categories, skip rules, unique_name, protected paths; 42 new tests |
| 5d-2 | Organize: scan | DONE | known folders via SHGetKnownFolderPath; top-level only; OneDrive + Public Desktop notes |
| 5d-3 | Organize: execute, manifest, undo | DONE | MoveFileExW flags 0, JSONL manifest per target, reverse-order undo; 296 tests total |
| 5d-4 | Organize: UI | DONE | segmented control, plain-words preview, honest progress, report, undo; read-only live scan of real Desktop/Downloads OK |
| 5e | Quick Launch | DONE | allowlist ids, ShellExecuteExW, PowerShell flavor, PC Manager installed-path LIVE-VERIFIED |
| 5f | Command Tips | DONE | all 11 commands verified on this machine; badge decisions recorded below |
| 5g | Final polish + verification | DONE | placeholder removed, order verified, UI_TEXT + SYSTEM_MANAGER docs written, audit clean |
| 5h-1 | Organize: pure logic (modes) | DONE | Mode/DateSource/Granularity, size buckets, EXIF parser, calendar math; 13 mode tests |
| 5h-2 | Organize: dates, EXIF, sizes in scan | DONE | EXIF via kamadak-exif (Original→Digitized, raw ASCII, never plain DateTime); cloud-only skipped BEFORE any read; scan progress events; fixture tests via experimental::Writer |
| 5h-3 | Organize: engine + manifest integration | DONE | plan-carried categories; run-line mode (missing=format, old manifests undo); Desktop refusal in Folder tab; Date/Size run+undo tests |
| 5h-4 | Organize: UI restructure | DONE | two tabs, Sort by selector, date options, mode sentences, option changes clear the preview |
| 5h-5 | Final gates + docs | DONE | UI_TEXT + SYSTEM_MANAGER + DECISIONS + README updated; audit clean |
(Status values: NOT STARTED / IN PROGRESS / DONE / BLOCKED — waiting for owner)

## Gates (last run)
- npm run build: PASS (2026-11-06, step 5g — final)
- cargo fmt --check: PASS
- cargo clippy -- -D warnings: PASS
- cargo test (count): 309 passed, 0 failed (5 ignored live tests also pass on demand)
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

### Power values, 2026-11-06, BEFORE any Awake work (powercfg -query SCHEME_CURRENT)
- Active scheme: 381b4222-f694-41f0-9685-ff5bb260df2e (Balanced)
- STANDBYIDLE (sleep after): AC 0x00000000 (0 = Never), DC 0x00000384 (900 s)
- HIBERNATEIDLE (hibernate after): AC 0x00000000 (0 = Never), DC 0x00002a30 (10800 s)
- VIDEOIDLE (turn off display after): AC 0x00000000 (0 = Never), DC 0x0000012c (300 s)
- Desktop machine (no battery in use; DC values present but inactive).
- Rule: after every live test, re-run the queries and confirm these values are unchanged.

### Desktop Icons registry values, 2026-11-06, BEFORE any 5c live test
Key: HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\HideDesktopIcons\NewStartPanel
- {20D04FE0-3AEA-1069-A2D8-08002B30309D} (This PC) = REG_DWORD 0x0 (present, shown)
- {645FF040-5081-101B-9F08-00AA002F954E} (Recycle Bin) = ABSENT (default: shown)
- {5399E694-6CE5-4D6D-8792-F8F34E1E2E6F} (Control Panel) = ABSENT (default: hidden)
- {F02C1A0D-BE21-4350-88B0-7367FC96EF3C} (Network) = ABSENT (default: hidden)
- {59031a47-3f72-44a7-89c5-5595fe6b30ee} (User's Files) = ABSENT (default: hidden)
- Only ONE value existed before any 5c work (This PC). The live test restores values AND absence exactly (RegDeleteValueW is used ONLY by the test's restore step for values the test itself created).

## Verified facts ([VERIFY] items)
- ARM64 check command = `cargo check --target aarch64-pc-windows-msvc` (root DECISIONS.md, commit fbb413d; target present in rustup).
- `lucide-react` 1.66.0 has `MonitorCog` (icon for the System Manager tile).
- Power GUIDs (2026-11-06, powercfg -aliases): SUB_SLEEP 238c9fa8-0aad-41ed-83f4-97be242c8f20, STANDBYIDLE 29f6c1db-86da-48c5-9fdb-f2b67b1f44da, HIBERNATEIDLE 9d7815a6-7ee4-497e-8888-515a05f02364, SUB_VIDEO 7516b95f-f776-4464-8c53-06167f40cc99, VIDEOIDLE 3c0bc021-c8a8-4e07-a973-6b14cbcb2b7e — ALL match the spec table exactly.
- Per-user power writes need NO admin on this machine (live enable succeeded as normal user; no UAC).
- Reading a NONEXISTENT scheme via PowerReadACValueIndex returns ERROR_FILE_NOT_FOUND (2) — the scheme_exists probe is correct (live probe test).
- Live tests (cargo test -- --ignored, 2026-11-06): enable set all six to Never and read-back verified; disable restored the exact snapshot; a manual change to sleep-ac 1200 s was detected by reconcile, KEPT, and the other five keys restored. G9 final check: all six values equal the snapshot afterwards.
- Live tests are serialized with a shared mutex (they all change the same real power scheme; parallel runs race).
- windows-sys 0.61 quirks learned: GUID fields are lowercase (data1…data4); Power* functions are gated behind the Win32_System_Registry feature (HKEY param), not Win32_System_Power alone; LocalFree lives in Win32::Foundation; PowerWriteDCValueIndex exists (needs explicit import).
- Crash simulation (kill app → restart → resumed banner) is an OWNER CHECKLIST item (needs the real UI); the backend logic is covered by unit tests (app-start reconcile → resumed flag / timer-while-closed wording).
- Desktop Icons CLSIDs (2026-11-06): This PC CLSID CONFIRMED by an existing registry value on this machine ({20D04FE0-…} = 0x0). The other four CLSIDs match the spec table and the live round trip wrote/read them correctly, but the DECISIVE per-icon verification (Windows Settings ↔ app mirroring) is the OWNER's checklist test in 5c.
- Live desktop-icons test (cargo test -- --ignored): This PC shown → read-back shown → hidden → read-back hidden; Recycle Bin explicit write verified; Drop guard restored values AND absence exactly (This PC back to 0x0, others absent — verified with reg query afterwards).
- Desktop refresh layers shipped: layer 1 = one SHCNE_ASSOCCHANGED (SHCNF_IDLIST | SHCNF_FLUSHNOWAIT); layer 2 (const SEND_DESKTOP_VIEW_REFRESH=true, easy to disable) = PostMessageW WM_COMMAND 0x7103 to the DESKTOP's SHELLDLL_DefView under Progman/WorkerW only (not CabinetWClass — this section changes only the desktop). Which layer actually repaints instantly is an OWNER observation; the UI carries the F5 note.
- Organize read-only live scan (cargo test -- --ignored --nocapture, 2026-11-06): Desktop resolved to C:\Users\EhsanRX\Desktop (7 files would move into 2 folders, 1 skipped, OneDrive: false); Downloads to C:\Users\EhsanRX\Downloads (6 files into 4 folders, 0 skipped, OneDrive: false). No writes performed anywhere (G9).
- windows-sys 0.61: FOLDERID_Desktop/FOLDERID_Downloads are GUID consts in Win32::UI::Shell; SHGetKnownFolderPath returns HRESULT; MoveFileExW flags 0 = plain rename that cannot replace; FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS/RECALL_ON_OPEN/OFFLINE all present in Win32::Storage::FileSystem.
- Quick Launch PC Manager (2026-11-06): PC Manager IS INSTALLED on this machine (Microsoft.MicrosoftPCManager_3.23.0.0_x64__8wekyb3d8bbwe) — the spec's "not installed, mark NOT live-tested" case did NOT apply. Live-verified END TO END: Repository\Packages layout confirmed (shape Name_Version_Arch_ResourceId_PublisherId, resource id may be EMPTY → double underscore); PackageRootFolder value name confirmed on a real package (REG_SZ, e.g. C:\Program Files\Mp3tag); PC Manager manifest has FOUR <Application> entries and the FIRST (Id="App", MSPCManager.exe) is the main app; `explorer.exe shell:AppsFolder\Microsoft.MicrosoftPCManager_8wekyb3d8bbwe!App` LAUNCHED THE APP (MSPCManager.exe seen in tasklist).
- PowerShell on this machine: PowerShell 7 NOT installed → the caption shows "Windows PowerShell" (fallback path System32\WindowsPowerShell\v1.0\powershell.exe). The pwsh-detection paths are unit-tested, not all live.
- ERROR_CANCELLED = 1223 (user dismissed UAC) handled as a neutral "Cancelled." message; admin variants NOT auto-launched in tests (G9) — owner tests them.
- windows-sys additions: GetSystemDirectoryW/GetWindowsDirectoryW need Win32_System_SystemInformation; SHELLEXECUTEINFOW has a Default impl (feature-gated on Win32_System_Registry); ShellExecuteExW with SEE_MASK_NOASYNC|SEE_MASK_FLAG_NO_UI returns errors instead of system dialogs.
- Command Tips verification (2026-11-06, non-admin shell on Win11 Pro 26300):
  * winget upgrade — RUN, works (real list of available updates shown). winget upgrade --all — syntax via --help (-r,--recurse,--all); NOT executed (would update apps).
  * tree "<folder>" /f — RUN on a temp tree, works.
  * powercfg /batteryreport /output — RUN with a temp output, works, non-admin. %USERPROFILE% EXPANDS IN CMD, STAYS LITERAL IN POWERSHELL (verified) → shell badge = Command Prompt, exactly as the spec suspected.
  * sfc — even /? demands admin ("You must be an administrator...") → Needs administrator = TRUE. DISM — error 740 "Elevated permissions are required" even for /Online /Cleanup-Image /? on this build → syntax is the documented standard, admin badge TRUE.
  * ipconfig /flushdns — RUN → "The requested operation requires elevation." ON THIS BUILD → admin badge = TRUE (reality overruled the table's guess).
  * systeminfo — RUN, works. Get-Volume — RUN, works (HealthStatus/SizeRemaining/Size). Get-FileHash — RUN on a temp file, SHA256. Get-Process | Sort-Object CPU -Descending | Select-Object -First 10 — RUN, works.
  * netsh wlan show profiles — RUN (profile NAMES only, no key output). netsh wlan show profile /? — syntax confirmed (name=, key=clear documented). key=clear NEVER executed — no Wi-Fi password printed or saved anywhere.
  * The spec's tip-10 note was truncated in the pasted spec ("In the UI show a real `…"); interpreted as showing the pipeline as ONE copyable line — implemented that way, flagged for 5g wording review.

## Open issues / questions for the owner
- Prompt 5h (2026-11-07): kamadak-exif 0.6.2 added — license BSD-2-Clause (its one dependency
  mutate_once is also BSD-2-Clause), dependency tree contains NO network-capable crates.
  windows-sys feature Win32_System_Time added (FileTimeToSystemTime + SystemTimeToTzSpecificLocalTime
  for DST-correct LOCAL year/month; the pure UTC math is unit-tested and cross-checked).
  The Organize scan now runs on a blocking thread with progress events ("Scanning… N files checked").
  The manifest run line carries "mode" (missing = format, old manifests keep undoing — tested).
  EXIF order: DateTimeOriginal then DateTimeDigitized; the plain DateTime tag is NEVER used (tested).
  The spec's "Date not found" applies in PhotoTaken mode to every non-photo and to files whose
  Windows date is unavailable; in Created/Modified modes a missing OS date is treated the same.
- ALL STEPS 5a-5g ARE DONE. The owner now reviews wording in docs/SYSTEM_MANAGER_UI_TEXT.md,
  hand-tests 5a-5f with the per-step checklists, and pushes personally. Wording edits are
  applied to the code as a follow-up after the owner marks them.

## Dependencies / permissions / windows-sys features added so far
- Cargo: tauri-plugin-notification = "2" (Awake best-effort native toast; Rust-side only, no webview permission granted, no network crates in tree).
- Cargo: serde_json = "1" moved from dev-dependencies to dependencies (Awake state file).
- windows-sys features: Win32_System_Power, Win32_System_Registry, Win32_Security (RegCreateKeyExW is gated on it — SECURITY_ATTRIBUTES param).
- Tauri capabilities: UNCHANGED (still core:default only). Events (awake://*) need no extra permission. No CSP change.
