# Progress

## Current status

**Prompt 3 (QR Code Generator) — checkpoint 3A done, awaiting the owner's
visual/scan verification.** Rust core complete with 82 passing tests
including QR read-back tests; minimal UI (Text tab, styles, colors, live
preview, Save SVG) ready in `npm run tauri dev`. Checkpoints 3B (PNG export,
Website/Wi-Fi tabs, options) and 3C (logo, polish, docs) are next.

**Prompt 2 (Image Converter) — checkpoints 2A–2C done; 2A/2B verified, 2C
awaiting the owner's manual checklist.** The tool is fully functional: mixed
batches up to 200 files convert with live per-file progress, cancel, summary
and folder reveal.

**Prompt 1 (foundation) — complete.** The app shell, tool registry, square-tile
home screen, 4 placeholder tools, Rust command skeleton, security baseline,
docs, git and GitHub are all in place.

## Prompt 3 — QR Code Generator

### Checkpoint 3A (this commit) — Rust core + minimal UI

- [x] Crates: `qrcodegen` 1.8 (QR matrix; Nayuki reference implementation,
      MIT, pure Rust, zero deps) and dev-only `rqrr` 0.11 (independent QR
      decoder for read-back tests, never ships; reuses our `image` 0.25)
- [x] Rust module `tools/qr_code/` split into pure, unit-tested files:
      `payload` (text/website/Wi-Fi builders, URL normalization, Wi-Fi
      escaping of `\ ; , : "`), `colors` (strict `#RRGGBB`, WCAG luminance
      + contrast, warnings), `matrix` (qrcodegen wrapper, multibyte-safe
      "max N characters fit" computation), `geometry` (both styles from one
      segment list), `svg` (renderer), `core` (validation pipeline), plus
      thin `commands`
- [x] Sharp style: all dark modules merged into ONE nonzero fill path —
      no hairline seams at any scale. Rounded style: module corners rounded
      only where BOTH orthogonal neighbors are light (touching modules stay
      joined); finder patterns drawn as rounded ring (even-odd) + center
      block keeping the 1:1:3:1:1 scan proportions
- [x] SVG is the single source: preview shows the exact document Save SVG
      writes (no width/height, viewBox only). No user text ever enters the
      SVG — only validated colors and geometry (asserted by tests)
- [x] Commands `generate_qr` and `save_qr_svg` (native Save As from Rust,
      default name `qr-code.svg`, no new webview permissions); DTO shape
      tests pin the frontend JSON contract
- [x] Read-back tests: generated matrix → PNG → decoded with rqrr →
      byte-identical text, for a URL at every EC level, Persian UTF-8,
      multiline text, a Wi-Fi payload with all special characters, and
      dense near-limit content (3B moves these onto the real PNG renderer
      to cover rounded geometry + colors too)
- [x] Minimal UI: Text tab (dir="auto" for Persian), live debounced preview
      (200 ms) on checkerboard, Sharp/Rounded segmented control, color
      pickers + hex fields + presets + transparent checkbox, character
      counter, warnings list, friendly too-long/invalid errors, Save SVG
- [x] Gates pass: `cargo test` (82), `cargo fmt --check`,
      `cargo clippy --all-targets -- -D warnings`, `npm run build`
- [ ] Owner verification: scan the preview with a phone (sharp + rounded,
      Persian text, black-on-white and white-on-black), save an SVG and
      open it in Chrome

### Checkpoint 3B (planned) — PNG export + all tabs + options

PNG rendering with `tiny-skia` 0.12 (pure Rust, ARM64-safe), size snapping,
Save PNG, Website + Wi-Fi tabs with validation and password rules, error
correction and margin controls, contrast/inverted/transparent warnings
wiring, too-long messages.

### Checkpoint 3C (planned) — logo + polish + docs

Safe logo decoding (same hardening as Image Converter), center clearing,
automatic level H with locked control, size limits + warnings, smallest
possible CSP addition (`img-src data:` for the embedded logo preview),
PROGRESS/DECISIONS/ADDING_A_TOOL updates, owner manual checklist.

## Prompt 2 — Image Converter

### Checkpoint 2A (verified by owner) — UI + file intake + bridge

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

### Checkpoint 2B (this commit) — conversion core with unit tests

- [x] `convert.rs`: decode → EXIF orientation → alpha flatten → encode,
      all 6 input × 5 output format combinations; outputs are built in
      memory so failures never leave partial files
- [x] Animated GIF/WebP inputs: first frame via the crate's animation API
      (composited); multi-page TIFF: first page (generic decode)
- [x] `naming.rs`: `name.newext` → `name (1).newext` collisions,
      `name-converted.ext` for same-format re-compression, Windows-safe
      sanitization (forbidden chars, trailing dots/spaces, reserved device
      names, length cap); Unicode/Persian names preserved
- [x] `pipeline.rs`: inspect → pick name → convert → write for one file;
      originals never touched; output cannot escape the chosen folder
- [x] Limits enforced in the engine: 80 MP pixel guard before decode,
      decoder allocation cap (belt-and-braces on top of the crate default)
- [x] 29 unit tests: all 30 format combinations roundtrip (pixel-exact for
      lossless→lossless), flatten exactness, alpha preservation, EXIF
      orientation via a hand-crafted TIFF, first-frame handling, naming
      rules, oversized/damaged/empty/folder rejection, Persian names,
      option validation
- [x] Fixed a real inspection bug found by tests: `gif_is_animated` did not
      skip the global color table
- [x] Gates pass: `cargo test`, `cargo fmt --check`,
      `cargo clippy -- -D warnings`, `npm run build`

### Checkpoint 2C (this commit) — full wiring

- [x] `convert_files` command: bounded worker pool (CPU cores, capped at 4),
      files START in list order (shared index), one failing file never stops
      the batch
- [x] Progress via a Tauri 2 `Channel`: per-file `converting`/`done`/
      `failed`/`skipped` events in list order; overall progress bar in the UI
- [x] `cancel_conversion` command: stops starting new files, running ones
      finish, unstarted files are marked Skipped ("cancelled")
- [x] Final summary: converted / failed / skipped counts, total size before
      → after
- [x] "Open output folder" via `reveal_outputs` (Explorer shows the output
      files selected); button only shown when all outputs share one folder
- [x] UI states: Empty → Ready → Converting (options/list locked, Cancel) →
      Done (Convert more resets the list, Back stays in the header)
- [x] Probe command from 2A removed (real conversion replaced it)
- [x] Fix from owner review: rejected files (non-images renamed to .jpg/.png,
      MP3/MP4/EXE, etc.) now show a red warning — "Supported formats: JPEG,
      PNG, WebP, BMP, GIF, and TIFF." — in BOTH the empty and ready states
      (previously a small gray note, invisible on the empty screen)
- [x] Gates pass: `npm run build`, `cargo fmt --check`,
      `cargo clippy -- -D warnings`, `cargo test` (29 tests)

### Manual checklist (owner)

Covered in the Prompt 2 hand-off message: 1 file, 10 files, 100 mixed files,
over-the-limit message, corrupted file, non-image renamed to .jpg, very large
image, transparent PNG → JPEG with background, rotated phone photo, Persian
name, path with spaces, missing/read-only output folder, name collisions,
same-format re-compression, animated GIF note, cancel mid-batch, closing the
app mid-batch, light/dark theme.

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

1. **Prompt 3 — QR Code Generator**: 3A done. Owner verifies the preview
   (scan with phone); then 3B (PNG export, Website/Wi-Fi tabs, options)
   and 3C (logo, polish, docs, manual checklist).
2. Prompt 2 — Image Converter: owner runs the manual checklist; any
   findings go here.
3. Later tools: Folder Icon Changer, Encrypted Notes (needs Persian/RTL
   text support; secret state only in Rust).
4. Installer work (separate prompts): NSIS x64 + ARM64, icons, versioning,
   optional code signing and CI.

## Open questions

- None currently.
