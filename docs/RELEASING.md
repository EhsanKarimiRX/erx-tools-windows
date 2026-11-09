# RELEASING — how to publish a new version of ERX Tools

This is the checklist for every future release. It is written for the owner, who gives instructions to an AI agent and does the GitHub steps personally. The agent never pushes, never creates git tags, and never publishes anything on GitHub — those are always owner steps.

## 1. Prerequisites

- **Visual Studio Build Tools**, including the **ARM64 component** ("MSVC v143 - VS 2022 C++ ARM64/ARM64EC build tools"). Start menu → Visual Studio Installer → Build Tools → Modify → Individual components → search "ARM64".
- **Rust targets** installed: run `rustup target list --installed` — both `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc` must appear.
- **Node.js** 18 or newer (`node --version`).
- **Network access** for the first build: `tauri build` downloads the NSIS tools once, and each "with-webview2" build downloads Microsoft's WebView2 installer (100–200 MB each).
- **Free disk space**: several GB for the build outputs.

## 2. Pick the version

Use the three-part form `MAJOR.MINOR.PATCH` (for example `0.2.1`). The git tag must always be exactly `v` + the version (for example `v0.2.1`). Never publish a release as "pre-release" — the app's update check ignores pre-releases.

## 3. Bump the version in all three places

Run these in order (replace `X.Y.Z` with the real version):

```
cd C:\dev\erxtools
npm version X.Y.Z --no-git-tag-version
```

Then set `version = "X.Y.Z"` in `src-tauri/Cargo.toml` (under `[package]`) and `"version": "X.Y.Z"` in `src-tauri/tauri.conf.json`. Then run `cargo check` inside `src-tauri` so `Cargo.lock` picks the new version up, and `cargo test version` — it fails if the three versions differ, so it must pass.

## 4. Write the release notes

Create `docs/releases/vX.Y.Z.md` (copy the most recent file in `docs/releases/` and edit it). This text is what gets pasted into the GitHub release description later.

## Rename in v0.2.2

- Since v0.2.2 the `productName` in `src-tauri/tauri.conf.json` is **ERX Tools** (previously "ERX Tools for Windows"). The NSIS default install folder (`%LOCALAPPDATA%\ERX Tools`), the Start Menu/Desktop shortcut names, the uninstall registry key and the installer window title all derive from `productName` (verified by reading the generated `src-tauri/target/release/nsis/x64/installer.nsi`), so **v0.2.2 installs NEXT TO an existing v0.2.0/v0.2.1 ("ERX Tools for Windows") and does not replace it**.
- No migration hook was added, on purpose (owner decision: not worth the risk).
- The v0.2.2 release notes must tell users to uninstall the old **"ERX Tools for Windows"** manually and to **not** tick "Delete the application data" during that uninstall — the data folders `%APPDATA%\com.erxtools.windows` and `%LOCALAPPDATA%\com.erxtools.windows` are identifier-based and shared by both apps.
- Do not run both apps at the same time.
- The released file names (`ERX-Tools-<version>-<arch>-setup….exe`) are unchanged, and later versions replace v0.2.2 normally.

## 5. Build the installers

```
npm run release:build
```

This builds **four installers + SHA256SUMS.txt** into `release-artifacts/vX.Y.Z/`. It takes a long time (expect 30–60 minutes; the first build on a machine is slower). Do not use the PC heavily meanwhile. The script fails fast on any problem; see Troubleshooting below.

## 6. Verify before publishing

- Recompute the checksums and compare with `SHA256SUMS.txt` (the release-verification scripts from Prompt 10e live in `target/`, which is not kept; re-create them or use PowerShell `Get-FileHash`).
- Install the small x64 installer by hand and check: app starts, version text shows the new version, a tool opens, the update button behaves.
- Check that each `with-webview2` file is at least 50 MB bigger than its standard sibling.
- Build the executables' architecture into the plan if anything was changed: x64 app must be `0x8664`, ARM64 app must be `0xAA64` (a tiny Python PE-header check did this in Prompt 10e; ask the agent to redo it).

## 7. Commit and push

The agent commits everything tracked. **The owner runs `git push` personally** (from PowerShell: `cd C:\dev\erxtools` then `git push`).

## 8. Publish on GitHub (owner, by hand)

1. Open https://github.com/EhsanKarimiRX/erx-tools-windows/releases and click **Draft a new release**.
2. Click **Choose a tag**, type `vX.Y.Z`, and click **Create new tag: vX.Y.Z on publish**. Leave the target branch as `main`.
3. **Release title:** `ERX Tools for Windows vX.Y.Z (early beta)` (or the version's real status wording).
4. **Description:** open `docs/releases/vX.Y.Z.md` in Notepad, select all, copy, and paste it into the description box.
5. Drag **6 files** from `release-artifacts/vX.Y.Z/` into the "Attach binaries by dropping them here" box: the four `ERX-Tools-X.Y.Z-…-setup….exe` files, `SHA256SUMS.txt`, and `THIRD_PARTY_LICENSES.md`. Wait until every file finishes uploading (the large ones take a while).
6. **Leave "Set as a pre-release" UNCHECKED** (the app's update button ignores pre-releases). Leave **"Set as the latest release"** checked.
7. Click **Publish release** (not "Save draft").
8. Check: open the release page in a private/incognito window; you should see the 6 files.

## 9. After publishing

- Install the new version from the release page download, open the app, and click the round arrow button next to the version number: it must turn **green** and say you have the latest version. If it turns red or stays gray, the tag probably does not match the version, or the release is a draft/pre-release.
- Send `SHA256SUMS.txt` (or the single hash) to anyone who asks for a way to verify their download.

## 10. Troubleshooting

- **ARM64 build fails with a linker error containing `link: missing operand after '\377\376'` or similar:** Git Bash's GNU coreutils `link.exe` (from `/usr/bin/`) is being used instead of MSVC's linker, because rustc's automatic MSVC discovery cannot see the installed Visual Studio and falls back to `PATH`. The fix is already in `src-tauri/.cargo/config.toml`: the `[target.aarch64-pc-windows-msvc]` section pins the real ARM64 linker (`…\VC\Tools\MSVC\<version>\bin\Hostx64\arm64\link.exe`) and adds `/LIBPATH:` entries for the MSVC `lib\arm64` folder and the Windows SDK's `um\arm64` and `ucrt\arm64` folders. **These paths are specific to this PC's Visual Studio and Windows SDK versions — when Visual Studio, the MSVC toolset, or the Windows SDK is updated, update the pinned paths to the newly installed versions.** (The complete MSVC toolset must be used; some newer toolsets ship without ARM64 CRT libraries.)
- **A build fails with `failed to bundle project: 'Peer disconnected'` while downloading (`go.microsoft.com/fwlink/...`):** this is a transient network error during the WebView2 offline-installer download. Just run `npm run release:build` again.
- **Windows SmartScreen warning ("Windows protected your PC")** during install: expected — the installers are not code-signed. Click **More info** → **Run anyway**.
- **The update check says there is no new release:** the release may be a draft or marked pre-release (both are ignored by the app), or the tag does not equal `v` + the app version.

## 11. Regenerating the third-party license notices

The notices live in `THIRD_PARTY_LICENSES.md` in the repo root. To regenerate them after changing dependencies:

1. In `src-tauri` run:

   ```
   cargo about generate about.hbs > ..\target\rust-licenses.md
   ```

   (`about.toml` lists the accepted licenses — including MPL-2.0 — and `about.hbs` is the Markdown template. If `cargo about generate` reports a crate under a license that is not in the accepted list, do not just add it: ask the owner.)

2. In the repo root run:

   ```
   node scripts/gen-third-party-js.mjs --rust target\rust-licenses.md
   ```

   This writes `THIRD_PARTY_LICENSES.md` (header + Rust crates section + JavaScript packages section) and copies it into `release-artifacts/v<version>/` when that folder exists.

3. Currently the file is **attached to the GitHub release** as a sixth file (see step 8 above). To bundle it **inside the installers** in a future version: add it to `bundle.resources` in `src-tauri/tauri.conf.json` (the entry is `"../THIRD_PARTY_LICENSES.md"`, relative to `src-tauri`) and rebuild **all four** installers with `npm run release:build`.
