# Progress

## Current status

**Prompt 4 (Folder Icon Changer) — sub-step 4A-1 done.** Module skeleton
with validated settings on the Rust side, mirrored in TypeScript. Gates
pass. NEXT: 4A-2 (color derivation, pattern geometry, SVG renderer,
`folder_icon_preview_svg` command, unit tests).

## PROMPT 4 STATUS

- Last finished sub-step: **4A-1** — `tools/folder_icon/` Rust module
  (`colors.rs` strict `#RRGGBB` parsing, `settings.rs` the single validated
  source of settings types/bounds) + frontend skeleton
  (`src/tools/folder-icon/` with `types.ts` mirroring the Rust settings and
  the 12 preset colors); module registered in `tools/mod.rs`. Temporary
  `#![allow(dead_code)]` at the module root until the 4A-2 preview command
  consumes the types (same pattern as error.rs).
- NEXT STEP: 4A-2 — OKLab color derivation (back panel/highlight), pattern
  geometry (vertical/horizontal/checker, clipped to the front panel),
  geometry-to-SVG renderer, `folder_icon_preview_svg` command, unit tests.
- Known issues: none. Small previews are SVG until 4B, then they switch to
  the exact .ico raster data (spec 4.2 "same data as the .ico").

**Prompt 3 (QR Code Generator) — code complete (3A–3C), awaiting the
owner's manual checklist.** All three content types, both styles, custom
colors, transparent background, error correction, margin, PNG/SVG export
with size snapping, and centered logo support. 100 Rust tests including
read-back of the real PNG renderer; all quality gates pass.

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

### Checkpoint 3B (this commit) — PNG export + all tabs + options

- [x] `raster.rs`: PNG rendering via `tiny-skia` 0.12 (pure Rust,
      ARM64-safe) from the SAME geometry the SVG writer consumes — the PNG
      file, SVG file and preview always match
- [x] Size snapping: requested 256–4096 px snapped down to a whole number
      of pixels per module; the UI shows the exact result ("saves at
      1008 × 1008 px (35 px/module)")
- [x] `save_qr_png` command (native Save As from Rust, default
      `qr-code.png`, no new webview permissions)
- [x] Scan-tested the rounded finder eyes with the independent decoder:
      outer radius ≥ 1.5 made level-L codes misread (format info around
      the eyes) at some sizes; landed on outer 1.25 / hole 0.25 / center
      1.0, which decodes at every level and size tested (see DECISIONS)
- [x] Read-back tests now drive the REAL renderer: both styles × all four
      EC levels × (URL, Persian UTF-8, Wi-Fi specials), custom colors,
      transparent composited on white, inverted codes (pixels inverted
      first; real scanners vary — the app warns), dense near-limit content
- [x] UI: Website tab (https:// added, final link shown, http/https only)
      and Wi-Fi tab (SSID, show/hide password, WPA/WEP/none, hidden
      checkbox, plain-text password warning; password cleared when leaving
      the tab or the tool), error correction L/M/Q/H, margin slider with
      hint, PNG size slider, Save PNG + Save SVG
- [x] Gates pass: `cargo test` (91), `cargo fmt --check`,
      `cargo clippy --all-targets -- -D warnings`, `npm run build`
- [ ] Owner verification: scan PNG exports with a phone (both styles, all
      tab types), check size snapping display, Wi-Fi password flow

### Checkpoint 3C (this commit) — logo + polish + docs

- [x] `logo.rs`: safe logo loading with the Image Converter's hardening
      (content-based format detection — no SVG logos in v1, 10 MB file
      limit, 80 MP decompression-bomb guard, decoder allocation cap),
      EXIF orientation applied, downscaled into a 512×512 transparent
      square canvas (aspect preserved, centered) and re-encoded as PNG
- [x] `pick_logo_file` / `load_logo` commands (native dialog from Rust,
      no new webview permissions); the frontend only ever holds the
      prepared PNG, never the original file
- [x] Logo integration: modules under (and one module around) the logo
      are cleared; with a background the area shows the background color,
      with transparency it stays transparent; logo embedded in the SVG as
      a base64 data URL (self-contained files) and composited into the
      PNG at the exact same position/size
- [x] Error correction forced to H whenever a logo is present — the UI
      locks the level control with a hint, and Rust force-upgrades
      regardless of what the UI sends
- [x] Logo size slider 10–20% of the QR width (default 15%), with a
      "close to the maximum — test scanning" warning from 18%; Rust
      validates the fraction and re-validates the PNG (never trusts the UI)
- [x] CSP: `img-src` gained `data:` — the smallest possible change, needed
      so the logo preview (inline SVG data-URL image + thumbnail) renders;
      still no remote sources, no unsafe-inline (see DECISIONS.md)
- [x] `base64` crate added (pure Rust, tiny) for the data URLs
- [x] Shared `DropZone` reused for logo intake (drag & drop + native
      browse button)
- [x] Gates pass: `cargo test` (100), `cargo fmt --check`,
      `cargo clippy --all-targets -- -D warnings`, `npm run build`
- [x] docs/ADDING_A_TOOL.md re-checked — the recipe was followed exactly
      (module layout, central command list, narrow permissions), no
      changes needed

### Owner manual checklist — QR Code Generator (Prompt 3)

Scan with a phone camera. If something fails, note the content type, style
and colors.

| # | Check | Notes |
|---|-------|-------|
| 1 | Text: plain English text, sharp style | scans, right text |
| 2 | Text: Persian text (`سلام`) | scans, phone shows it correctly |
| 3 | Text: multi-line text | line breaks preserved |
| 4 | Website: `example.com` (no scheme typed) | hint shows https://, phone opens the right address |
| 5 | Website: full https URL | scans |
| 6 | Wi-Fi: join network | phone offers to join |
| 7 | Wi-Fi: password with `; , : "` and a `\` | joins with that exact password |
| 8 | Wi-Fi: No password network | joins without password |
| 9 | Wi-Fi: leave tab, come back | password field empty again |
| 10 | Sharp style | scans |
| 11 | Rounded style | scans, looks rounded incl. the eyes |
| 12 | Black on white (default) | scans |
| 13 | White on black | note which scanner apps fail (inverted) |
| 14 | Custom colors (e.g. dark blue on cream) | scans |
| 15 | Transparent background: printed/shown on WHITE page | scans |
| 16 | Transparent background on a DARK page | expect problems — that is what the warning is for |
| 17 | Small logo (10%) | scans |
| 18 | Large logo (20%) + warning shown | check the warning appears; test scanning carefully |
| 19 | Logo with transparency (PNG with alpha) | looks right on the checkerboard and on white |
| 20 | Empty input | Save buttons disabled, hint shown |
| 21 | Very long text | red message with the max character count |
| 22 | Save PNG, open in Photos, scan | scans; size matches the shown "saves at N × N px" |
| 23 | Save SVG, open in Chrome, scan | scans; contains the logo if one was set |
| 24 | Save path with Persian characters | file written correctly |
| 25 | Change options quickly | no flicker, no freezing, preview stays current |
| 26 | Light and dark Windows theme | readable, no broken colors |
| 27 | Error correction levels L→H (no logo) | switchable; denser code at H |
| 28 | Margin slider 1 → 10 | quiet zone grows/shrinks |

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
