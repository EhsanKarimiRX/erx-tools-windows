# ERX Tools for Windows

A lightweight, fast, offline toolbox app for Windows 10+, built with
[Tauri 2](https://tauri.app) (Rust backend, WebView2 frontend) and
React + TypeScript.

The home screen shows every tool as a square tile. Tools live in isolated
folders and are lazy-loaded only when opened — see
[docs/ADDING_A_TOOL.md](docs/ADDING_A_TOOL.md) to add one.

## Tools

- **Image Converter** — convert images between JPEG, PNG, WebP, BMP, GIF
  and TIFF in batches, with live progress, cancel and a summary.
- **QR Code Generator** — create QR codes from text, links or Wi-Fi
  credentials; sharp or rounded style, custom colors, logo support; export
  as PNG or SVG.
- **Folder Icon Changer** — recolor Windows folder icons in any color,
  with optional stripe/checker patterns; applies a multi-size .ico plus
  desktop.ini to selected folders and restores the default icon again.
  Fully offline, no admin rights. Note: Windows may take a few seconds to
  show a new folder icon; if one is late, reopen the folder or right-click
  an empty area and choose Refresh (a restart shows it in every case).
  Tested on Windows 11 only; Windows 10 is untested. No claim of instant
  refresh.
- **Encrypted Notes** — planned (placeholder tile).

## Development

Prerequisites: Node.js (LTS), Rust (stable, MSVC toolchain), WebView2
(preinstalled on Windows 10/11).

```powershell
npm install
npm run tauri dev
```

The first Rust compile takes several minutes; later runs are fast.

## Checks

```powershell
npm run build              # TypeScript strict check + production frontend build
cd src-tauri
cargo fmt --check          # Rust formatting
cargo clippy -- -D warnings  # Rust lints
```

## Building an installer

Not set up yet (planned: NSIS setup .exe for x64 and ARM64). The release
build command will be `npm run tauri build`.

## Security

The app is fully offline: no network calls, no telemetry, no remote content.
The webview has a strict Content-Security-Policy and minimal Tauri
permissions (core defaults only). The UI never touches the file system
directly; all file work goes through Rust commands. See DECISIONS.md for the
rationale and the "Security rules" section below.

### Security rules (for every future change)

- Never add a Tauri plugin or capability unless a tool needs it; document why.
- Never relax the CSP beyond what the app strictly needs; prefer separate
  `devCsp` changes for dev-mode conveniences.
- Never load remote URLs, fonts, scripts or images in the webview.
- Never expose file system, shell, http or opener permissions "just in case".
- User-facing error strings live in Rust (`ToolError`); internal details must
  not leak to the UI.
- Commit `package-lock.json` and `src-tauri/Cargo.lock` for reproducible builds.
