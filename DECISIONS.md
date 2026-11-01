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
  conversion (2B) via `ImageDecoder::orientation()` +
  `DynamicImage::apply_orientation()` (image 0.25.8+).
