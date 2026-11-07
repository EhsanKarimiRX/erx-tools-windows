# ERX Tools for Windows

A small, offline toolbox for Windows. Every tool opens from a simple square tile on the home screen and does one job: convert images, make QR codes, recolor folder icons, keep the PC awake, tidy up folders, and open common Windows tools.

> **Early beta.** ERX Tools is new and is being tested with friends. There is no installer yet (see [Download](#download)), and so far it has only been tested on Windows 11 (see [Tested on](#tested-on)).

## Why ERX Tools

- **Offline by design.** The app itself makes no network calls, has no telemetry and loads no remote content.
- **No administrator rights needed.** The app does not ask for admin to run. (Some Windows tools you launch from it may ask for permission themselves.)
- **Honest results.** Batch work ends with a clear report of what succeeded, failed or was skipped, and why. No fake progress bars.
- **Careful with your files.** Originals are never modified. Output files never overwrite existing files. The organizer only moves files, explains what it will do before it does it, and has Undo.
- **Small and focused.** Each tool lives in its own folder and is loaded only when you open it.

## Tools

### Image Converter
Convert images in batches (up to 200 files at a time) with live progress, cancel and a summary.
- **Input:** JPEG, PNG, WebP, BMP, GIF, TIFF and HEIC/HEIF (iPhone photos). HEIC is read with the codecs installed in Windows, so it needs Microsoft's *HEIF Image Extensions* and *HEVC Video Extensions* from the Microsoft Store (many PCs already have them). If they are missing, the app says so.
- **Output:** JPEG (with a quality slider), PNG, WebP (lossless), BMP and TIFF.
- Camera rotation is applied automatically. Metadata (EXIF, GPS) and color profiles are not kept, so wide-color photos such as iPhone photos may look slightly less vivid. Only the first image/frame of a file is converted.

### QR Code Generator
Create QR codes from text, links or Wi-Fi credentials. Sharp or rounded style, custom colors, optional logo in the center, export as PNG or SVG.

### Folder Icon Changer
Recolor Windows folder icons in any color, with optional stripe or checker patterns. It writes a multi-size `.ico` plus `desktop.ini` to the folders you choose and can restore the default icon. Offline, no admin rights.
- Windows can take up to about 45 seconds to show a new folder icon. If one is late, reopen the folder or right-click an empty area and choose Refresh (a restart always shows it).
- Deleting a customized folder may show a Windows warning about `desktop.ini`. That is normal: the file only stores the icon settings.

### System Manager
Everyday Windows utilities in one panel:
- **Awake** — keeps the PC from sleeping during long runs. It temporarily sets the Windows sleep, hibernate and screen-off timeouts to *Never* and puts your original values back when you turn it off. Your original values are saved to disk first, so they can be restored even after a crash. Options: let the screen turn off normally, and an optional auto-off timer. Awake only blocks *automatic* sleep; clicking Sleep or closing a laptop lid still follows Windows.
- **Desktop Icons** — show or hide This PC, Recycle Bin, Control Panel, Network and your user folder on the desktop.
- **Organize** — tidy loose files into folders. The *Desktop* tab sorts the Desktop into Apps, Documents and More. The *Folder* tab sorts any folder (or Downloads) by format, by date (photo date taken, created or modified; by year or by month; files without a date go to "Date not found") or by size. It shows a plain-language preview first, never deletes or overwrites anything, leaves existing sub-folders alone, and can undo the last run.
- **Quick Launch** — one-click buttons for Control Panel, Defragment and Optimize Drives, Command Prompt, PowerShell (PowerShell 7 if installed), Device Manager, Services and Microsoft PC Manager (it opens the app if installed, otherwise offers its Microsoft Store page).
- **Command Tips** — a list of handy Windows commands you can copy and run yourself. ERX Tools never runs them for you.

### Encrypted Notes
Planned. Currently a placeholder tile.

## Requirements

- Windows 10 or Windows 11 (x64 or ARM64).
- Microsoft Edge WebView2 Runtime (included with Windows 11 and most up-to-date Windows 10 PCs).
- For HEIC photos: Microsoft's HEIF Image Extensions and HEVC Video Extensions (see Image Converter).

## Tested on

| Platform | Status |
|---|---|
| Windows 11, x64 | Developed and tested by hand |
| Windows 10 | Not tested yet |
| Windows on ARM64 | The code compiles for ARM64 but has not been run there |

If something does not work on your PC, please open an [issue](https://github.com/EhsanKarimiRX/erx-tools-windows/issues) and mention your Windows version.

## Download

No installer has been published yet. Setup files for x64 and ARM64 are planned and will appear on the [Releases page](https://github.com/EhsanKarimiRX/erx-tools-windows/releases).

## Privacy and security

- The app itself makes no network calls, sends no telemetry and loads no remote content. The webview has a strict Content-Security-Policy and only Tauri's core default permissions.
- The UI never touches the file system directly; all file work goes through Rust commands.
- Some buttons hand off to Windows (for example opening the Microsoft Store page for PC Manager). That is Windows doing it, not ERX Tools.
- The code has had no professional security audit.

## Development

Prerequisites: Node.js (LTS), Rust (stable, MSVC toolchain), WebView2.

```
npm install
npm run tauri dev
```

The first Rust compile takes several minutes; later runs are fast.

Checks:

```
npm run build                 # TypeScript strict check + production frontend build
cd src-tauri
cargo fmt --check             # Rust formatting
cargo clippy -- -D warnings   # Rust lints
cargo test                    # Rust tests
```

Building an installer is not set up yet (planned: NSIS setup `.exe` for x64 and ARM64, built with `npm run tauri build`).

To add a tool, see [docs/ADDING_A_TOOL.md](docs/ADDING_A_TOOL.md). Design decisions are recorded in [DECISIONS.md](DECISIONS.md).

### Security rules for every change

- Never add a Tauri plugin or capability unless a tool needs it; document why.
- Never relax the CSP beyond what the app strictly needs; prefer separate `devCsp` changes for dev-mode conveniences.
- Never load remote URLs, fonts, scripts or images in the webview.
- Never expose file system, shell, http or opener permissions "just in case".
- User-facing error strings live in Rust (`ToolError`); internal details must not leak to the UI.
- Commit `package-lock.json` and `src-tauri/Cargo.lock` for reproducible builds.

## How this project is built

ERX Tools is a product of one owner, who is not a professional programmer, built together with AI. The owner decides what to build and how it should behave. An AI consultant helps design each feature and writes detailed step-by-step specifications, and an AI coding agent writes the code from them. The owner tests every feature by hand. Automated checks run on every step: TypeScript strict mode, `rustfmt`, `clippy` with warnings as errors, and more than 370 Rust tests.

## Credits

| Who | Role |
|---|---|
| **EhsanKarimiRX** (project owner) | Idea, product design, every feature decision, requirements, hands-on testing and acceptance, and publishing. |
| **Claude Sonnet 5.5** (Anthropic), in the Claude desktop app | Consultant and prompt writer: discussed each feature with the owner, designed the technical specifications and the step-by-step build prompts, and reviewed the agent's reports. |
| **GLM 5.3 Flash** (about 90% of the implementation, by the owner's estimate) and **GLM 5.3** (the early stage), in Z Code (the z.ai desktop app) | The coding agents: wrote and tested the code by following those prompts. |

Built with [Tauri 2](https://tauri.app/), [React](https://react.dev/), TypeScript and Rust.

## Name and logo

The source code is released under the MIT License (see below). The **ERX Tools** name and the ERX Tools **logo** (the files in the `branding/` folder and the app icons) are **not** covered by that license. You are welcome to fork the code and change it, but please give your version its own name and its own logo, and do not present it as the official ERX Tools. Details: [branding/NOTICE.md](branding/NOTICE.md).

## License

The source code is released under the [MIT License](LICENSE). The name and logo are not part of that license (see [Name and logo](#name-and-logo)).
