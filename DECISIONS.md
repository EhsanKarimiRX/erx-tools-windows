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
