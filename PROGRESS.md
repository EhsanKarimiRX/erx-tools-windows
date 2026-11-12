# Progress

## Current status

**v0.2.3 release in preparation (2026-11-12).** Ready and in the app:
Image Converter (with HEIC input), QR Code Generator, Folder Icon Changer
(folders and Windows shortcuts, much faster icon update after the Shell
pass), System Manager (Awake, Desktop Icons, Organize, Quick Launch,
Command Tips), the Favicons tool, the About button and the update button
on the home screen. Encrypted Notes is still a placeholder tile. The owner
hand-tested Favicons, Folder Icon (including shortcuts), About and the new
Command Tips on 2026-11-12. Next: the four v0.2.3 installers, then the
GitHub release.

## PROMPT 11 STATUS (Favicons tool - implemented, manually tested)
- Spec: docs/PROMPT_11_SPEC.md. Progress log: docs/PROMPT_11_PROGRESS.md.
- Backend: src-tauri/src/tools/favicons/ (source, render, ico, package, commands). 5 commands registered in lib.rs (favicons_pick_source, favicons_inspect_source, favicons_preview, favicons_save, favicons_reveal_folder).
- Frontend: src/tools/favicons/ (components + favicons.css), registered in src/tools/registry.ts.
- Padding: two options, "Favicon padding" (ico, 16, 32) and "Home screen icon padding" (Apple touch icon, Android 192/512); the maskable icon keeps a fixed margin.
- New dependency: resvg 0.49 (default features off, raster-images on) for SVG input.
- Manual test checklist: docs/favicons-manual-checklist.md — run by the owner on 2026-11-12 (the file's boxes are kept as the template for the next tool).
- THIRD_PARTY_LICENSES.md was regenerated for v0.2.3 (resvg and its crates are now listed).

## PROMPT 4.2 STATUS (FINAL - Prompt 4.3)

- **Prompt 4.3 done (four commits):** (1) the rename nudge REMOVED -
  never rename user folders; a temp-dir test proves apply/restore never
  rename the folder or create siblings; (2) ONE best-effort view refresh
  per batch (`refresh_shell_views`: post the shell view's Refresh command
  0x7103 to every open Explorer view and the Desktop via PostMessageW,
  behind SEND_VIEW_REFRESH=true, fails safe); (3) calm honest UI note
  after batches + the Apply-button fix (Applied/Failed/Skipped rows
  re-arm to Ready when the design changes; Apply targets every
  non-blocked row - re-apply is a no-op via skip-identical); (4) honest
  docs (DECISIONS "Icon refresh investigation (final)" corrects the
  earlier root-cause claims; README states the delay and Win11-only
  testing). **Final honest status: not always instant; usually after
  reopening the folder, sometimes a minute, restart as last resort;
  FolderIco behaves the same on a first apply.** 202 tests, all gates
  green including ARM64. Bisect worktrees and C:/dev/bisect-target
  removed; C:/dev/wip-current.patch + wip-untracked kept for the owner.
- NEXT STEP: owner runs the 4.3 acceptance table. Then Encrypted Notes,
  installers.
- Known issues: icon refresh delay is Windows 11 behavior (unproven
  cause); documented honestly in the UI and README.

## PROMPT 4.2 STATUS (rename nudge - SUPERSEDED, see above)

- **A/B experiment verdict (owner verified): the rename event is the
  trigger.** Two folders, identical apply; B with a real rename
  (MoveFileW + SHCNE_RENAMEFOLDER) turned red instantly; A with
  notifications only stayed default. Implemented: after every
  Apply/Restore the folder is renamed to its own name + invisible
  zero-width space and back (visible name/path unchanged), with
  RENAMEFOLDER events per hop (`nudge_folder_by_rename`). Fallback if a
  folder is locked: silent skip + the F5 hint. Expected: **icons update
  immediately**. 200 tests (+2 nudge tests), gates green.
- NEXT STEP: owner acceptance test - apply to 2 fresh folders, icons
  should appear immediately without F5. Then Encrypted Notes, installers.
- Known issues: none known; the nudge skips silently on locked folders
  (F5 hint covers).

## PROMPT 4.2 STATUS (bisect fix adopted)

- **Bisect done by the owner (five worktree builds): the heavy 4.1-A
  notification layer is the regression that made F5 useless** (L0 and the
  minimal WIP layer both give "F5 fixes it"; L1/L2/L3 all break it).
  Earlier "notification layer exonerated" statement was wrong - the
  "not instant" symptom was the wrong discriminator; F5-reliability was
  the real signal. Adopted: the minimal notification layer (one
  UPDATEITEM PATHW|FLUSHNOWAIT per folder, one ASSOCCHANGED
  IDLIST|FLUSHNOWAIT per batch) as ONE local commit, keeping all 4.1/4.2
  improvements. UI hint now says "press F5 in the Explorer window (or on
  the Desktop)". Honest status: **not instant; F5 reliably refreshes**;
  FolderIco is instant (its EnumWindows + window messages + InvalidateRect
  + COM init + rename mechanisms are a possible future improvement, not
  implemented - see DECISIONS.md Future improvements). Unproven
  hypothesis recorded: the blocking-FLUSH storm + deferred second wave
  poisoned Explorer's cached icon evaluation. Bisect worktrees
  (C:\dev\bisect) and C:\dev\wip-current.patch kept until the owner
  confirms cleanup.
- NEXT STEP: owner acceptance test (restart Explorer, apply to 2 fresh
  folders, press F5). Then Encrypted Notes, installers.
- Known issues: not instant by design of Windows 11's handling; F5 is the
  documented workaround in the UI.

Older 4.2 status (notification experiment):

- **Notification experiment implemented (uncommitted).** Disk comparison
  of C:\dev\FolderIcon vs C:\dev\OurIcon showed structurally identical
  artifacts, so only the Shell notification layer changed to the minimal
  reference pattern: one UPDATEITEM (PATHW | FLUSHNOWAIT) per folder, one
  ASSOCCHANGED (IDLIST | FLUSHNOWAIT) per batch; ATTRIBUTES/UPDATEDIR/
  PIDL duplication, blocking FLUSH and the 300 ms deferred retry removed.
  Also removed from the previous round (owner's UX direction): the
  "Refresh icons" button/command and the debug probe + Restart Manager
  feature (regression probes for tests kept); honest helper text below
  the options. Disk path untouched. 197 tests, gates green. NOT proven
  fixed until the owner's manual test.
- NEXT STEP: owner runs the working-tree build, applies to two fresh
  folders with the app open, reports immediate / after F5.
- Known issues: Explorer behavior unproven; DECISIONS.md records the
  experiment as hypothesis aligned with reference behavior.

Previous 4.2 status (diagnosis + ReplaceFileW fix):

- Checkpoints **4.2-A (diagnosis) and 4.2-B (fix) done.** Conclusion:
  **H2 confirmed** — during a re-apply the old desktop.ini had its
  Hidden+System attributes cleared and the renamed-in file lacked final
  attrs for a moment; a Read-only folder keeps consulting desktop.ini,
  Explorer cached the broken state as "default icon". **H1 not
  confirmed** — the full audit + lock probes found nothing held open
  (exclusive-open/rename probes green in tests; Restart Manager probe
  available via the debug-only `folder_icon_debug_probe` command).
  Fix: `replace_file` now sets final attributes on the TEMP file and
  swaps with `ReplaceFileW` (no attribute clearing, no
  delete-then-create), identical re-applies write nothing, every apply
  ends with a verification read-back (failure → rollback + error), and
  the batch summary carries the honest "press F5 or Refresh icons" note.
  Timeline before/after recorded in DECISIONS.md. 200 tests (+4).
- NEXT STEP: owner runs the 4.2 table in docs/folder-icon-manual-
  checklist.md (especially row 2 — no default-icon flash — and row 8 —
  resmon handle check). Escalation candidates documented if needed.
- Known issues: none known; the fix's real-world effect awaits the
  owner's table.

## PROMPT 4.1 STATUS

- Checkpoints **4.1-A and 4.1-B done.** A: write order fixed (folder
  attribute LAST), full per-folder notification set (UPDATEITEM +
  ATTRIBUTES + UPDATEDIR folder/parent, path AND PIDL forms, SHCNF_FLUSH;
  PIDL = SHCNF_IDLIST — no SHCNF_PIDW exists), deferred 300 ms retry on a
  bounded daemon worker thread, one ASSOCCHANGED per finished batch,
  "Refresh icons" button, constants in `notification.rs`, A0 "before"
  recorded in DECISIONS.md. B: opaque bbox was 81.25% of the cell width —
  rescaled to 248×186 (96.875% width, 4:3 kept, vertically centered),
  every frame rendered natively with integer pixel snapping, checker
  cells clamped to the panel edge, preview still from the same geometry;
  bbox tests pin ≥95% width at every size, ≤1 px margin at 16/24/32 and
  the exact 256 bbox. 196 tests. Gates + ARM64 green.
- NEXT STEP: owner runs the 4.1 manual tables (refresh + size) appended
  to docs/folder-icon-manual-checklist.md. If some refresh rows still
  need F5, the documented escalation order is: longer/multiple deferred
  retries, SHCNE_RENAMEFOLDER, or per-folder ASSOCCHANGED (the Refresh
  button already covers the last one manually).
- Known issues: folders customized before 4.1-B keep the old smaller
  look until re-applied (expected; new content gets a new file name).

Previous status (Prompt 4 body):

- Checkpoint **4.1-A done** (A0–A6 implemented in one pass, as the prompt
  instructs): write order fixed (folder attribute LAST), full per-folder
  notification set (UPDATEITEM + ATTRIBUTES + UPDATEDIR folder/parent,
  path AND PIDL forms, SHCNF_FLUSH; PIDL = SHCNF_IDLIST — no SHCNF_PIDW
  exists), deferred 300 ms retry on a bounded daemon worker thread, one
  ASSOCCHANGED per finished batch, "Refresh icons" button, all constants
  in `notification.rs`. A0 "before" state recorded in DECISIONS.md.
  194 tests (+4 notification tests: non-blocking scheduling, bounded +
  dedupe queue, PIDL resolution, event-set safety).
- NEXT STEP: checkpoint **4.1-B** (icon size: measure opaque bbox,
  rescale to ~97% canvas width, per-size native render with pixel
  snapping, tests).
- Known issues: whether Explorer now refreshes without F5 must be
  confirmed by the owner's refresh table; escalation options are
  documented in DECISIONS.md.

## PROMPT 4 STATUS

- **All sub-steps done.** Last finished: **4C-3** — docs (README tools
  list, DECISIONS.md Folder Icon section with the final eligibility list,
  the owner-verified relative-path and targeted-refresh decisions, and
  the unverified items), unused single-folder commands removed (minimal
  command surface), final quality gates green.
- NEXT STEP: the owner runs docs/folder-icon-manual-checklist.md (rows
  1–19; 4B rows 5–12 already verified). Findings go back into this tool.
- Known issues: none.

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

- Encrypted Notes is still a placeholder tile ("This tool is not built
  yet."); its tile opens so the screen can be seen.
- Folder icons now register quickly through the Shell pass, but Windows
  can still take a while to repaint a view it already cached; the tool's
  note under the result tells the user to reopen the folder or refresh.

## Next steps

1. **v0.2.3 release:** bump done; next is `npm run release:build` (four
   installers + SHA256SUMS), the owner's install check, then the GitHub
   release with the tag `v0.2.3` (owner publishes).
2. After v0.2.3: Encrypted Notes (needs Persian/RTL text support; secret
   state only in Rust), then optional code signing and CI.
3. Later tools: add the next entry to `src/tools/registry.ts` following
   docs/ADDING_A_TOOL.md.

## Open questions

- None currently.
