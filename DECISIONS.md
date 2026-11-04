# Decisions

Why the foundation looks the way it does. Newest decisions can be appended at
the bottom of their section.

## Framework and stack

- **Tauri 2 + React 19 + TypeScript (strict) + Vite.** Tauri gives a tiny,
  fast, native-window app with a Rust backend; React + TS is mainstream and
  well supported by the official template. WebView2 is preinstalled on
  Windows 10/11.
- **npm** as package manager; lockfiles (`package-lock.json`,
  `src-tauri/Cargo.lock`) are committed for reproducible builds.
- **No heavy dependencies.** Only runtime additions: `@tauri-apps/api`
  (bridge) and `lucide-react` (icons; ISC license, tree-shakes, bundled
  locally — no remote fonts or scripts). `thiserror` on the Rust side for
  the shared error type.
- **No router.** A state-based view switch in `App.tsx` (home ↔ one tool) is
  simpler and fully sufficient for a tile launcher; React Router would add
  complexity for zero benefit at this scale.
- **No state library.** Plain React state/hooks only.
- **ARM64-safe choices:** no native Node modules, no C dependencies; pure
  Rust crates + WebView2, so the later ARM64 NSIS build is not blocked.

## Structure

- **`src/tools/registry.ts` is the single source of tools.** The home screen
  is generated from it; adding a tool never touches the shell. Each entry is
  a typed `ToolDefinition` (id, name, description, icon, lazy `load`,
  `status`).
- **Tool isolation:** tools never import from each other; shared UI lives in
  `src/shared/` (`ToolLayout` today). Each tool is a folder under
  `src/tools/<id>/` with `index.tsx` exporting a default component.
- **Lazy loading:** `load: () => import("./<id>")` gives one Vite chunk per
  tool (verified in the build output); tool code is fetched only when the
  tool is opened. `ToolPage` wraps every tool in `Suspense` +
  `ErrorBoundary`.
- **`src/tools/_template/`** is a copyable starting point for new tools.
- **Rust mirrors the structure:** `src-tauri/src/tools/` holds one module per
  tool plus shared `error.rs` and `app_info.rs`. All commands are registered
  in the single `generate_handler!` list in `lib.rs` (a Tauri requirement) —
  one line per command.
- **Errors:** `ToolError` (thiserror) converts to a user-friendly English
  string for the frontend via its `Serialize` impl. Variants will grow as
  real tools land; internal details must never reach the UI.

## UI

- **Square tiles** (aspect-ratio 1/1) in a CSS grid with
  `repeat(auto-fill, minmax(160px, 1fr))` — scales from the minimum window
  size to maximized and to 30+ future tools (scrollable).
- **Tiles are native `<button>`s** — free keyboard accessibility (Tab,
  Enter, Space), `:focus-visible` outlines styled. "coming-soon" tiles are
  `disabled` (dimmed, unclickable).
- **Theme:** CSS variables in `src/styles/theme.css`;
  `prefers-color-scheme` follows the Windows setting automatically; no
  switcher yet. System font stack (Segoe UI first); no remote fonts.
- **Tooltips:** tile `title` attribute carries the one-line description.

## Names and identity

- Display name / window title / productName: **ERX Tools for Windows**
- npm package + Rust crate: **erx-tools**
- Bundle identifier: **com.erxtools.windows** — valid reverse-DNS form
  (alphanumeric + periods, no hyphens, does not end in `.app`), confirmed
  against Tauri's identifier rules. Chosen once; never change it after
  release (install identity depends on it).
- GitHub repo: **erx-tools-windows** (public).

## Security

- **Strict CSP** in `tauri.conf.json` for release (`csp`): `'self'` only,
  `object-src 'none'`, `base-uri 'none'`, no `unsafe-inline`, no remote
  sources; `connect-src ipc: http://ipc.localhost` is the Tauri IPC
  requirement. Dev mode gets a separate, slightly wider `devCsp` (inline
  styles/scripts for Vite's HMR client, `data:` images, HMR websocket) —
  release builds never see it.
- **Least-privilege capabilities:** `src-tauri/capabilities/default.json`
  grants only `core:default` for the `main` window. No filesystem, shell,
  http, or opener permissions. Every future tool adds exactly what it needs
  and documents why.
- **Fully offline:** no network calls, telemetry, remote content, or web
  fonts. `get_app_info` reads name/version from the Tauri package info —
  the footer never hardcodes them.
- **Release hardening by default:** devtools are disabled in release Tauri
  builds unless explicitly enabled; `main.rs` keeps the
  `windows_subsystem = "windows"` attribute (no console window).
- **File access:** the UI never touches the file system; all file work goes
  through Rust commands (added per tool, with minimal permissions).

## Process

- **Checkpoints (1A scaffold, 1B frontend, 1C Rust + security + docs +
  git):** after each, the app runs and is committed — safe stopping points.
- **Checks gate:** `npm run build`, `cargo fmt --check`,
  `cargo clippy -- -D warnings` must pass before every commit.

## Dev environment

- **Vite dev server pinned to IPv4 `127.0.0.1`** (`vite.config.ts`). On this
  machine an OS-level setting makes Vite bind only to `[::1]` (IPv6), which
  the Tauri CLI's probe on `127.0.0.1` never sees, so `tauri dev` hangs until
  timeout. Pinning the host fixes it; do not "simplify" it away.

## Image Converter (Prompt 2)

- **Image decoding/encoding: `image` crate 0.25 with default features OFF**
  and only the needed format features enabled (`bmp gif jpeg png tiff webp`).
  Smaller binary and attack surface; all chosen codecs are pure Rust, so x64
  and ARM64 builds stay trivial.
- **WebP output is lossless-only in v1.** The pure-Rust WebP encoder
  (`image`/`image-webp`) implements only VP8L lossless. Real lossy WebP
  would require libwebp (C code) — extra build risk for ARM64 CI; deferred
  by decision of the owner. The UI labels the format "WebP (lossless)".
- **GIF output: skipped.** The pure-Rust GIF encoder is palette-based
  (256 colors), giving poor photo quality. GIF remains a supported INPUT.
- **Native file/folder dialogs via `tauri-plugin-dialog`, used only from
  Rust commands** (`blocking_pick_files` / `blocking_pick_folder`). The
  webview gets no dialog permission at all; capabilities stay
  `core:default`. No JS dialog package is installed.
- **Drag & drop uses Tauri's `onDragDropEvent`** (core webview API, covered
  by `core:default`): real absolute paths reach the frontend, which passes
  them to Rust for content-based inspection.
- **File format detection is content-based** (`ImageReader::with_guessed_format`),
  never extension-based; unsupported/damaged/oversized files are rejected
  with friendly reasons before conversion.
- **Limits (v1):** 200 files per batch (one `MAX_FILES` constant, UI and
  behavior), 256 MB per input file, 80 megapixels per image
  (decompression-bomb guard), enforced before decode; decoder limits
  follow in 2B.
- **Privacy by construction:** outputs are re-encoded from decoded pixels,
  so no EXIF/GPS/metadata survives; EXIF orientation is applied during
  conversion via `ImageDecoder::orientation()` +
  `DynamicImage::apply_orientation()` (image 0.25.8+).
- **First frames of animations come from the crate's animation API**
  (`AnimationDecoder::into_frames`, fully composited) for animated GIF and
  animated WebP alike — both decode in pure Rust; multi-page TIFF simply
  uses page 0 of the generic decode.
- **Outputs are encoded in memory, then written in one `fs::write`.** A
  failed conversion can never leave a partial output file.
- **Naming rules live in `naming.rs`:** `name.newext` → `name (1).newext`
  for collisions, `name-converted.ext` for same-format re-compression.
  Stems are sanitized against Windows-forbidden characters, trailing
  dots/spaces, reserved device names (CON, COM1…) and overlong names; the
  sanitized stem cannot contain path separators, so `Path::join` can never
  escape the chosen output folder. Unicode/Persian names pass through.
- **Limits are enforced at two points:** file size + header pixel count at
  inspection (before the batch), and a hard pixel guard + decoder
  allocation cap in the engine right before decode (decompression bombs).
- **Batch progress uses a Tauri 2 `Channel`** (`tauri::ipc::Channel`), the
  currently recommended mechanism for command → frontend streaming — no
  global event bus, no extra permissions. Events carry the list index, so
  the UI always shows results in list order regardless of completion order.
- **Worker pool is bounded at 4** (`min(CPU cores, 4)`): one image can
  decode into ~320 MB of pixels, so unbounded parallelism could exhaust
  memory; files are STARTED in list order via a shared atomic index.
- **Cancel is a managed `AtomicBool`** (`BatchState`, command
  `cancel_conversion`): workers stop pulling new files, running ones
  finish, unstarted files are reported as Skipped; outputs already written
  are kept. The flag is reset when a new batch starts.
- **"Open output folder" uses `tauri-plugin-opener` from the Rust side only**
  (`reveal_item_in_dir`, showing the output files selected in Explorer).
  Like the dialog plugin, the webview receives no opener permission;
  capabilities stay `core:default`.

## QR Code Generator (Prompt 3, so far)

- **QR matrix: `qrcodegen` 1.8** (Project Nayuki, MIT). The reference QR
  implementation: all 40 versions, all 4 EC levels, strict spec conformance,
  UTF-8 byte mode out of the box. Zero dependencies and pure Rust, so x64
  and ARM64 stay trivial. Last published 2022, but the library is the
  companion of the widely used multi-language QR-Code-generator project and
  is feature-complete for our needs.
- **Read-back decoder: `rqrr` 0.11 as a dev-dependency only.** An
  independent pure-Rust decoder proves the generated codes actually scan:
  tests render a PNG and assert the decoded text equals the input. It never
  enters the release binary. It reuses our exact `image` 0.25 — no version
  conflict.
- **PNG rendering will use `tiny-skia` 0.12** (checkpoint 3B): a pure-Rust
  2D rasterizer (~few hundred KB, ARM64-safe). We rasterize from the SAME
  geometry the SVG writer consumes, which avoids pulling in `resvg`
  (multi-MB SVG parser) while guaranteeing PNG and SVG match.
- **One SVG document is the single source of truth.** `generate_qr` returns
  the exact string `save_qr_svg` writes. The preview injects it into the
  DOM (`dangerouslySetInnerHTML`): safe because the markup is generated
  entirely in Rust from validated `#RRGGBB` colors and our own geometry —
  QR content never appears as text in the SVG (it lives in the module
  pattern; a test asserts the payload string is absent). CSP is unaffected:
  inline SVG elements are not affected by `script-src`, and the SVG uses
  presentation attributes, not inline styles. A one-word CSP addition
  (`img-src data:`) is only needed in 3C for the embedded logo preview.
- **The SVG has a `viewBox` but no width/height**, so it scales to the
  preview panel and to any viewer size; no size option applies to SVG
  export (PNG snapping comes in 3B).
- **No hairline seams:** sharp style merges every dark module into ONE path
  filled with the nonzero rule — a single fill region, so adjacent squares
  can never show AA gaps at any zoom. Rounded style reuses the same corner
  walker with per-corner radii: a corner is rounded only when BOTH
  orthogonal neighbors touching it are light, so touching dark modules keep
  straight shared edges and stay seamless.
- **Rounded finder eyes preserve scanability:** each 7×7 finder is drawn as
  a rounded outer ring (even-odd fill, 1-module stroke) plus a rounded 3×3
  center — the proportions scanners rely on are untouched; the read-back
  tests verify decoding.
- **Contrast threshold: 3.0:1** (WCAG contrast ratio). Below that, a
  non-blocking "hard to scan" warning appears. Additionally: a warning when
  the code is lighter than the background (inverted; some scanner apps
  refuse those) and when the background is transparent (result depends on
  the surface behind). All choices are unit-tested.
- **Colors are strictly `#RRGGBB`** (leading `#` required) — the same rule
  that keeps the SVG injection safe. The frontend adds a missing `#` on
  blur for convenience; Rust remains the authority.
- **Payload building lives in Rust**, not the frontend: the UI sends raw
  fields (`{kind: "text"|"website"|"wifi", ...}`) and Rust builds, escapes
  (Wi-Fi `\ ; , : "`), normalizes (`https://` added, http/https only) and
  validates. DTO shape tests pin the JSON contract so renames fail at test
  time, not at runtime.
- **"Too long" reports a real limit:** the longest prefix (in characters,
  multibyte-safe) that still fits at the selected level is computed by
  binary search and shown in the error and in the live counter.
- **Dialogs stay Rust-side** (`tauri-plugin-dialog`, already initialized):
  `save_qr_svg` opens the native Save As dialog with default name
  `qr-code.svg`; the webview gains no new permissions (capabilities remain
  `core:default`). Overwrite confirmation is the dialog's own, per spec.

- **PNG rendering: `tiny-skia` 0.12** (checkpoint 3B): pure-Rust rasterizer
  with anti-aliased path filling; ~1 MB of binary size, no system deps, so
  x64/ARM64 stay trivial. We rasterize from the same `Shape` list the SVG
  writer consumes — not from the SVG markup — guaranteeing both exports
  match without pulling in `resvg`.
- **PNG size snapping:** requested 256–4096 px is snapped DOWN to a whole
  number of pixels per module (`floor(requested/total) × total`), so every
  edge lands on exact pixel boundaries; the UI shows the exact size.
- **Rounded finder eyes are scan-tested, not just aesthetic:** with an
  independent decoder (rqrr) the outer-eye radius 1.5+ made LEVEL-L codes
  fail (format info around the eyes misread) at some render sizes, while
  other levels passed — a silent real-world scanning risk. Landed on outer
  1.25 / hole 0.25 / center 1.0, which decodes at every level × size ×
  content tested. The read-back suite now guards this permanently.
- **Wi-Fi password handling:** the password exists only in React state and
  in the Rust command's RAM (never logged, never written); it is cleared
  the moment the user leaves the Wi-Fi tab and when the tool unmounts. The
  payload is built and escaped entirely in Rust.
- **Website validation is deliberately permissive:** scheme detection via a
  letters-only prefix before the first `:` (so `example.com:8080/x` stays
  host:port), `https://` added when missing, whitespace/control chars and
  empty hosts rejected. No URL crate — a full RFC parser adds binary size
  for zero scanning benefit; QR scanners just open the string.
- **Logo loading reuses the Image Converter's hardening:** content-based
  format detection (a renamed .exe still fails), 10 MB file limit, 80 MP
  pixel guard, decoder allocation cap, EXIF orientation applied. SVG logos
  are rejected in v1 (XML parsing is a whole attack surface; PNG/JPEG/WebP
  cover the need).
- **The logo is normalized to a 512×512 transparent PNG** (aspect kept,
  centered, no cropping) exactly once, in Rust. The frontend only holds
  this prepared PNG; every generate/save call re-validates it (base64 →
  real PNG → exact canvas size) because the UI is never trusted.
- **Logo area handling:** modules fully inside the logo rectangle plus one
  module of padding are removed. No special background shape is needed:
  with a background the cleared area shows it, with a transparent
  background it stays transparent — both by construction. The logo is
  embedded in the SVG as a `data:image/png;base64` URL (files stay
  self-contained) and composited into the PNG at the same module
  coordinates, so both exports match.
- **Level H is forced with a logo** — twice: the UI locks the control at H
  when a logo is present, and Rust force-upgrades regardless of what
  arrives in the command. Removing the logo returns the level control to
  the user (UI default Q).
- **Logo size:** slider 10–20% of the QR width, default 15%; a
  non-blocking "close to the maximum" warning appears from 18%. Rust
  rejects fractions outside 5–20%.
- **CSP change (smallest possible):** `img-src` gained `data:`. Needed so
  the logo preview renders — the inline SVG references the logo by a
  data URL, and the logo thumbnail is a data-URL `<img>`. Nothing else
  widened: still no remote sources, no `unsafe-inline` for scripts/styles.
- **`base64` crate 0.22** added for the data URLs (pure Rust, tiny,
  ARM64-safe).
- **tiny-skia placement gotcha (for the record):** `draw_pixmap` applies
  its transform to the x/y position too; the logo position is therefore
  baked into the transform (scale then translate, drawn at the origin).
  The read-back test with a logo caught this — the misplaced logo covered
  a finder pattern and detection failed.

## Folder Icon Changer (Prompt 4)

### Icon design and color model

- **One geometry, two backends (like the QR tool):** `geometry.rs` builds
  an original 256×256 folder design — back panel with tab, larger front
  panel overlapping the lower part, rounded corners, subtle white
  top-highlight fade on the front panel. The SVG writer and the tiny-skia
  rasterizer both consume the same `Drawing`, so preview, SVG and .ico
  always match. All artwork is original; nothing was traced.
- **Color model: OKLab, not HSL.** The user picks ONE base color (the
  front panel). The back panel is derived in OKLab (Björn Ottosson's
  published reference formulas, public domain; correctness pinned by
  known-value and round-trip tests) because its L channel matches
  perceived lightness across hues. Back-panel rule: lightness ×0.76 for
  light colors, −0.10 for mid colors, +0.10 for near-black (L < 0.10,
  where darkening further is impossible). A minimum ΔL of 0.10 is
  enforced and tested for every preset and the pure black/white/saturated
  extremes. Hue is kept, chroma slightly muted (×0.9); out-of-gamut
  results are gamut-mapped by shrinking chroma toward neutral (binary
  search), never by per-channel clamping (which shifts hue).
- **Patterns** (vertical / horizontal stripes, checkerboard) live only on
  the front panel, clipped in SVG via `clipPath` and in tiny-skia via a
  `Mask` — verified against the actual tiny-skia 0.12.0 source
  (`Mask::fill_path` + `fill_path(..., Some(&mask))`); no resvg needed.
  Cells: bar width 4/6/9/13/18 px (5 steps) at canvas scale, stripes have
  an equal gap (period = 2 × width).
- **Pattern anti-noise rule (small sizes):** when the standard cell would
  shrink below 2 device pixels (frames ≤ 32 px with thin cells), the cell
  is widened to exactly 2 device pixels and the pattern is REGENERATED in
  canvas units for that frame, so bars stay crisp instead of dissolving
  into gray noise. The 256 frame and the large preview always use the
  standard cell.
- **HEX input accepts `#RGB` and `#RRGGBB`**; validation lives once in
  Rust (`Rgb::parse`), the TypeScript mirror never decides validity.
- **Pattern contrast hint** fires below WCAG 1.5:1 between pattern and
  base color — a decorative overlay stays visible below the QR tool's
  3.0 scannability threshold, but at 1.5 it melts into the panel.
  Non-blocking by design.

### ICO and rendering

- **No new crates.** The ICO container comes from the `image` crate's
  `ico` feature, which only enables `bmp`+`png` code paths that were
  already compiled in (verified in image 0.25.10's Cargo.toml). Frames
  16–128 px are 32bpp BITMAPINFOHEADER DIBs built here (bottom-up BGRA
  + 1bpp AND mask whose bits are set only where alpha = 0); the 256 px
  frame is PNG-compressed (explicitly allowed by the format, spec 4.1).
  Read-back tests decode the file through image's own independent
  ICO+BMP decoder, so builder and verifier are separate implementations.
- **Content-derived icon file name** (`erx-folder-icon-<8 hex>.ico`,
  FNV-1a of the .ico bytes): a changed color always produces a new file
  name, so Explorer's icon cache can never show a stale image. Same
  content → same name, making re-apply idempotent (tested byte-exactly).
- **ARM64:** `cargo check --target aarch64-pc-windows-msvc` PASSES
  (verified 2026-11-04 with windows-sys 0.61, tiny-skia 0.12, image
  0.25). Everything is pure Rust; no C dependencies.

### Applying and restoring (desktop.ini)

- **The icon path is RELATIVE** (`IconResource=<file name>,0`) so the
  icon survives moving/renaming/copying the folder. **Verified working by
  the owner on Windows 11 (2026-11-04): the icon stayed after rename and
  move.** No absolute-path fallback needed.
- **Shell refresh is targeted:** `SHChangeNotify` with UPDATEITEM on the
  folder + UPDATEDIR on the parent per folder — deliberately not the
  heavy global "association changed" broadcast. **Verified by the owner
  on Windows 11: the icon appears and changes without logoff or manual
  F5.**
- **desktop.ini is preserved byte-exactly** outside the one edited line:
  other sections/keys (InfoTip, LocalizedResourceName, [ViewState], …),
  per-line endings (CRLF/LF kept, even mixed), and the file encoding —
  UTF-16 LE with BOM, UTF-8 with/without BOM, or ANSI. ANSI is handled as
  latin-1, which round-trips every byte value exactly (edited lines are
  pure ASCII, so Windows-1252 content we keep survives unchanged). A
  file that cannot be decoded safely (broken UTF-16, duplicate
  [.ShellClassInfo] sections) is NEVER overwritten — the folder is
  marked Failed with a clear message (tested).
- **Encodings for NEW desktop.ini files:** UTF-8 without BOM — the
  content this tool writes is pure ASCII, which every Explorer version
  reads correctly.
- **Attributes:** desktop.ini and the .ico get Hidden + System (cleared
  before rewriting, set again after — replacing a Hidden+System file by
  rename would otherwise be refused by Windows). The folder gets the
  Read-only attribute (preserving its other attributes) because the Shell
  only processes desktop.ini on Read-only/System folders. Read-only on a
  folder does NOT make its contents read-only — it is a flag the Shell
  uses to look for desktop.ini. Verified in practice by the owner's
  tests (files inside stayed writable).
- **Safety:** writes go through temp-file-then-rename inside the same
  folder; every failure path rolls back exactly what it changed (icon
  file removed if newly created, folder attribute reverted if we added
  it). Only files matching `erx-folder-icon-<8 hex>.ico` are ever
  deleted — foreign .ico files and foreign desktop.ini keys survive both
  apply and restore (tested). Restore removes the IconResource key(s)
  (ours or foreign — "restore default" means the default icon), deletes
  only our .ico files, deletes a desktop.ini that has no `Key=Value`
  lines left and then clears the Read-only attribute.
- **No permission pre-check on inspect:** Windows offers no reliable
  non-writing way to test directory write access, so inspecting never
  writes; permission problems surface at apply time from the real error
  (PermissionDenied → "No permission to modify this folder."). The tool
  never asks for or uses admin rights.

### Folder eligibility (the final list — spec 4.3)

One data-driven function (`eligibility.rs`), environment from Windows
known-folder APIs (`SHGetKnownFolderPath`, so redirected locations are
honored), never hardcoded drive letters. Paths are resolved first
(canonicalize → strip the `\\?\` verbatim prefix → case-insensitive
component compare; trailing separators and `..` collapse during
canonicalize).

**Blocked** ("Icon change is not available for this folder." + reason):
1. Not an existing directory (files, missing paths, .zip files).
2. Drive roots (`C:\`, `C:`, `D:\`).
3. Protected system locations — everything under: the Windows directory,
   Program Files, Program Files (x86), and (as the first folder on any
   drive root, themselves and everything under them) $Recycle.Bin, System
   Volume Information, Recovery, Boot, Config.Msi, MSOCache, PerfLogs,
   Documents and Settings. Root ONLY: the ProgramData root, the Users
   root (their subfolders stay allowed, per spec).
4. Known user folders THEMSELVES (subfolders stay allowed): user profile
   root, Desktop, Documents, Downloads, Music, Pictures, Videos,
   Favorites, Links, Saved Games, Searches, Contacts, AppData (derived
   as RoamingAppData's parent), AppData\Roaming, AppData\Local, the
   OneDrive root — all from the known-folder API, so redirected
   locations (e.g. Documents moved to D:) are covered. Reason: Windows
   manages their desktop.ini itself.
5. Symlinks, junctions and mount points as the dropped folder
   (FILE_ATTRIBUTE_REPARSE_POINT).
6. Network locations: UNC paths (covers WebDAV's `@SSL`/`DavWWWRoot`
   forms) and mapped drives reported DRIVE_REMOTE by GetDriveTypeW.
7. Paths whose icon file would exceed 259 UTF-16 units (MAX_PATH − NUL;
   the Shell parses desktop.ini IconResource without long-path
   awareness): folder path + separator + the 28-character icon file name.

**Allowed with warning** (non-blocking): folders inside OneDrive (known
folder `FOLDERID_SkyDrive` plus the OneDrive/OneDriveConsumer/
OneDriveCommercial env vars). Reason: sync clients may skip or reset
hidden system files such as desktop.ini.

**Unverified / known limits:**
- Other cloud providers (Dropbox, Google Drive, …) are not detected —
  they have no known-folder ID and no standard env var. They simply get
  no warning; whether their clients keep desktop.ini is unverified.
- Windows 10 was NOT tested (no device available). The mechanisms used
  (desktop.ini, SHGetKnownFolderPath, SHChangeNotify) are documented as
  available since Vista/Windows 7, but nothing was verified on Windows 10.

### Batch processing

- **Sequential, never parallel** (spec 4.4) — unlike the Image
  Converter's worker pool, folder operations touch the Shell and must
  notify in order.
- **Hard cap 100 folders** (`MAX_BATCH`), enforced in Rust by the batch
  commands and mirrored in the UI list; tested with a real 100-folder
  apply+restore loop.
- **Per-folder results** (Applied / Restored / Skipped / Failed + reason)
  stream through a Tauri Channel; eligibility is re-checked right before
  each folder (blocked → Skipped, not Failed). One failing folder never
  stops the batch. Cancel skips the remaining folders.

### UI

- **Live previews from the exact .ico data:** the small 16/32/48 px
  previews are PNG data URLs rendered by the same rasterizer that builds
  the .ico frames — what the UI shows at small sizes IS the icon data.
  The large preview is the SVG (identical geometry, 256 design).
- **The pattern color control is disabled while the pattern is off**, so
  its state can never be invalid when needed.
- **No new Tauri permissions:** dialogs open from Rust (the dialog
  plugin was already present for earlier tools); the capability file is
  unchanged. The command list grew only by Folder Icon commands; the
  unused single-folder apply/restore commands from the 4B test phase were
  removed again (minimal command surface).

## Folder Icon refresh fixes (Prompt 4.1, Part A)

### A0 — what the code did BEFORE (recorded from the code, not memory)

Write order per applied folder (`apply_icon` before 4.1-A1):

1. Eligibility re-check.
2. Build the .ico bytes, derive the content-hash file name.
3. **The folder's Read-only attribute was set FIRST** (preserving other
   attributes), before any file was touched.
4. The .ico was written (target attributes cleared, temp+rename, then
   Hidden+System set immediately on the new file).
5. desktop.ini was read, parsed (never overwritten when unreadable),
   edited and written the same way.
6. Previous ERX icon files (older hashes) were deleted.
7. Notification: `SHCNE_UPDATEITEM` for the folder and `SHCNE_UPDATEDIR`
   for its parent, both with `SHCNF_PATHW`, **without** `SHCNF_FLUSH`,
   **without** a PIDL form, **without** `SHCNE_ATTRIBUTES`, **without**
   an UPDATEDIR for the folder itself, and **without** any retry.

`restore_icon` notified with the same weak pair.

Owner's evidence on Windows 11: the on-disk state was always correct and
F5 always showed the right icon, but the new icon often appeared only
after the NEXT Apply "pushed" it — a view-update race, not a data problem.

### A1 — write order AFTER

`.ico` written → desktop.ini written → attributes on BOTH files
(Hidden + System) → **the folder's Read-only attribute LAST** (preserving
others) → all handles closed (std fs closes on drop; nothing is held) →
notification. Writes go through temp+rename inside the folder; on any
failure after the first write the previous desktop.ini is put back
byte-exactly (or deleted if none) and our icon files are removed — the
folder attribute needs no revert because it is set last.

### A2 — per-folder notification AFTER

Per folder, after the last file operation: `SHCNE_UPDATEITEM`,
`SHCNE_ATTRIBUTES`, `SHCNE_UPDATEDIR` for the folder and
`SHCNE_UPDATEDIR` for its parent — **each event in both forms**:
`SHCNF_PATHW` (path) and `SHCNF_IDLIST` (PIDL from `SHParseDisplayName`;
freed with `ILFree`), all with `SHCNF_FLUSH`. Flag check against the
current SHChangeNotify documentation and windows-sys 0.61: the PIDL form
is `SHCNF_IDLIST` (0) — **there is no `SHCNF_PIDW` in the current flag
set** (the docs list IDLIST/PATHA/PRINTERA/PATHW/PRINTERW + FLUSH
0x1000 / FLUSHNOWAIT 0x3000). `SHParseDisplayName` is used for the PIDLs.

### A3 — deferred retry

The same notification set is sent again `DEFERRED_NOTIFY_DELAY_MS = 300`
ms later by a dedicated worker thread (`notification.rs`): pushing a job
never blocks the UI or the batch loop, the queue is bounded
(`DEFERRED_QUEUE_CAP = 64`, oldest dropped, duplicates for the same path
coalesced), and the thread is a daemon — the process exiting (app close)
stops it. Nothing is kept alive across app restarts; no icon-cache files
are touched and explorer.exe is never killed.

### A4 — global broadcast per batch

After a WHOLE batch finishes (apply or restore) ONE
`SHCNE_ASSOCCHANGED` with `SHCNF_IDLIST | SHCNF_FLUSH` is sent — never
per folder. It makes every Explorer window rebuild its icon
associations; a brief flicker in open Explorer windows is possible and
expected.

### A5 — on-demand refresh

The "Refresh icons" button sends the same global broadcast on demand
(command `folder_icon_refresh_icons`). It is available even with an
empty list, for folders customized before this fix.

### What each strategy is for (A6 summary)

1. Immediate per-folder set (UPDATEITEM + ATTRIBUTES + UPDATEDIR on the
   folder, UPDATEDIR on the parent; path + PIDL forms; flush) — tells the
   Shell this specific item changed and refreshes the views showing it.
2. Deferred retry after 300 ms — second chance for views that miss the
   first notification while the folder's Read-only attribute changes
   (the race the owner observed).
3. One global ASSOCCHANGED per batch / on demand — the heavy hammer:
   rebuilds associations everywhere; last resort, so it is never per
   folder.

The content-derived icon file name stays as-is: different content →
different name, so the icon cache cannot show stale bytes (spec 4.1-A6
declines a nonce unless evidence shows a stale-cache problem — it does
not; the owner's evidence pointed at view refresh, not cache).

### Honest limits

Whether the combination is enough on every machine/view cannot be proven
from code: the owner's refresh table (docs/folder-icon-manual-checklist.md)
decides. If some rows still need F5, the next escalation would be (in
order): a longer/multiple deferred retries (e.g. 300 ms + 1.5 s), sending
`SHCNE_RENAMEFOLDER` (renaming a folder in place), or SHCNE_ASSOCCHANGED
per folder — the last one is exactly what the Refresh button already
covers manually.

### Folder icon cell fill (Prompt 4.1, Part B)

- **B1 — measured BEFORE:** the opaque bounding box of our rendered icon
  was 208×156 px on the 256 canvas (81.25% width, 60.9% height; 24 px
  side margins, 50 px top/bottom) — visibly smaller than Windows' own
  folder glyphs, which fill the cell far more. (Measured programmatically
  by rendering each .ico size and scanning alpha > 0; the owner's
  screenshot at C:\dev\size-compare.png told the same story.)
- **B2 — AFTER:** the artwork is rescaled to **248 × 186 px** (a uniform
  ×1.1923; the 4:3 folder aspect is kept) = **96.875% of the canvas
  width**, side margins 4 px. Placement: **vertically centered** (35 px
  top and bottom clearance) — Windows' own folder glyphs sit optically
  centered in their cell, and with our wider-than-tall shape centering
  keeps the tab from crowding the cell's top edge at 16 px, where the
  whole folder is ~15 px wide and a 1 px placement difference is
  imperceptible. Corner radii and the tab slope were scaled with the
  artwork (PANEL_RADIUS 14 → 17), so tab, corners and highlight are not
  clipped — pinned by tests (exact 256 bbox 4..251 × 35..220; tab top
  edge drawn; corner arc shows anti-aliased partial pixels).
- **B3 — native rendering with pixel snapping:** every .ico frame is
  rendered from the vector geometry at its own size (never downscaled
  from 256 — that was already true); additionally every path point is
  now snapped to the integer device grid after scaling (`device_path`),
  so panel edges land exactly on pixel boundaries and small sizes stay
  crisp. The 256 frame is unaffected (scale 1 snaps to the design's own
  integer grid). A checkerboard quirk this exposed: the last row/column
  of checker cells now clamp to the panel edge instead of overhanging
  the canvas (cell sizes 13/18 do not divide 248 evenly).
- **B4 — one geometry:** preview SVG and all .ico frames still come from
  the same `Drawing` (both consume `IconParams::drawing*()`), so preview,
  .ico and Explorer agree. The small 16/32/48 px previews remain the
  exact .ico frame bitmaps.
- **B5 — measured AFTER (opaque bbox incl. anti-aliased edge pixels):**
  16 px → 16×12 (margins 0), 24 → 24×18 (0), 32 → 32×24 (0), 48 → 48×36
  (0), 64 → 62×48 (1), 128 → 124×94 (2), 256 → 248×186 (4). Tests assert
  ≥95% width at every size, ≤1 px side margin at 16/24/32, the 248:186
  aspect (±1 px + 3% snapping tolerance), bbox strictly inside the
  canvas, and the exact 256 bbox. Pattern and color behavior unchanged:
  patterns still clip to the front panel (a new test pins the bar/gap
  pixels at 16 and 256 with the snapped geometry), extreme colors keep
  the minimum panel lightness difference (tests untouched and passing).
- **B6 — existing icons:** folders already customized keep their old
  (smaller) look until re-applied — the on-disk .ico is only replaced
  when the user applies again, and the new content automatically gets a
  new file name, so no stale cache is possible.

## Folder Icon: diagnosis "icons do not appear while folders are in the list" (Prompt 4.2)

### Conclusion first (spec 4.2-A5)

**H2 is confirmed: Explorer was observing broken intermediate states of
desktop.ini. H1 is NOT confirmed: a full handle/lock audit found nothing
held open by our code.** During a re-apply on a folder that already had a
custom icon, the OLD desktop.ini had its Hidden+System attributes CLEARED
first (needed for the rename-replace), and the renamed-in file lacked its
final attributes until a moment later. The folder itself was still
Read-only from the previous apply, so Explorer kept consulting
desktop.ini and could read it exactly during that window - where a
desktop.ini without Hidden+System is ignored by the Shell. That evaluates
the folder to the DEFAULT icon and the negative result sticks. This
explains every reported symptom: only previously-customized folders
flashed to default (first-apply folders are not Read-only mid-sequence,
so the Shell never consults desktop.ini during it), a later batch's
ASSOCCHANGED re-read everything and "pushed" the earlier folders'
refresh, and the on-disk state was always correct afterwards.

### A1 - handle and lifetime audit (what was found: nothing held)

Every filesystem access in the Folder Icon backend is a short-lived
`std::fs` call: read, write, rename (MoveFileExW REPLACE_EXISTING),
read_dir, canonicalize, Get/SetFileAttributesW. All of them open and
close their handle within the call; std fs opens with
FILE_SHARE_READ|WRITE|DELETE. **No handle, File, or directory reference
is stored anywhere**: no Tauri managed state beyond the batch cancel
flag (an AtomicBool), no struct fields, no statics, no worker threads or
channels holding paths-with-handles (the deferred-notification queue
holds only `PathBuf`s and opens nothing itself), no watchers (no notify
crate, no ReadDirectoryChangesW). Temp files (`.erx-tmp`) are deleted on
every failure path; on success the temp BECOMES the target (no leftover).
The rollback's "previous desktop.ini" lives only as bytes in memory
inside the apply call. Inspect calls (on add, per row) are stateless
Rust: GetFileAttributesW + canonicalize, everything closed on return.
The frontend list rows hold only path strings, status enums and
timestamps - no thumbnails, no open-folder calls, nothing that touches
the folder while it sits in the list; the shared DropZone only forwards
drag-drop path strings. The WebView2 process receives path strings only
(drag-drop payload, dialog results) - it has nothing to hold file
handles with.

### A2 - lock probe (evidence)

`probe.rs` (spec 4.2-A2): exclusive open with share mode 0
(ERROR_SHARING_VIOLATION detection), a rename-the-folder-and-back probe,
and a Restart Manager listing (RmStartSession/RmRegisterResources/
RmGetList/RmEndSession, verified against windows-sys 0.61 and the
official docs). Results:

- Automated test `nothing_holds_the_files_after_apply`: after a real
  apply, desktop.ini, the .ico and the folder are all exclusive-openable
  and the folder is renamable back and forth - **nothing in our process
  holds anything** (H1 false for erxtools.exe).
- The dev-only command `folder_icon_debug_probe` (registered only in
  debug builds) exposes the same probe plus the Restart Manager list to
  the real UI flow; the owner can verify with Resource Monitor too
  (manual row 8). RM listing from inside a test could only see the test
  process anyway; the exclusive-open probe already covers "any process".

### A3 - event timeline (trace before the fix)

Dev-only tracing (`trace()`, ERX_FOLDER_ICON_TRACE=1 in debug builds,
elapsed milliseconds, no user data). Re-apply on a folder with an
existing custom icon, BEFORE the fix:

```
write: target attrs CLEARED (intermediate state begins)   <- desktop.ini visible/non-system
write: rename done (target still lacks final attrs)        <- new content, still non-system
apply: ini attrs set (desktop.ini final)                   <- final state, ~1ms later
```

Two observable windows exist where a Read-only folder's desktop.ini
lacks Hidden+System (and one moment where it is replaced by a
not-yet-hidden file). Explorer's change-notification processing is
asynchronous - it can read milliseconds later, but it reacts to the
events those steps emit and caches what it saw. After the fix:

```
write: ReplaceFileW done (attrs intact throughout)   <- single atomic swap, never non-system
apply: files final (desktop.ini hidden+system throughout)
```

The new sequence: .ico (new content-derived name) written final ->
desktop.ini temp written + given final attributes -> ReplaceFileW swaps
content atomically -> folder attribute (no-op on re-apply) -> old .ico
deleted (state already final) -> verified read-back -> notify. At NO
moment is desktop.ini missing, non-hidden/system, empty or partial, and
every intermediate state a watcher can observe is a VALID custom-icon
state (either the old or the new icon).

### B - fixes implemented (spec 4.2-B1..B5)

- **B2 (the fix):** `replace_file` no longer clears the target's
  attributes and never delete-then-creates. It writes the temp file, sets
  Hidden+System on the TEMP, and swaps it in with `ReplaceFileW`
  (REPLACEFILE_IGNORE_MERGE_ERRORS; docs-verified 2026-11-04: it merges
  the replaced file's attributes onto the replacement and handles the
  target without pre-clearing). New targets use a plain rename. The .ico
  always uses a content-derived name, so a color change never overwrites.
- **Skip-identical:** when the .ico or desktop.ini already holds exactly
  the final bytes with final attributes, nothing is written at all - a
  same-color re-apply produces zero file-system writes (tested: modified
  times unchanged).
- **B3:** notifications remain strictly after everything is finished
  (all handles closed before the notify); the 300 ms deferred retry from
  4.1 is kept; one ASSOCCHANGED still runs once per finished batch and
  from the Refresh button. No settle delay was added: the evidence
  pointed at broken intermediate states, not at notification timing.
- **B4:** every apply now ends with a verification read-back
  (`verify_applied`: desktop.ini parses and points at the new .ico, both
  files Hidden+System, folder Read-only); a failed verification rolls
  the apply back and reports an error instead of pretending success.
  The batch summary gained the honest note "If Explorer still shows the
  old icon, press F5 or use Refresh icons." - justified by the evidence
  that Windows itself does not always repaint immediately in every view.
- **B1:** nothing to remove - the audit found no handles (recorded
  honestly rather than inventing a fix).
- **B5 tests:** `replace_file_keeps_hidden_system_through_replacement`
  (content swapped, attributes intact, no temp leftovers),
  `identical_reapply_skips_the_writes` (modified times unchanged),
  `verification_detects_wrong_content`, `nothing_holds_the_files_after_apply`
  (exclusive open + folder rename probe); rollback byte-exactness, the
  100-folder batch and the non-blocking scheduling tests are unchanged
  and pass. 200 tests total.

### Honest limits

Whether the fix removes the default-icon flash on EVERY view on the
owner's machine must come from the 4.2 manual table. If some row still
fails, the next candidates (in order) are: a longer settle delay before
the immediate notification, sending the notification for the PARENT
before the folder, or per-folder ASSOCCHANGED (the Refresh button
already covers it manually). The Restart Manager probe is available in
debug builds if a third-party process (e.g. a sync client) turns out to
hold the files.
