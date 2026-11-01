# Progress

## Current status

**Prompt 2 (Image Converter) — checkpoint 2A done, awaiting user verification.**
The tool's UI is complete and wired to real Rust commands for inspection,
dialogs and a bridge probe. Actual conversion arrives in 2B/2C.

**Prompt 1 (foundation) — complete.** The app shell, tool registry, square-tile
home screen, 4 placeholder tools, Rust command skeleton, security baseline,
docs, git and GitHub are all in place.

## Prompt 2 — Image Converter

### Checkpoint 2A (this commit) — UI + file intake + bridge

- [x] Cleanups: `authors` = EhsanKarimiRX, unused `serde_json` removed,
      vite IPv4 pin documented in DECISIONS.md
- [x] Shared `DropZone` (Tauri `onDragDropEvent`, real paths; window-wide
      drag highlight; disabled state) and shared `ProgressBar`
- [x] Rust module `tools/image_convert/` split into `core` (pure,
      unit-testable) + `commands` (thin Tauri layer)
- [x] Content-sniffing inspection of every candidate file (format detected
      from bytes, never the extension), with animated-GIF/WebP and
      multi-page-TIFF detection at header level
- [x] Native file dialog (multi-select, filtered) and folder dialog opened
      from Rust — no new webview permissions
- [x] File list: name/size/detected format/status, remove one, clear all,
      duplicate skip with note, 200-file limit with overflow note,
      unsupported/invalid files rejected with reasons, mixed formats,
      Persian/Unicode names via Rust-side path handling
- [x] Options panel: output format, JPEG quality slider (default 85),
      background color for flattening (JPEG/BMP), output location
      (same folder / chosen folder), metadata-removal note
- [x] Convert button → `probe_conversion` command proving the JS → Rust
      bridge with the real validated options payload
- [x] Gates pass: `npm run build`, `cargo fmt --check`,
      `cargo clippy -- -D warnings`

### Checkpoint 2B (next) — conversion core with unit tests

Decode/encode for all supported combinations, alpha flattening, EXIF
orientation, naming/collision rules, limits, error cases, first-frame
handling.

### Checkpoint 2C (after that) — full wiring

Ordered start with bounded parallel workers, progress reporting, per-file
status, cancel, summary, Open output folder, polish, docs.

## Finished

- [x] Tauri 2 + React + TypeScript scaffold (create-tauri-app, react-ts, npm)
- [x] Template cleaned: greet demo, demo UI/CSS/SVGs, opener plugin removed
- [x] App identity: "ERX Tools for Windows", `erx-tools`, 0.1.0,
      identifier `com.erxtools.windows`
- [x] Window: 1000x700 default, 800x560 minimum, centered, English title
- [x] Tool registry (`src/tools/registry.ts`); home screen generated from it
- [x] Square-tile grid (CSS grid, auto-fill, minmax(160px, 1fr)), hover /
      focus / pressed states, keyboard accessible (native buttons)
- [x] Lazy loading per tool (dynamic import; one Vite chunk per tool)
- [x] Navigation: tiny state-based view switch; Back returns home
- [x] Error boundary per tool: friendly error screen, app keeps working
- [x] Theme via CSS variables, follows Windows light/dark (prefers-color-scheme)
- [x] lucide-react icons (ISC license, tree-shaken, bundled offline)
- [x] Rust: `tools` module, central `generate_handler!` list in `lib.rs`,
      shared `ToolError` type, `get_app_info` command
- [x] Footer shows app version fetched from Rust (proves the bridge)
- [x] Security baseline: strict CSP (`csp` + dev-only `devCsp`),
      capabilities reduced to `core:default`, no plugins, no remote content
- [x] Checks pass: `npm run build`, `cargo fmt --check`,
      `cargo clippy -- -D warnings`
- [x] Docs: README, DECISIONS, docs/ADDING_A_TOOL, this file
- [x] Git repository initialized (branch `main`, remote `origin` set to
      `https://github.com/EhsanKarimiRX/erx-tools-windows.git`)
- [ ] GitHub push — done manually by the owner:
      `git push -u origin main` (first push opens a GitHub login)
- [x] Verified in browser: navigation, placeholders, error boundary,
      focus walk, dark/light themes

## Known issues

- The test harness (in-app browser) does not synthesize button-activating
  Enter key events (no default action on synthetic keydown). Not an app bug:
  tiles are native `<button>` elements, whose Enter/Space activation is
  guaranteed in real browsers and WebView2. Keyboard focus movement was
  verified; activation will be confirmed in the desktop window.
- Placeholder tools are marked `status: "ready"` so they can be opened for
  testing; each shows "This tool is not built yet."

## Next steps

1. **Prompt 2 — Image Converter**: 2A done (UI + intake + bridge). Next:
   2B (conversion core + tests), then 2C (parallel pipeline, progress,
   cancel, summary).
2. Later tools: QR Code Generator, Folder Icon Changer, Encrypted Notes
   (needs Persian/RTL text support; secret state only in Rust).
3. Installer work (separate prompts): NSIS x64 + ARM64, icons, versioning,
   optional code signing and CI.

## Open questions

- None currently.
