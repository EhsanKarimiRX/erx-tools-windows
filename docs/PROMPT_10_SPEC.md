# PROMPT 10 — FIRST RELEASE (v0.2.0): INSTALLERS, CHECKS, DOCS
## ERX Tools for Windows · steps 10a to 10g

> **STATUS: READY TO BUILD.** The owner decided everything in section 2. Follow the steps in order. Do not skip ahead. **You prepare and verify the release files; the OWNER publishes them on GitHub by hand** (section 6 contains the exact steps you must print for him).

---

## 0. READ THIS FIRST — rules and resume protocol

**Rules (apply to ALL steps):**
- **R1 Never `git push`, never create git tags, never create a GitHub release.** The owner does all of that personally. Local commits only, message style as in `git log` (e.g. `Prompt 10a: version 0.2.0, NSIS settings`).
- **R2 Task list rule.** Before writing ANY final report, mark EVERY task in your task list as completed (use TaskUpdate), including the very last "report" task. The owner watches that checklist.
- **R3 Do not change app behavior.** Allowed files to change: `package.json`, `package-lock.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src-tauri/tauri.conf.json`, `src-tauri/.cargo/config.toml`, `src-tauri/release-configs/*`, `scripts/*`, `.gitignore`, `README.md`, `THIRD_PARTY_LICENSES.md`, `about.toml`/`about.hbs`, and files in `docs/`. **Do not edit anything in `src/` or `src-tauri/src/`.** If a source change seems necessary: STOP and ask the owner.
- **R4 Network.** The app's own code stays as is (its only network access is the update button). Build tooling may use the network: `tauri build` downloads the NSIS tools and (for the "offline" variants) Microsoft's WebView2 installer, and you may `cargo install` a developer tool. Never add a new dependency to the app's `Cargo.toml`/`package.json`.
- **R5 Never commit binaries.** `release-artifacts/` is git-ignored (10a). Before every commit run `git status` and make sure no `.exe`, `.msi` or other binary is staged.
- **R6 Do not invent facts.** Anything marked **[VERIFY]** must be checked on the real machine (read the schema, run the command, read the file). Numbers in this prompt that come from memory are hints. Say plainly what you verified and what you could not.
- **R7 English only.** Wording of release notes and README edits is given in this prompt; use it as written (fill only the `<…>` placeholders).
- **R8 Gates** (run when a step changes config/code files; all must pass; fix, do not suppress): `npm run build`; in `src-tauri`: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test` (baseline from Prompt 9: **at least 374 passed, 0 failed**; the Prompt 9b version-consistency test must pass); ARM64 check `cargo check --target aarch64-pc-windows-msvc`. **Caution learned in Prompt 8:** the Tauri build script does not watch icon/config files; after changing `tauri.conf.json` or icons, force a re-run with `cargo clean -p <package name>` (package-scoped) before the gates so they are real.
- **R9 Long commands.** Builds can take many minutes. If your shell tool has a time limit (commonly 10 minutes), run the command in the background with output redirected to a log file in `target/` and poll the log every minute until it shows the end marker. Never kill a build just because it is slow.
- **R10 Stop and ask** if something is not covered here and changes what the owner will see, or if a step seems to require breaking a rule.

**Resume protocol:**
1. At the start of every session check whether `docs/PROMPT_10_PROGRESS.md` exists. If yes: read it, run `git status` and `git log -5 --oneline`, continue at the first step not marked DONE (if one is IN PROGRESS, inspect `git diff` and finish it; never redo DONE steps). If no: fresh start with 10a.
2. First action of 10a: save the full text of THIS prompt, verbatim, to `docs/PROMPT_10_SPEC.md` and create `docs/PROMPT_10_PROGRESS.md` from Appendix A.
3. Update the progress file after every micro-step.
4. **STOPS:** STOP and wait for the owner after **10a**, after **10e** and after **10g**. Between the other steps continue without stopping. After a STOP never start the next step until the owner says "continue".

---

## 1. Context

- Project: Tauri 2 (Rust backend, React 19 + TypeScript frontend). Repo `C:\dev\erxtools`, public GitHub repo `EhsanKarimiRX/erx-tools-windows`, MIT license (the name "ERX Tools" and the logo are excluded from MIT, see `branding/NOTICE.md`). Everything up to Prompt 9 is pushed to GitHub. Current app version: **0.1.0**. **No GitHub release exists yet.**
- Tools in the app: Image Converter (incl. HEIC input via Windows codecs), QR Code Generator, Folder Icon Changer, System Manager (Awake, Desktop Icons, Organize, Quick Launch, Command Tips), Encrypted Notes (placeholder tile only). The home screen has an update button next to the version number (it only checks GitHub when clicked; implemented in Prompt 9; it ignores pre-releases and drafts and compares the release tag with the app version).
- Windows is the only target. Windows 11 x64 is the owner's machine and the only platform tested by hand. ARM64 compiles (`cargo check`) but has never been run. Windows 10 is untested.
- Earlier lesson: `cargo check` does NOT link. ARM64 installers need the ARM64 MSVC linker/libraries, which may not be installed (see 10a-2).

---

## 2. Decisions (made by the owner — do not reopen)

1. **First public release version: `0.2.0`** (tag `v0.2.0`). The tag must always equal `v` + the app version.
2. **Four installers** (NSIS `setup.exe`), per-user install (no administrator rights for the app itself):
   | File name | Architecture | WebView2 handling |
   |---|---|---|
   | `ERX-Tools-0.2.0-x64-setup.exe` | x64 | **Standard (small):** the installer downloads Microsoft's WebView2 only if the PC lacks it |
   | `ERX-Tools-0.2.0-x64-setup-with-webview2.exe` | x64 | **Offline:** WebView2 runtime is inside the installer (large) |
   | `ERX-Tools-0.2.0-arm64-setup.exe` | ARM64 | Standard (small) |
   | `ERX-Tools-0.2.0-arm64-setup-with-webview2.exe` | ARM64 | Offline (large) |
   plus `SHA256SUMS.txt` (SHA-256 of the four files).
3. **No code signing** (not possible for the owner). The release notes and README must say honestly that Windows SmartScreen (and possibly antivirus software) may show a warning.
4. **Remove the "Visual C++ Redistributable" prerequisite** by statically linking the C runtime (`+crt-static`), so the only real prerequisite left is WebView2.
5. **Third-party license notices** are shipped (file in the repo and inside the installer) — but this step is NON-BLOCKING (10b).
6. The GitHub release is **a normal release, NOT a pre-release** (the app's update check ignores pre-releases), and it is called "early beta" only in its title and text.
7. Honest limits to state everywhere: early beta; tested by hand on Windows 11 x64 only; ARM64 and Windows 10 untested; unsigned installers; HEIC needs Microsoft's extensions; Encrypted Notes is not available yet.

---

# 10a — Setup, version 0.2.0, installer settings, static CRT, prerequisites check

**10a-1. Resume setup.** Save the spec, create the progress file (Appendix A). Run `git status` (must be clean) and `git log -3 --oneline`; confirm local `main` equals `origin/main` (`git status -sb` shows no "ahead"). If the tree is not clean or main is ahead/behind: STOP and ask.

**10a-2. Prerequisite check (read-only; write every result into the progress file).**
1. Rust targets: `rustup target list --installed` — both `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc` must be listed.
2. ARM64 linker/libraries (needed to actually BUILD ARM64, not just check): run (Git Bash or PowerShell)
   `"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe" -products * -requires Microsoft.VisualStudio.Component.VC.Tools.ARM64 -property installationPath`
   If it prints a path → installed. If it prints nothing → NOT installed. Also search for an ARM64 MSVC folder as a second check: look under the VS/Build Tools install path for `VC\Tools\MSVC\*\lib\arm64`.
3. Node version (`node --version`; must be ≥ 18), `npm --version`, `python --version`.
4. Whether the Tauri CLI works: `npm run tauri -- --version`.
5. Whether this machine has an existing installation of the app (we will test installing it in 10e): look for a registry key under `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall` whose `DisplayName` contains "ERX Tools", and for the folder `%LOCALAPPDATA%\ERX Tools for Windows`. Report what you find.

**10a-3. Version bump to 0.2.0.**
- Run `npm version 0.2.0 --no-git-tag-version` (it must change only `package.json` and `package-lock.json`; confirm with `git diff --stat`; it must NOT create a git tag).
- Set `version = "0.2.0"` in `src-tauri/Cargo.toml` `[package]`.
- Set `"version": "0.2.0"` in `src-tauri/tauri.conf.json`.
- Run `cargo check` in `src-tauri` so `Cargo.lock` updates its own package version; confirm `git diff` shows only the version change in `Cargo.lock`.
- Run `cargo test version` — the Prompt 9b version-consistency test must pass.

**10a-4. Installer settings in `src-tauri/tauri.conf.json`.** Open the file and the official schema (`node_modules/@tauri-apps/cli/config.schema.json`, or the schema the file's `$schema` points to) and **[VERIFY]** each field name and allowed value below BEFORE writing it. Merge into the existing `bundle` object (keep `icon`, `publisher`, `copyright`, `shortDescription`, `category` and everything else already there):
```json
"bundle": {
  "targets": ["nsis"],
  "windows": {
    "webviewInstallMode": { "type": "downloadBootstrapper", "silent": true },
    "nsis": {
      "installMode": "currentUser",
      "languages": ["English"],
      "displayLanguageSelector": false,
      "compression": "lzma"
    }
  }
}
```
(`"targets": ["nsis"]` stops Tauri from also trying to build an MSI, which needs extra tools.) Do not change `productName`, `identifier`, window settings, CSP or capabilities.

**10a-5. Two override files** (create the folder `src-tauri/release-configs/`):
- `webview2-download.json`: `{ "bundle": { "windows": { "webviewInstallMode": { "type": "downloadBootstrapper", "silent": true } } } }`
- `webview2-offline.json`: `{ "bundle": { "windows": { "webviewInstallMode": { "type": "offlineInstaller", "silent": true } } } }`
**[VERIFY]** that `npm run tauri build -- --help` shows the option `-c, --config <CONFIG>` and that it merges the given JSON file over `tauri.conf.json` (it does a JSON merge, so only the keys given are replaced). Write the finding into the progress file.

**10a-6. Static C runtime.** Create `src-tauri/.cargo/config.toml` (if the file already exists, merge; do not overwrite other settings) with:
```toml
[target.x86_64-pc-windows-msvc]
rustflags = ["-C", "target-feature=+crt-static"]

[target.aarch64-pc-windows-msvc]
rustflags = ["-C", "target-feature=+crt-static"]
```
This will be checked for real in 10e (the built exe must not need `vcruntime140.dll`). If the gates below fail with linker errors caused by this flag: revert this file, record the exact error, and continue (the release notes will then list the "Microsoft Visual C++ Redistributable" as a requirement — tell the owner in the report).

**10a-7. `.gitignore`.** Add the line `release-artifacts/` (if missing). Confirm `target/` is already ignored.

**10a-8. Gates** (R8, including the forced build-script re-run). The first build after adding `+crt-static` recompiles everything; this takes a while.

**10a-9. Commit and report.** One local commit: `Prompt 10a: version 0.2.0, NSIS bundle settings, static CRT`. Update the progress file; mark all tasks completed; **STOP** and report (Appendix B). In the report give the prerequisite results from 10a-2 clearly, and if the ARM64 MSVC component is NOT installed, print these exact instructions for the owner:
> "To build the ARM64 installers, please install one extra Visual Studio component: Start menu → **Visual Studio Installer** → find **Build Tools** (or Visual Studio) → **Modify** → tab **Individual components** → search "ARM64" → tick **MSVC v143 - VS 2022 C++ ARM64/ARM64EC build tools (Latest)** → **Modify** and wait for it to finish. Then tell me to continue."

---

# 10b — Third-party license notices (NON-BLOCKING)

Goal: a file `THIRD_PARTY_LICENSES.md` in the repo root listing the open-source components that end up in the app, with their license texts, and the same file shipped inside the installer. **If this step cannot be finished with reasonable effort, SKIP it, write the reason in the progress file and in the final report, and continue with 10c (the release is not blocked).**

1. **Rust crates.** Install the developer tool once: `cargo install cargo-about --locked` (network; takes several minutes). In `src-tauri` run `cargo about init` (creates `about.toml` and `about.hbs`; keep them in the repo). In `about.toml` set `accepted` to this permissive list: `["MIT", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib", "Unicode-3.0", "Unicode-DFS-2016", "0BSD", "CC0-1.0", "BSL-1.0"]` and set `targets = ["x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc"]` so only crates used on Windows are listed. Run `cargo about generate about.hbs` (adapt the template so the output is Markdown: for each license text, its name, the list of crates using it with versions, and the full license text). If it reports a crate under a license NOT in the accepted list: do NOT add it blindly. If it is another permissive license, add it and note it; if it is MPL, LGPL, GPL, AGPL, SSPL or "unknown", STOP this step and report the crate name and license to the owner (skip the step, continue).
2. **JavaScript packages that ship in the app** (production dependencies only: the ones in `package.json` `dependencies` and their transitive production dependencies; NOT dev tools like vite/typescript). Use `npm ls --omit=dev --all --parseable` to list them, then read each package's `package.json` (`name`, `version`, `license`) and its LICENSE file text (`LICENSE`, `LICENSE.md`, `license` …). Write a small one-off Node script at `scripts/gen-third-party-js.mjs` (keep it in the repo) that prints a Markdown section for them. Same rule for non-permissive licenses (STOP/skip).
3. **Assemble** `THIRD_PARTY_LICENSES.md` in the repo root: a short header ("ERX Tools for Windows is MIT-licensed. It includes the following third-party components, each under its own license. Generated for version 0.2.0."), then a "Rust crates" section, then a "JavaScript packages" section. Also list in the header: "Windows components such as WebView2 and the HEIF/HEVC codecs are provided by Microsoft and are not part of this file."
4. **Ship it inside the installer.** In `tauri.conf.json` add the file as a bundle resource so it is installed next to the exe. **[VERIFY]** the exact `bundle.resources` syntax in the schema (Tauri 2 accepts an array of paths or an object `{ "source": "target" }`). The source path is relative to `src-tauri`, so it is `../THIRD_PARTY_LICENSES.md`. Confirm after the build in 10e that the file is present in the installed folder.
5. Gates (R8). Commit: `Prompt 10b: third-party license notices`. Continue to 10c without stopping.

---

# 10c — Release build script

Create `scripts/build-release.mjs` (Node ESM, no dependencies) and add `"release:build": "node scripts/build-release.mjs"` to `package.json` `scripts`. **This is a reference implementation. Read it, check it, adapt it where your checks show a difference ([VERIFY] the Tauri output folder and file-name pattern on your first real build), and explain any change in the progress file.**

```js
// Builds the four Windows installers + SHA256SUMS.txt into release-artifacts/v<version>/.
// Usage: npm run release:build            (real build, takes a long time)
//        node scripts/build-release.mjs --dry-run   (prints what it would do)
import { spawnSync } from "node:child_process";
import { readFileSync, existsSync, mkdirSync, rmSync, copyFileSync, readdirSync, writeFileSync, statSync } from "node:fs";
import { createHash } from "node:crypto";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(fileURLToPath(new URL("..", import.meta.url)));
const dryRun = process.argv.includes("--dry-run");
const fail = (msg) => { console.error(`\nRELEASE BUILD FAILED: ${msg}`); process.exit(1); };

const tauriConf = JSON.parse(readFileSync(join(root, "src-tauri", "tauri.conf.json"), "utf8"));
const pkg = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
const cargoToml = readFileSync(join(root, "src-tauri", "Cargo.toml"), "utf8");
const cargoVersion = /^version\s*=\s*"([^"]+)"/m.exec(cargoToml)?.[1];
const version = tauriConf.version;
if (pkg.version !== version || cargoVersion !== version) {
  fail(`version mismatch: tauri.conf.json=${version}, package.json=${pkg.version}, Cargo.toml=${cargoVersion}`);
}

const nsisDir = (...parts) => join(root, "src-tauri", "target", ...parts, "release", "bundle", "nsis");
const targets = [
  { arch: "x64", args: [], dir: nsisDir() },
  { arch: "arm64", args: ["--target", "aarch64-pc-windows-msvc"], dir: nsisDir("aarch64-pc-windows-msvc") },
];
const variants = [
  { suffix: "", config: join("src-tauri", "release-configs", "webview2-download.json") },
  { suffix: "-with-webview2", config: join("src-tauri", "release-configs", "webview2-offline.json") },
];

const outDir = join(root, "release-artifacts", `v${version}`);
if (!dryRun) mkdirSync(outDir, { recursive: true });
const produced = [];

for (const t of targets) {
  for (const v of variants) {
    const outName = `ERX-Tools-${version}-${t.arch}-setup${v.suffix}.exe`;
    const args = ["run", "tauri", "build", "--", ...t.args, "--config", v.config];
    console.log(`\n=== ${outName} ===\nnpm ${args.join(" ")}`);
    if (dryRun) { produced.push(outName); continue; }
    // Remove stale installers first: both variants of one architecture produce the SAME file name.
    if (existsSync(t.dir)) for (const f of readdirSync(t.dir)) if (f.endsWith("-setup.exe")) rmSync(join(t.dir, f));
    const r = spawnSync("npm", args, { cwd: root, stdio: "inherit", shell: true });
    if (r.status !== 0) fail(`${outName}: tauri build exited with code ${r.status}`);
    const wanted = `_${version}_${t.arch}-setup.exe`;
    const found = existsSync(t.dir) ? readdirSync(t.dir).filter((f) => f.endsWith(wanted)) : [];
    if (found.length !== 1) fail(`${outName}: expected exactly one installer ending with ${wanted} in ${t.dir}, found ${found.length}`);
    copyFileSync(join(t.dir, found[0]), join(outDir, outName));
    produced.push(outName);
  }
}

if (!dryRun) {
  const sha = (p) => createHash("sha256").update(readFileSync(p)).digest("hex");
  const lines = produced.map((n) => `${sha(join(outDir, n))}  ${n}`);
  writeFileSync(join(outDir, "SHA256SUMS.txt"), lines.join("\n") + "\n");
  console.log("\nFiles:");
  for (const n of produced) console.log(`  ${n}  ${(statSync(join(outDir, n)).size / 1048576).toFixed(1)} MB`);
  console.log(`\nOutput folder: ${outDir}`);
}
console.log("\nRELEASE BUILD DONE");
```

Steps:
1. Create the script and the npm alias.
2. Run `node scripts/build-release.mjs --dry-run`; it must print four sections and `RELEASE BUILD DONE` and create nothing.
3. Test the version-mismatch protection without changing real files: copy the script logic is not needed — instead temporarily run it with an environment where you edit nothing; simply explain in the progress file how the check works (do NOT break the real files to test it).
4. Local commit: `Prompt 10c: release build script`. Continue to 10d without stopping.

---

# 10d — Build all four installers

1. Make sure nothing else is using the build folders (the dev app must not be running). Check `git status` is clean.
2. Run `npm run release:build` following R9 (background + log file `target/release-build.log`, poll until the log contains `RELEASE BUILD DONE` or `RELEASE BUILD FAILED`). Expect: the first `tauri build` downloads NSIS tools; the offline variants download Microsoft's WebView2 installer for the matching architecture (about 100+ MB each); four full packaging runs.
3. **If the ARM64 build fails** with linker errors mentioning missing ARM64 libraries or `link.exe`/`LNK1104`/`msvcrt`: this means the ARM64 MSVC component is not installed (10a-2). Do NOT try to work around it. Record the exact error, mark 10d as BLOCKED in the progress file, make sure the x64 installers (if they were built before the failure) are kept, and STOP with a report that repeats the installation instructions from 10a-9. After the owner installs the component and says "continue", re-run the whole script.
4. **If a download fails** (NSIS tools or WebView2 offline installer): record the exact message, retry ONCE; if it fails again, mark BLOCKED, STOP and report.
5. On success the folder `release-artifacts/v0.2.0/` contains exactly five files: the four installers named as in section 2 and `SHA256SUMS.txt`. List them with sizes. Continue to 10e.

---

# 10e — Verify the installers (then STOP)

Write small scratch scripts into `target/` (git-ignored) instead of long one-liners, to avoid shell quoting problems. All results go into the progress file ("Verified facts").

**10e-1. Architecture of the built programs.** The built app executables are `src-tauri/target/release/<name>.exe` (x64) and `src-tauri/target/aarch64-pc-windows-msvc/release/<name>.exe` (ARM64), where `<name>` is the package name in `Cargo.toml` (**[VERIFY]**). Write `target/pe_check.py`:
```python
import struct, sys
p = sys.argv[1]
d = open(p, "rb").read(4096)
o = struct.unpack_from("<I", d, 0x3C)[0]
assert d[o:o+4] == b"PE\0\0", "not a PE file"
print(p, "machine = 0x%04X" % struct.unpack_from("<H", d, o + 4)[0])
```
Expected: `0x8664` for x64 and `0xAA64` for ARM64. If an ARM64 build was skipped, say so.

**10e-2. No Visual C++ runtime dependency.** Write `target/crt_check.py`: read each app exe (both architectures) as bytes, lowercase it, and report whether it contains `vcruntime140`, `vcruntime140_1` or `msvcp140`. Expected: none present (the static CRT worked). If present: report it clearly (do not try to "fix" it in this step); the README/release notes must then name the "Microsoft Visual C++ Redistributable" as a requirement — tell the owner.

**10e-3. Version metadata.** Write `target/versioninfo.ps1` that prints `(Get-Item <path>).VersionInfo | Format-List ProductName, ProductVersion, FileVersion, CompanyName, LegalCopyright, FileDescription` and run it (`powershell -NoProfile -ExecutionPolicy Bypass -File target/versioninfo.ps1 <path>`) for the x64 app exe and the x64 standard installer. Expected: product name "ERX Tools for Windows" and version 0.2.0; report what is shown for company/copyright/description.

**10e-4. Checksums and sizes.** Recompute SHA-256 of each of the four installers (`Get-FileHash -Algorithm SHA256` in a scratch .ps1, or Python `hashlib`) and compare with `SHA256SUMS.txt`: all four must match. Then check that each "with-webview2" file is at least 50 MB larger than its standard sibling; if not, the offline setting was not applied (report as a problem). Report all eight sizes.

**10e-5. Install / start / uninstall smoke test of the x64 STANDARD installer** (only if 10a-2 found NO existing installation; otherwise skip and say so). Write `target/install_test.ps1` doing, in order, and printing each result:
1. Confirm no running process of the app and no uninstall registry entry containing "ERX Tools" under `HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall`.
2. `Start-Process <x64 standard installer> -ArgumentList "/S" -Wait` (silent install).
3. Check: the install folder exists (expected `$env:LOCALAPPDATA\ERX Tools for Windows` — **[VERIFY]**; if elsewhere, find it via the registry `InstallLocation`/`UninstallString`), it contains the app exe, `uninstall.exe` and (if 10b succeeded) `THIRD_PARTY_LICENSES.md`; the uninstall registry entry exists; a Start Menu shortcut named like "ERX Tools" exists under `$env:APPDATA\Microsoft\Windows\Start Menu\Programs` (search recursively).
4. Start the installed exe, wait 6 seconds, confirm the process is running and `Responding` is true, then stop it (`Stop-Process`).
5. Run the uninstaller silently: `& "<install folder>\uninstall.exe" /S` and poll up to 60 seconds until the install folder is gone (the NSIS uninstaller copies itself and returns immediately).
6. Check: install folder gone, registry entry gone, Start Menu shortcut gone. Report what is left (for example `%APPDATA%\com.erxtools.windows` app data is normally kept by a silent uninstall — report whether it exists).
If any step fails: do not repeat blindly; report the exact outcome. Do NOT run the offline variants (they are large) and do NOT run ARM64 installers (this PC is x64).

**10e-6. Commit** only if you changed tracked files in this step (otherwise nothing to commit). Update the progress file; mark all tasks completed; **STOP** and report (Appendix B). Include the owner's manual test checklist:
1. Open `C:\dev\erxtools\release-artifacts\v0.2.0\` and look at the five files and their sizes.
2. Double-click `ERX-Tools-0.2.0-x64-setup.exe`. Windows may show a blue "Windows protected your PC" window because the installer is not signed: click **More info** → **Run anyway**. Go through the installer. It should not ask for an administrator password for ERX Tools itself.
3. Start ERX Tools from the Start menu. Check: the new logo, the version text shows **0.2.0**, a tool opens.
4. Click the round arrow button next to the version: since the release is not published yet, it will say that no published release was found (gray). That is expected now.
5. Uninstall it: Settings → Apps → Installed apps → ERX Tools for Windows → Uninstall.
6. Tell the consultant if anything looked wrong.

---

# 10f — Documents: RELEASING.md, release notes, README

**10f-1. `docs/RELEASING.md`** — the checklist for every future release, written in plain English for a non-programmer owner who gives instructions to an AI agent. Content (write it fully, with the exact commands):
1. **Prerequisites:** Visual Studio Build Tools incl. the ARM64 component; Rust targets; Node; network for the first build; free disk space (several GB).
2. **Pick the version** (`MAJOR.MINOR.PATCH`). Tag = `v` + version. Never publish a release as "pre-release" (the app's update check ignores pre-releases).
3. **Bump the version in all three places** (and the lock files): `npm version X.Y.Z --no-git-tag-version`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, then `cargo check` in `src-tauri`; `cargo test version` must pass (it fails if the three versions differ).
4. **Write the release notes** `docs/releases/vX.Y.Z.md` (copy the previous file and edit it).
5. **Build:** `npm run release:build` (explain: builds four installers + checksums into `release-artifacts/vX.Y.Z/`; takes a long time; do not use the PC heavily meanwhile).
6. **Verify:** checksums, the x64 installer test on a PC, and what 10e checked.
7. **Commit and push** (`git push` by the owner).
8. **Publish on GitHub** — copy the steps from section 6 of this prompt (they must be identical, with the version made generic).
9. **After publishing:** install the new version and click the update button: it must turn green; send the checksums to anyone who asks.
10. **Troubleshooting:** ARM64 linker errors → install the ARM64 build tools component; WebView2 download failure → retry; SmartScreen warning is expected (unsigned); "update check says no release" → the release is a draft/pre-release or the tag does not match.
11. **Regenerating third-party license notices** (if 10b was done): the exact commands you used.

**10f-2. `docs/releases/v0.2.0.md`** — the text the owner pastes into the GitHub release description. Use EXACTLY this text, replacing only the `<…>` placeholders with the real values from 10d/10e (sizes in MB, rounded to whole numbers):

```markdown
**ERX Tools for Windows v0.2.0 — early beta**

This is the first public release of ERX Tools, a small, offline toolbox for Windows. It is an **early beta**: it is meant to be tried by real users, and feedback is very welcome.

## What's inside

- **Image Converter** — convert images in batches between JPEG, PNG, WebP, BMP, GIF, TIFF and HEIC/HEIF (iPhone photos; HEIC needs Microsoft's HEIF and HEVC extensions).
- **QR Code Generator** — text, links and Wi-Fi; PNG or SVG; custom colors and logo.
- **Folder Icon Changer** — recolor folder icons, with optional patterns.
- **System Manager** — Awake (keep the PC awake), Desktop Icons, Organize (by format, date or size, with Undo), Quick Launch and Command Tips.
- **Update check** — a round button next to the version number on the home screen. It asks GitHub for the newest version only when you click it. Nothing is downloaded or installed automatically.

## Which file should I download?

| Your PC | File | Size |
|---|---|---|
| Most PCs (Intel or AMD) | `ERX-Tools-0.2.0-x64-setup.exe` | <X> MB |
| Most PCs, installing without internet | `ERX-Tools-0.2.0-x64-setup-with-webview2.exe` | <X2> MB |
| Windows on ARM (for example Snapdragon laptops) | `ERX-Tools-0.2.0-arm64-setup.exe` | <A> MB |
| Windows on ARM, installing without internet | `ERX-Tools-0.2.0-arm64-setup-with-webview2.exe` | <A2> MB |

Not sure which one? Open **Settings → System → About** and look at **System type**: "x64-based processor" means x64; "ARM-based processor" means ARM64.

The small installers download Microsoft's WebView2 component during setup only if your PC does not already have it (Windows 11 and most up-to-date Windows 10 PCs have it). The "with-webview2" installers contain it, so they are much bigger but need no internet.

## Installing

1. Download the file for your PC and double-click it. ERX Tools installs for your user account and does not need administrator rights.
2. The installers are **not code-signed**, so Windows SmartScreen may show "Windows protected your PC". Click **More info**, then **Run anyway**. Some antivirus programs may also warn about unsigned programs.
3. Start ERX Tools from the Start menu.

## Check your download (optional)

`SHA256SUMS.txt` lists the SHA-256 checksum of each installer. In PowerShell, run `Get-FileHash .\<file name> -Algorithm SHA256` and compare the result with that file.

## Known limits

- Tested by hand on **Windows 11 (x64)** only. **Windows 10** and the **ARM64** installers have not been tested.
- The installers are not signed (see above).
- HEIC photos need Microsoft's *HEIF Image Extensions* and *HEVC Video Extensions* from the Microsoft Store. The app tells you if they are missing.
- After changing a folder icon, Windows can take up to about 45 seconds to show it.
- Encrypted Notes is not available yet (placeholder tile).
- If you use **Awake**, turn it off before uninstalling ERX Tools; otherwise your PC's sleep settings can stay on "Never" (you can fix them in Windows Settings → System → Power).

## Feedback

Please open an issue: https://github.com/EhsanKarimiRX/erx-tools-windows/issues — and say which Windows version you use and which installer you installed.
```
(If 10e showed that the Visual C++ Redistributable IS still needed, add to "Installing" step 1: "If the program does not start, install the Microsoft Visual C++ Redistributable.". If 10b was skipped, nothing else changes.)

**10f-3. `README.md` edits.** Open the README, find each quoted text EXACTLY, and replace ONLY these five places (if a quoted text is not found exactly, find the equivalent and report the difference; do not edit anything else):
1. The top note. Replace
`> **Early beta.** ERX Tools is new and is being tested with friends. There is no installer yet (see [Download](#download)), and so far it has only been tested on Windows 11 (see [Tested on](#tested-on)).`
with
`> **Early beta.** ERX Tools is new and is being tested with friends. Installers are on the [Releases page](https://github.com/EhsanKarimiRX/erx-tools-windows/releases) (see [Download](#download)). So far it has only been tested on Windows 11 (see [Tested on](#tested-on)).`
2. The Requirements bullet about WebView2. Replace
`- Microsoft Edge WebView2 Runtime (included with Windows 11 and most up-to-date Windows 10 PCs).`
with
`- Microsoft Edge WebView2 Runtime (included with Windows 11 and most up-to-date Windows 10 PCs). The standard installers download it during setup if it is missing; the "with-webview2" installers contain it.`
3. The whole text of the `## Download` section (everything between the `## Download` heading and the next heading). Replace it with:
```markdown
Download the installer for your PC from the [latest release](https://github.com/EhsanKarimiRX/erx-tools-windows/releases/latest):

| Your PC | File |
|---|---|
| Most PCs (Intel or AMD, "x64") | `ERX-Tools-<version>-x64-setup.exe` |
| Most PCs, installing without internet | `ERX-Tools-<version>-x64-setup-with-webview2.exe` |
| Windows on ARM | `ERX-Tools-<version>-arm64-setup.exe` |
| Windows on ARM, installing without internet | `ERX-Tools-<version>-arm64-setup-with-webview2.exe` |

- The installers are **not code-signed**, so Windows SmartScreen may warn you ("Windows protected your PC"): click **More info** → **Run anyway**.
- ERX Tools installs for your user account and does not need administrator rights.
- If you use **Awake**, turn it off before uninstalling, otherwise your PC's sleep settings can stay on "Never".
- Each release lists SHA-256 checksums so you can verify your download.
```
4. In the Development section, replace
`Building an installer is not set up yet (planned: NSIS setup `.exe` for x64 and ARM64, built with `npm run tauri build`).`
with
`Building the installers (x64 and ARM64, two variants each) is described in [docs/RELEASING.md](docs/RELEASING.md).`
5. Only if 10b succeeded: under `## License`, add a new last line: `Third-party components and their licenses are listed in [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).`
Check that every relative link in the changed parts resolves to an existing file.

**10f-4.** `npm run build` once (sanity). Local commit: `Prompt 10f: RELEASING.md, v0.2.0 release notes, README`. Continue to 10g.

---

# 10g — Final verification (then STOP)

1. Run all gates (R8) with the forced build-script re-run; the final `cargo test` count must be ≥ 374 with 0 failed.
2. `git status` must be clean and show NO binary files; `git log --oneline -8` lists the Prompt 10 commits; report how many commits local `main` is ahead of `origin/main`.
3. Confirm that `release-artifacts/v0.2.0/` still holds the five files and that `SHA256SUMS.txt` matches (recompute once more).
4. Confirm that the version appears as `0.2.0` in `package.json`, `package-lock.json` (top-level), `Cargo.toml`, `Cargo.lock` (the app's package entry) and `tauri.conf.json`.
5. Update the progress file (all DONE), mark all tasks completed, and print the owner's publishing steps from section 6 EXACTLY, then **STOP**.

---

## 6. Owner's publishing steps (print these in the 10g report, exactly)

> **Publishing v0.2.0 on GitHub** (you do this by hand; I never publish for you)
> 1. In PowerShell: `cd C:\dev\erxtools` then `git push` (this uploads the final commits to GitHub).
> 2. Open https://github.com/EhsanKarimiRX/erx-tools-windows/releases and click **Draft a new release**.
> 3. Click **Choose a tag**, type `v0.2.0`, and click **Create new tag: v0.2.0 on publish**. Leave the target branch as `main`.
> 4. **Release title:** `ERX Tools for Windows v0.2.0 (early beta)`.
> 5. **Description:** open `C:\dev\erxtools\docs\releases\v0.2.0.md` in Notepad, select all, copy, and paste it into the description box.
> 6. Drag these 5 files from `C:\dev\erxtools\release-artifacts\v0.2.0\` into the box that says "Attach binaries by dropping them here": the four `ERX-Tools-0.2.0-…-setup….exe` files and `SHA256SUMS.txt`. Wait until every file finishes uploading (the large ones take a while).
> 7. **Leave "Set as a pre-release" UNCHECKED** (the app's update button ignores pre-releases). Leave **"Set as the latest release"** checked. Do not tick "Create a discussion".
> 8. Click **Publish release** (not "Save draft").
> 9. Check: open the release page in a private/incognito window; you should see the 5 files. Download the small x64 installer from there and compare its checksum with `SHA256SUMS.txt` (`Get-FileHash` in PowerShell).
> 10. Install v0.2.0 from that download, open the app and click the round arrow button next to the version: it should turn **green** and say you have the latest version (0.2.0). If it turns red or stays gray, tell the consultant.
> 11. For future releases, ask the agent to follow `docs/RELEASING.md`.

---

## APPENDIX A — progress file template (`docs/PROMPT_10_PROGRESS.md`)

```markdown
# Prompt 10 — First release (v0.2.0): progress
Last updated: <date/time>
## How to resume
Read docs/PROMPT_10_SPEC.md (full prompt). Continue at the first step not DONE. Never git push, never tag, never publish.
## Status
| Step | Description | Status | Notes |
|---|---|---|---|
| 10a | Setup, version 0.2.0, NSIS settings, static CRT, prerequisites | NOT STARTED | |
| 10b | Third-party licenses (non-blocking) | NOT STARTED | |
| 10c | Release build script | NOT STARTED | |
| 10d | Build four installers | NOT STARTED | |
| 10e | Verify installers | NOT STARTED | |
| 10f | RELEASING.md, release notes, README | NOT STARTED | |
| 10g | Final verification | NOT STARTED | |
(Status: NOT STARTED / IN PROGRESS / DONE / BLOCKED — waiting for owner / SKIPPED — reason)
## Gates (last run)
## Prerequisites found (10a-2)
## Verified facts
## Open questions for the owner
```

## APPENDIX B — report format (after 10a, 10e, 10g)

1. What was done (3–6 lines). 2. Gates (build / fmt / clippy / test count / ARM64). 3. Files created/changed (tracked) and the installer files with sizes (untracked). 4. New dependencies/features (expected: none in the app; list dev tools you installed). 5. Verified facts and what could NOT be verified. 6. Owner checklist/steps (as specified in the step). 7. Known limits. 8. Questions for the owner. 9. Progress file updated: yes; commit hashes; commits ahead of origin; **Pushed: NO**; no tag created. 10. All tasks in the task list are marked completed. 11. "Waiting for the owner."