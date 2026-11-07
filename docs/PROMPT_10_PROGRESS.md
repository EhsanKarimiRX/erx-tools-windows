# Prompt 10 — First release (v0.2.0): progress
Last updated: 2026-11-07 20:35
## How to resume
Read docs/PROMPT_10_SPEC.md (full prompt). Continue at the first step not DONE. Never git push, never tag, never publish.
## Status
| Step | Description | Status | Notes |
|---|---|---|---|
| 10a | Setup, version 0.2.0, NSIS settings, static CRT, prerequisites | DONE | Commit d701c78; all gates green; STOP after this step per spec |
| 10b | Third-party licenses (non-blocking) | SKIPPED — MPL-2.0 crates in Rust dependency tree (see Deviation notes) | cargo-about 0.9.3 installed and works; retry recipe recorded |
| 10c | Release build script | DONE | scripts/build-release.mjs (reference impl used verbatim); dry-run verified |
| 10d | Build four installers | IN PROGRESS | x64 both variants OK; ARM64 needed a linker-discovery fix (see Deviation notes) — full script re-run for final set |
| 10e | Verify installers | NOT STARTED | |
| 10f | RELEASING.md, release notes, README | NOT STARTED | |
| 10g | Final verification | NOT STARTED | |
(Status: NOT STARTED / IN PROGRESS / DONE / BLOCKED — waiting for owner / SKIPPED — reason)
## Gates (last run)
2026-11-07, after 10a changes, with forced build-script re-run (`cargo clean -p erx-tools` first):
- `npm run build` → exit 0
- `cargo fmt --check` → exit 0
- `cargo clippy -- -D warnings` → exit 0
- `cargo test` → exit 0, **417 passed, 0 failed, 20 ignored** (baseline ≥374 met; incl. Prompt 9b version-consistency tests, 12/12 via `cargo test version` right after the bump)
- `cargo check --target aarch64-pc-windows-msvc` → exit 0
Log: target/p10-gates.log (git-ignored).
## Prerequisites found (10a-2)
1. **Rust targets** (`rustup target list --installed`): `aarch64-pc-windows-msvc` and `x86_64-pc-windows-msvc` BOTH installed. ✓
2. **ARM64 linker/libraries**: vswhere query from the spec (`-requires Microsoft.VisualStudio.Component.VC.Tools.ARM64`) printed NOTHING (exit 0). vswhere JSON shows the installation `C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools` (VS 18 Build Tools, installationVersion 18.11.12328.437) reporting **0 packages** — the component query cannot see into this VS 18 installation. Second check (folder search) FOUND the ARM64 toolset:
   - Toolset `14.44.35207`: COMPLETE — `lib/arm64` has 84 files incl. `libcmt.lib` and `msvcrt.lib`; `bin/Hostx64/arm64/` contains `cl.exe` and `link.exe`.
   - Toolset `14.51.36231`: PARTIAL — `lib/arm64` only 29 files, no CRT libs.
   → Conclusion: ARM64 building should work; the definitive test is the real ARM64 build in 10d.
3. **Node** v26.12.1 (≥18 ✓), **npm** 12.0.2, **Python** 3.10.11. ✓
4. **Tauri CLI** works: `npm run tauri -- --version` → tauri-cli 2.12.4. ✓
5. **Existing installation**: NONE found. PowerShell `Get-ItemProperty 'HKCU:\...\Uninstall\*'` with DisplayName -like '*ERX*' → empty; folder `%LOCALAPPDATA%\ERX Tools for Windows` does not exist. (A raw `reg query /s /f` from Git Bash failed due to shell flag mangling; the PowerShell check covers the same ground.) → 10e-5 install smoke test CAN run.
## Verified facts
- 10a-1: fresh start (no progress/spec file existed before). `git status` clean; `git status -sb` shows `## main...origin/main` with no ahead/behind → local main equals origin/main at 9e054a7 (Prompt 9e). Spec saved verbatim to docs/PROMPT_10_SPEC.md (415 lines).
- 10a-3: `npm version 0.2.0 --no-git-tag-version` changed ONLY package.json + package-lock.json (git diff --stat confirms), no tag created. Cargo.toml `[package] version` and tauri.conf.json `version` set to 0.2.0. Cargo.lock diff = the app's package entry only (`erx-tools` 0.1.0 → 0.2.0).
- 10a-4 (schema verification against `node_modules/@tauri-apps/cli/config.schema.json`, tauri-cli 2.12.4):
  - `bundle.targets` → BundleTarget: "all" OR array of BundleType {deb,rpm,appimage,msi,nsis,app,dmg} → `["nsis"]` valid.
  - `bundle.windows` → exists; props include `webviewInstallMode` and `nsis`.
  - `webviewInstallMode.type` enum: skip / downloadBootstrapper / embedBootstrapper / offlineInstaller / fixedRuntime → both used values valid; `silent` boolean (default true).
  - `nsis.installMode` enum: currentUser / perMachine / both → "currentUser" valid (also the schema default). `nsis.languages`: array (default ["English"]). `nsis.displayLanguageSelector`: boolean (default false). `nsis.compression` enum: zlib / bzip2 / lzma / none → "lzma" valid (also the schema default).
  - `bundle.resources`: array of paths, globs allowed, relative to src-tauri, structure preserved under $RESOURCE → for 10b, `../THIRD_PARTY_LICENSES.md` is the right syntax (verified early while the schema was open).
  - Merged into tauri.conf.json keeping icon/publisher/copyright/shortDescription/category; JSON re-validated with python json.load.
- 10a-5: `npm run tauri build -- --help` shows `-c, --config <CONFIG>` ("JSON strings or paths to JSON, JSON5 or TOML files to merge with the default configuration file… Configurations are merged in the order they are provided, which means a particular value overwrites previous values when a config key-value pair conflicts."). Created `src-tauri/release-configs/webview2-download.json` (downloadBootstrapper) and `webview2-offline.json` (offlineInstaller), both silent:true.
- 10a-6: created `src-tauri/.cargo/config.toml` (folder did not exist before) with +crt-static rustflags for both windows-msvc targets.
- 10a-7: `.gitignore` got `release-artifacts/`; `target/` and `src-tauri/target` were already ignored.
## Verified facts (10c)
- 10c: Cargo's real build directory IS `src-tauri/target` (`cargo metadata` reports `target_directory: C:\dev\erxtools\src-tauri\target`; today's check/clippy/test artifacts are fresh there). The root `C:\dev\erxtools\target\` holds stale leftovers from an earlier layout and is git-ignored; no config file or env var redirects cargo. → The reference script's `nsisDir` paths (`src-tauri/target[/<triple>]/release/bundle/nsis`) are correct as written. Output file-name pattern (`_<version>_<arch>-setup.exe` suffix match) still to be confirmed on the first real build in 10d.
- 10c: `node scripts/build-release.mjs --dry-run` printed exactly 4 sections (x64 download / x64 offline / arm64 download / arm64 offline) with the right `npm run tauri build -- …` command lines and `RELEASE BUILD DONE`, exit 0, and created NO `release-artifacts/` folder.
- 10c: How the version-mismatch protection works (explained instead of breaking real files, per spec): at startup the script reads `tauri.conf.json` `version`, `package.json` `version`, and the first `version = "…"` in `src-tauri/Cargo.toml`; if any two differ it prints `RELEASE BUILD FAILED: version mismatch: tauri.conf.json=…, package.json=…, Cargo.toml=…` and exits 1 before building anything. All three are currently 0.2.0, so the guard passes silently.
## Open questions for the owner
- 10b: 5 Rust crates in the (Tauri-transitive) dependency tree are MPL-2.0, which the spec puts on the STOP list. If the owner ever decides MPL-2.0 is acceptable (it is a weak copyleft license commonly used by Mozilla projects; filing this as a question, not an action), 10b can be completed later via the retry recipe above. Nothing in v0.2.0 depends on this.
## Verified facts (10d, first run)
- x64 standard installer built and archived OK: release-artifacts/v0.2.0/ERX-Tools-0.2.0-x64-setup.exe (2,387,142 bytes ≈ 2.3 MB, build took ~3m18s for the release compile).
- **Tauri NSIS output name pattern verified** ([VERIFY] from 10c): `ERX Tools for Windows_0.2.0_x64-setup.exe` → script's `_${version}_${t.arch}-setup.exe` suffix match works as written.
- x64 offline variant FAILED at the WebView2 download step: `Downloading https://go.microsoft.com/fwlink/?linkid=2124701` → `failed to bundle project: 'Peer disconnected'`. Network failure during the ~100+ MB Microsoft offline-installer download. Per spec 10d-4: retrying ONCE (log: target/release-build-retry1.log).
- Release-only warning (cosmetic, non-fatal; gates unaffected): `unused import: std::time::Duration` in `src\tools\update_check\commands.rs:7` — appears only in release profile (used in debug builds, so dev-profile gates never see it). R3 forbids editing src/; owner may fix it in a source prompt.
## Verified facts (10d, continued)
- Retry 1: the WebView2 download succeeded on the second attempt ("Peer disconnected" was transient). Both x64 installers archived.
- ARM64 failure diagnosed (NOT the spec's missing-component case — the component IS installed): `link: missing operand after '\377\376'` → GNU coreutils 8.32 `link.exe` from Git Bash `/usr/bin/` ran instead of MSVC's linker, because rustc's MSVC discovery finds nothing in the VS 18 BuildTools install (vswhere reports 0 packages) and falls back to PATH, where coreutils shadows MSVC. Host x64 detection worked, so only cross-ARM64 was affected.
- Fix (in the R3-allowed file `src-tauri/.cargo/config.toml`): for `[target.aarch64-pc-windows-msvc]` pin `linker` to `...\MSVC\14.44.35207\bin\Hostx64\arm64\link.exe` and add `/LIBPATH` link-args for VC `lib\arm64` (14.44.35207), Windows SDK `10.0.26100.0\um\arm64` and `\ucrt\arm64`. Direct test build (`npm run tauri build -- --target aarch64-pc-windows-msvc --config …webview2-download.json`) → exit 0, app exe built, NSIS bundle produced (log target/p10-arm64-test.log).
## Deviation notes
- Spec command `cargo install cargo-about --locked` compiled cargo-about 0.9.3 but did NOT install the binary ("bin cargo-about requires the features: cli"). Re-ran with `cargo install cargo-about --locked --features cli` → installed, `cargo about --version` → cargo-about 0.9.3. Recorded here per R6.
- 10b SKIPPED per the spec's own STOP rule ("if it is MPL, LGPL, GPL, AGPL, SSPL or 'unknown', STOP this step and report the crate name and license to the owner"). `cargo about generate` reported exactly 5 crates under **MPL-2.0**, all reachable from Tauri's dependency tree:
  - cssparser 0.37.0
  - cssparser-macros 0.7.1
  - dtoa-short 0.3.5
  - option-ext 0.2.0
  - selectors 0.38.0
  No THIRD_PARTY_LICENSES.md was created; no `bundle.resources` entry added. The scratch `about.toml`/`about.hbs` were deleted again so `git status` stays clean.
  **Retry recipe (if the owner later accepts MPL-2.0):** in `src-tauri` run `cargo about init`, replace `about.toml` accepted list with ["MIT","Apache-2.0","BSD-2-Clause","BSD-3-Clause","ISC","Zlib","Unicode-3.0","Unicode-DFS-2016","0BSD","CC0-1.0","BSL-1.0","MPL-2.0"] plus `targets = ["x86_64-pc-windows-msvc","aarch64-pc-windows-msvc"]`, replace `about.hbs` with a Markdown template (sections: `### {{name}}`, used-by list of `{{crate.name}} {{crate.version}}`, fenced `{{text}}`), run `cargo about generate about.hbs`, then follow spec 10b-2..10b-4.
  Consequence for later steps: installer contains no license file (10e-3 expectation adjusted); README edit 5 of 10f-3 does NOT apply; release notes unchanged (spec: "If 10b was skipped, nothing else changes").
