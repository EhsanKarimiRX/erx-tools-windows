# ERX Tools for Windows

A small, offline toolbox for Windows. Every tool opens from a simple square tile on the home screen and does one job: convert images, make QR codes, recolor folder icons, keep the PC awake, tidy up folders, and open common Windows tools.

> **Early beta.** ERX Tools is new and is being tested with friends. Installers are on the [Releases page](https://github.com/EhsanKarimiRX/erx-tools-windows/releases) (see [Download](#download)). So far it has only been tested on Windows 11 (see [Tested on](#tested-on)).

## Why ERX Tools

- **Offline by design.** No telemetry and no remote content. The only network request is the update check, and only when you click it (see Privacy and security).
- **No administrator rights needed.** The app does not ask for admin to run. (Some Windows tools you launch from it may ask for permission themselves.)
- **Honest results.** Batch work ends with a clear report of what succeeded, failed or was skipped, and why. No fake progress bars.
- **Careful with your files.** Originals are never modified. Output files never overwrite existing files. The organizer only moves files, explains what it will do before it does it, and has Undo.
- **Small and focused.** Each tool lives in its own folder and is loaded only when you open it.

## Tools

### Image Converter
Convert images in batches (up to 200 files at a time) with live progress, cancel and a summary.
- **Input:** JPEG, PNG, WebP, BMP, GIF, TIFF and HEIC/HEIF (iPhone photos). HEIC is read with the codecs installed in Windows, so it needs Microsoft's *HEIF Image Extensions* and *HEVC Video Extensions* from the Microsoft Store (many PCs already have them). If they are missing, the app says so.
- **Output:** JPEG (with a quality slider), PNG, WebP (lossless), BMP and TIFF.
- **Output names:** every converted file gets "ERX" before the extension (photo.jpg → photoERX.png). If that name is taken it counts up (photoERX (1).png, photoERX (2).png, …), so nothing is ever overwritten.
- Camera rotation is applied automatically. Metadata (EXIF, GPS) and color profiles are not kept, so wide-color photos such as iPhone photos may look slightly less vivid. Only the first image/frame of a file is converted.

### QR Code Generator
Create QR codes from text, links or Wi-Fi credentials. Sharp or rounded style, custom colors, optional logo in the center, export as PNG or SVG. The export size (256–4096 px, default 1080) is saved exactly as typed, the rounded style draws a rounded plate behind the logo, and Save uses a real Save dialog that always gives the file the right extension.

### Favicons
Create a complete favicon package (favicon.ico, PNG icons, Apple touch icon, Android and maskable icons, web manifest, HTML snippet) from one logo image (PNG, JPEG, WebP, BMP, GIF, TIFF, ICO or SVG).

### Folder Icon Changer
Recolor Windows folder icons in any color, with optional stripe or checker patterns. It writes a multi-size `.ico` plus `desktop.ini` to the folders you choose and can restore the default icon. Offline, no admin rights.
- Windows shortcuts (`.lnk`) work too: drop a shortcut and its own icon is changed, without touching the shortcut's target (the icon is kept in the app's own icon folder).
- Windows usually shows a new folder icon within a few seconds. If one is late, open a different folder and then come back to it, or right-click an empty area and choose Refresh (a restart always shows it).
- Deleting a customized folder may show a Windows warning about `desktop.ini`. That is normal: the file only stores the icon settings.

### System Manager
Everyday Windows utilities in one panel:
- **Awake** — keeps the PC from sleeping during long runs. It temporarily sets the Windows sleep, hibernate and screen-off timeouts to *Never* and puts your original values back when you turn it off. Your original values are saved to disk first, so they can be restored even after a crash. Options: let the screen turn off normally, and an optional auto-off timer. Awake only blocks *automatic* sleep; clicking Sleep or closing a laptop lid still follows Windows.
- **Desktop Icons** — show or hide This PC, Recycle Bin, Control Panel, Network and your user folder on the desktop.
- **Organize** — tidy loose files into folders. The *Desktop* tab sorts the Desktop into Apps, Documents and More. The *Folder* tab sorts any folder (or Downloads) by format, by date (photo date taken, created or modified; by year or by month; files without a date go to "Date not found") or by size. It shows a plain-language preview first, never deletes or overwrites anything, leaves existing sub-folders alone, and can undo the last run.
- **Quick Launch** — one-click buttons for Control Panel, Defragment and Optimize Drives, Command Prompt, PowerShell (PowerShell 7 if installed), Device Manager, Services and Microsoft PC Manager (it opens the app if installed, otherwise offers its Microsoft Store page).
- **Command Tips** — a list of handy Windows commands you can copy and run yourself. ERX Tools never runs them for you. It includes "List installed apps" and "Save installed apps to a file", which cover the classic desktop programs and the Microsoft Store apps of your Windows account (portable apps that were never installed are not listed). Long commands start collapsed and expand with a More button.

### Encrypted Notes
Planned. Currently a placeholder tile.

## About the app

The round **i** button at the bottom-right of the home screen shows the app version, a short note about the MIT License and a **View on GitHub** button that opens the project's GitHub page in your browser.

## Requirements

- Windows 10 or Windows 11 (x64 or ARM64).
- Microsoft Edge WebView2 Runtime (included with Windows 11 and most up-to-date Windows 10 PCs). The standard installers download it during setup if it is missing; the "with-webview2" installers contain it.
- For HEIC photos: Microsoft's HEIF Image Extensions and HEVC Video Extensions (see Image Converter).

## Tested on

| Platform | Status |
|---|---|
| Windows 11, x64 | Developed and tested by hand |
| Windows 10 | Not tested yet |
| Windows on ARM64 | The code compiles for ARM64 but has not been run there |

If something does not work on your PC, please open an [issue](https://github.com/EhsanKarimiRX/erx-tools-windows/issues) and mention your Windows version.

## Download

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

## Privacy and security

- The only network request ERX Tools makes is the **update check**, and only when you click the update button next to the version number on the home screen. It is one HTTPS request to GitHub asking for the latest release number. Nothing is downloaded or installed automatically, and nothing about you or your files is sent. Like any web server, GitHub can see your IP address and the app version. Everything else works offline; the app sends no telemetry and loads no remote content. The webview has a strict Content-Security-Policy and only Tauri's core default permissions.
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

Building the installers (x64 and ARM64, two variants each) is described in [docs/RELEASING.md](docs/RELEASING.md).

To add a tool, see [docs/ADDING_A_TOOL.md](docs/ADDING_A_TOOL.md). Design decisions are recorded in [DECISIONS.md](DECISIONS.md).

### Security rules for every change

- Never add a Tauri plugin or capability unless a tool needs it; document why.
- Never relax the CSP beyond what the app strictly needs; prefer separate `devCsp` changes for dev-mode conveniences.
- Never load remote URLs, fonts, scripts or images in the webview.
- Never expose file system, shell, http or opener permissions "just in case".
- User-facing error strings live in Rust (`ToolError`); internal details must not leak to the UI.
- Commit `package-lock.json` and `src-tauri/Cargo.lock` for reproducible builds.
- The update check (written in Rust, in the update module) is the only allowed network access. Do not add other network calls without the owner's explicit approval.

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

The source code is released under the MIT License (see below). The **ERX Tools** name, the ERX Tools **logo** (the files in the `branding/` folder, `src/assets/erx-logo.png` and the app icons) and the ERX Tools **wallpaper** (`src-tauri/assets/erx-wallpaper.jpg`) are **not** covered by that license; they were made for the project and remain the project owner's. You are welcome to fork the code and change it, but please give your version its own name, its own logo and its own wallpaper, and do not present it as the official ERX Tools. Details: [branding/NOTICE.md](branding/NOTICE.md).

## License

The source code is released under the [MIT License](LICENSE). The name and logo are not part of that license (see [Name and logo](#name-and-logo)).
Third-party components and their licenses are listed in [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).
