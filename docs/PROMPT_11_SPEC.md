# PROMPT 11 SPEC: "Site Icons" tool (favicon generator)

## 1. What is this tool?

A favicon is the small icon a browser shows in the tab, bookmarks and history for a website. Modern websites also need larger icons: an Apple touch icon (iPhone/iPad home screen), Android/PWA icons declared in a web manifest, and an HTML snippet that links all of them.

"Site Icons" is a tool inside ERX Tools. The user drops ONE square-ish logo image (PNG, JPG, WebP, BMP, GIF, TIFF, ICO or SVG) and the tool generates a complete "favicon package" for a website: favicon.ico, PNG icons, an Apple touch icon, Android icons, a web manifest and a ready-to-paste HTML snippet. The tool replaces an older Python program (RXfavicongenerator.py) and improves on it. Everything runs locally, offline, in Rust.

## 2. Hard rules for the coding agent (apply to EVERY step)

1. Touch only the files that the current step lists.
2. Never import code from another tool (not in TypeScript, not in Rust). If a pattern is useful (for example the ICO writer in folder_icon), read it and write a small own copy inside site_icons.
3. Do not add a dependency unless the step says so.
4. All user-visible text is English.
5. After each step run the commands the step lists and paste the real output. If something fails, STOP and report the exact error. Do not improvise or "fix" unrelated code.
6. Never run git commit or git push.
7. Pure logic goes in core-style modules (no Tauri types) so it is unit-testable. Tauri code stays in commands.rs.

## 3. Output files (the "favicon package")

Written into a new folder named `<site_name>_favicon_package` (unique: if it exists use `_1`, `_2`, ...).

| File | Size | Rules |
|---|---|---|
| favicon.ico | 16, 32, 48 in one file | uses padded base (favicon padding) |
| favicon-16x16.png | 16x16 | favicon padding applied |
| favicon-32x32.png | 32x32 | favicon padding applied |
| favicon.svg | n/a | ONLY when the source is SVG: the original file bytes copied unchanged |
| apple-touch-icon.png | 180x180 | NO padding; flattened (opaque) on background_color |
| android-chrome-192x192.png | 192x192 | NO padding; transparency kept |
| android-chrome-512x512.png | 512x512 | NO padding; transparency kept |
| android-chrome-maskable-512x512.png | 512x512 | ONLY if include_maskable; logo scaled to 60% of the canvas (20% padding per side), opaque background_color |
| site.webmanifest | n/a | built with serde_json (never string formatting) |
| favicon-snippet.html | n/a | HTML <head> snippet |

### site.webmanifest fields
name, short_name (both = site display name; if empty use "Website"), icons (192 and 512; plus the maskable one with "purpose": "maskable" when generated; src values start with "/", e.g. "/android-chrome-192x192.png"; type "image/png"), theme_color, background_color, display "standalone". Pretty-printed JSON with a trailing newline.

### favicon-snippet.html content
```html
<link rel="icon" href="/favicon.ico" sizes="32x32">
<link rel="icon" href="/favicon.svg" type="image/svg+xml">  <!-- only when source is SVG -->
<link rel="icon" type="image/png" sizes="32x32" href="/favicon-32x32.png">
<link rel="icon" type="image/png" sizes="16x16" href="/favicon-16x16.png">
<link rel="apple-touch-icon" href="/apple-touch-icon.png">
<link rel="manifest" href="/site.webmanifest">
<meta name="theme-color" content="#ffffff">
```
(theme-color content = the chosen theme color, normalized to lowercase #rrggbb)

## 4. Options (SiteIconsOptions)

| Option | Type | Default | Notes |
|---|---|---|---|
| site_name | string | "" | display name; also used for the folder name |
| padding_percent | number | 0 | clamp 0..=45; ONLY for favicon files (ico, 16, 32) |
| trim_transparent | bool | true | crop fully transparent border before squaring |
| background_color | "#rgb" or "#rrggbb" | "#ffffff" | apple icon, maskable icon, manifest background_color |
| theme_color | "#rgb" or "#rrggbb" | "#ffffff" | manifest theme_color + meta theme-color |
| include_maskable | bool | true | |

Colors are validated and normalized to lowercase "#rrggbb". Invalid color -> a clear error.

## 5. Image pipeline (exact order)

1. LOAD. Read the file bytes. Reject files larger than 64 MiB. Detect the type by CONTENT, never by extension: first `image::guess_format(&bytes)`; if it succeeds, it is a raster image. If it fails, check whether the bytes look like SVG (UTF-8 text containing `<svg` in the first 4096 bytes). Otherwise return "not supported". For raster images read the dimensions first (without full decode) and reject width or height equal to 0 or above 10000. Decode to RGBA8 (first frame for GIF).
2. SVG. Parse with usvg (default options, NO fonts loaded, no external resources). Render with resvg/tiny-skia so that the LONGEST side is 1024 px (keep aspect ratio), on a transparent pixmap. Convert the premultiplied pixmap to straight (non-premultiplied) RGBA. From here on an SVG is just a raster "master" image. Keep the original SVG bytes for favicon.svg. Known limit: SVG text is not rendered (tell the user to convert text to paths).
3. TRIM (if trim_transparent). Find the bounding box of pixels with alpha > 0 and crop to it. If there is no such pixel, error "The image is fully transparent".
4. SQUARE. Create a transparent square canvas with side = max(width, height) and paste the image centered (x = (side - w) / 2, y = (side - h) / 2, integer division). Result = `base`.
5. FAVICON BASE. If padding_percent == 0, favicon_base = base. Otherwise content = max(1, round(side * (100 - 2*p) / 100)); resize base to content x content, paste centered on a transparent canvas of the same side as base.
6. RESIZE. One helper `resize_square(img, size)`: if already that size return a clone. Otherwise: convert to premultiplied alpha, `image::imageops::resize` with FilterType::Lanczos3, convert back to straight alpha (divide by alpha when alpha > 0). This avoids dark fringes at transparent edges.
7. FLATTEN helper `flatten_on_color(img, rgb)`: result = src*a + bg*(1-a) per channel, alpha 255.
8. ENCODE. PNG with the `image` PNG encoder (best compression if the API allows, otherwise default). ICO with image::codecs::ico::IcoEncoder (read src-tauri/src/tools/folder_icon/ico.rs for the pattern; write own copy).

## 6. Backend layout (src-tauri/src/tools/site_icons/)

- mod.rs: module declarations and re-exports
- source.rs: load + detect + SVG render + warnings (pure logic)
- render.rs: trim, square, padding, resize_square, flatten (pure logic)
- ico.rs: ICO writer (pure logic)
- package.rs: file list builder: returns Vec<(file_name, bytes)>, manifest JSON, HTML snippet, folder-name sanitizing, unique folder (pure logic, only std::fs for the folder helper)
- commands.rs: Tauri commands + DTOs (the only file that uses tauri types)

### Folder name rules
sanitize: trim; keep chars where `char::is_alphanumeric()` or '-' or '_'; a space becomes '_'; drop everything else; strip leading/trailing '_'; empty -> "website"; truncate to 40 chars; if the lowercase result is a Windows reserved name (con, prn, aux, nul, com1-com9, lpt1-lpt9) append "_site".

### Commands (all `#[tauri::command(async)]` so heavy work never blocks the UI thread)
1. `pick_site_icon_source(app) -> Option<String>`: native open-file dialog (filter: images + svg).
2. `inspect_site_icon_source(path) -> Result<SourceInfoDto, ToolError>`: file_name, kind ("raster" | "svg"), width, height, warnings (Vec<String>), preview_data_url (PNG data URL, max 256 px, base64).
3. `preview_site_icons(path, options) -> Result<PreviewDto, ToolError>`: icons (file_name, width, height, data_url) for every generated PNG, plus html_snippet and warnings.
4. `save_site_icons(app, path, options) -> Result<Option<SaveResultDto>, ToolError>`: opens a native "choose output folder" dialog, creates the unique package folder, writes all files, returns folder path and file list with byte sizes. Returns None if the dialog is cancelled. If any write fails, remove the files and the folder this call created, then return the error.
5. `reveal_site_icons_folder(folder) -> Result<(), ToolError>`: show the folder in Explorer (same technique as image_convert reveal_outputs).

### Warnings (computed from the source, English text)
- shorter side < 512 px (raster): "The source is small (WxH px). Larger icons will look blurry. Use at least 512x512."
- non-square after trim (ratio > 1.05): "The image is not square. It will be centered on a transparent square canvas."
- JPEG or BMP: "This format has no transparency, so the logo background will stay in the icons."

## 7. Frontend (src/tools/site-icons/)

Files: index.tsx, types.ts, site-icons.css (class prefix `si-`), optional small components. Registered in src/tools/registry.ts as id "site-icons", name "Site Icons", description "Create favicon and web app icons from your logo", status "ready", lazy loaded, lucide-react icon.

UI top to bottom:
1. One short explanation line (what a favicon is).
2. Shared `DropZone` (src/shared/DropZone.tsx) + browse button (uses pick_site_icon_source). Only the first dropped file is used.
3. Source card: thumbnail, file name, kind, dimensions, warnings.
4. Options: site name (text), padding % (number or slider 0-45), trim checkbox, background color, theme color, maskable checkbox. Colors use a native `<input type="color">` plus a hex text field with validation (do NOT import folder-icon ColorControl).
5. Preview grid: every generated PNG with its file name and size, drawn on a checkerboard so transparency is visible. Preview requests are debounced (250 ms) and stale responses are ignored (request counter).
6. Primary button "Generate and save": calls save_site_icons. On success show the folder path, the list of files, a "Show in folder" button, and the HTML snippet in a `<pre>` with a "Copy" button.
7. Errors shown inline with role="alert". Status text in an aria-live region.
Follow existing theme variables and the shared classes .button-primary / .button-secondary. Light/dark works through theme.css variables.

## 8. Required Rust unit tests (generate all images in code, no binary fixtures)

- sanitize_folder_name: spaces, unicode letters kept, symbols dropped, empty, reserved names, long names
- unique folder naming (_1, _2)
- hex color parse: #rgb, #rrggbb, uppercase, invalid
- trim: bounding box correct; fully transparent image -> error
- square: wide and tall images end centered
- padding: p=0 unchanged, p=25 content is 50% of the side, p clamped to 45
- resize_square: output size correct; a half-transparent white pixel region keeps white color (no dark fringe)
- ICO bytes: header type = 1, image count = 3, entry sizes 16/32/48
- every PNG output decodes to the expected size
- apple icon: every pixel alpha = 255; maskable icon: every pixel alpha = 255
- manifest: parses back with serde_json; a site name containing a double quote stays valid JSON; empty name -> "Website"
- html snippet: contains the svg line only for SVG sources
- source detection: PNG bytes -> raster, small inline SVG string -> svg, random bytes -> not supported
- SVG render: a tiny inline SVG with a solid red square produces a master whose center pixel is red and opaque
- size/dimension limits rejected with clear errors

## 9. Build phases
- Phase 1 (this file + info gathering): no code.
- Phase 2: Rust core (dependencies, source.rs, render.rs, ico.rs, package.rs, tests). No Tauri commands yet.
- Phase 3: commands.rs, mod.rs / lib.rs wiring, backend tests, clippy.
- Phase 4: frontend and registry entry.
- Phase 5: polish, manual test checklist, docs (README tool list, PROGRESS.md, DECISIONS.md, docs/releases/NEXT.md).

## 10. Input format handling and Phase 2 decisions (these OVERRIDE section 5 step 1 where they differ)

Accepted raster formats (detected by file CONTENT, never by extension): PNG, JPEG, WebP, BMP, GIF, TIFF, ICO. Plus SVG (plain UTF-8 text).

Detection order for the bytes:
1. File must not be empty and must be at most 64 MiB (const MAX_FILE_BYTES).
2. `image::guess_format(&bytes)`:
   - Ok(Png | Jpeg | WebP | Bmp | Gif | Tiff | Ico): decode as raster.
   - Ok(any other format, e.g. Avif, Tga, Pnm, Dds, Hdr, OpenExr, Qoi, Farbfeld): error "<Name> images are not supported. Convert the image to PNG first."
   - Err: continue with step 3.
3. Magic-byte checks for known-but-unsupported files (friendly errors):
   - bytes 4..8 are "ftyp" and bytes 8..12 are one of heic, heix, hevc, hevx, mif1, msf1: "HEIC/HEIF images are not supported here. Convert the image to PNG first (the Image Converter tool can do this)."
   - starts with "%PDF": "PDF files are not supported. Export the logo as PNG or SVG."
   - starts with "8BPS": "Photoshop (PSD) files are not supported. Export the logo as PNG or SVG."
   - starts with bytes 0x1F 0x8B (gzip, e.g. .svgz): "Compressed files are not supported. Unzip the SVG first."
4. SVG sniff: strip an optional UTF-8 BOM, then check that the first 16384 bytes (lossy UTF-8) contain "<svg". If yes: SVG. (This replaces the 4096 in section 5.)
5. Otherwise error: "This file is not a supported image. Supported: PNG, JPEG, WebP, BMP, GIF, TIFF, ICO, SVG."

Raster decoding rules:
- Read width/height from the decoder BEFORE decoding pixels; reject width or height == 0, or above 10000, or width*height above 100_000_000, with a clear English message.
- Apply the EXIF orientation if the image crate API provides it (ImageDecoder::orientation() and DynamicImage::apply_orientation). If those APIs do not exist in image 0.25.10, skip orientation and report it.
- Animated GIF / WebP: use the first frame (default behaviour).
- Convert to RGBA8 (this also handles 16-bit and grayscale images).
- If decoding fails: print the technical error with eprintln! and return the friendly message "The image could not be decoded. The file may be damaged."

SVG rules:
- The SVG must be valid UTF-8 (after removing the BOM), otherwise: "The SVG file is not valid UTF-8 text."
- Parse with resvg::usvg (re-exported), default options, no fonts. Try to disable loading of non-data image hrefs (local files) through the usvg Options image href resolver; if that API is not simple to use after 2 attempts, leave the default and report it.
- If the SVG parse fails: eprintln! the detail, return "The SVG file could not be read. It may be damaged."
- Width and height of the tree must both be > 0 and finite.
- Render so that the longest side is 1024 px (const SVG_MASTER_SIZE = 1024), transparent background.
- Warning when the SVG source text contains "<text": "This SVG contains text. Text is not rendered. Convert text to outlines (paths) in your vector editor."
- favicon.svg: only when the source is SVG, copy the ORIGINAL bytes unchanged.

Warnings (all English), computed in source.rs, returned with the loaded source:
- raster whose shorter side < 512: "The source is small (WxH px). Larger icons will look blurry. Use at least 512x512."
- JPEG or BMP: "This format has no transparency, so the logo background will stay in the icons."
- SVG with text (above).
- Non-square warning is NOT computed in source.rs (it depends on trim); it is computed in package.rs: after trim, if max(w,h)/min(w,h) > 1.05: "The image is not square. It will be centered on a transparent square canvas."

ICO writing: use image::codecs::ico::{IcoEncoder, IcoFrame}; every frame as PNG (IcoFrame::as_png). Sizes 16, 32, 48.
Dependency: resvg 0.49 with default-features = false and features = ["raster-images"] (so no text/fonts, no svgz).
Use the re-exports resvg::usvg and resvg::tiny_skia (do not add usvg as a separate dependency).

## 11. Naming decision and Phase 3 decisions (these OVERRIDE earlier sections where they differ)

The tool's FINAL name is "Favicons". Everywhere in sections 1-10 where the text says "Site Icons", "site_icons", "site-icons" or the CSS prefix `si-`, use:
- UI name / tile name: "Favicons" (tile description: "Create favicon and web app icons from your logo")
- tool id (registry.ts): "favicons"
- frontend folder: src/tools/favicons/ ; CSS file favicons.css ; CSS class prefix `fav-`
- Rust module: src-tauri/src/tools/favicons/ ; options struct: FaviconsOptions (was SiteIconsOptions)
- Tauri commands (all prefixed `favicons_`):
  1. favicons_pick_source(app) -> Option<String>
  2. favicons_inspect_source(path) -> Result<SourceInfoDto, ToolError>
  3. favicons_preview(path, options) -> Result<PreviewDto, ToolError>
  4. favicons_save(app, path, options) -> Result<Option<SaveResultDto>, ToolError>
  5. favicons_reveal_folder(folder) -> Result<(), ToolError>
  All five use `#[tauri::command(async)]` so heavy work never runs on the UI thread. (The older tools use plain sync commands; this difference is intentional.)

DTO rules: all DTOs use `#[serde(rename_all = "camelCase")]`. Errors are returned as `ToolError::Internal(message)` with the friendly English message produced by the core (the core already returns friendly strings).

DTO shapes:
- FaviconsOptionsDto (Deserialize): siteName: string, paddingPercent: number, trimTransparent: boolean, backgroundColor: string, themeColor: string, includeMaskable: boolean
- SourceInfoDto (Serialize): path, fileName, kind ("raster" | "svg"), formatLabel, width, height, warnings: string[], previewDataUrl (PNG data URL, longest side max 256 px)
- IconPreviewDto (Serialize): fileName, width, height, sizeBytes, dataUrl
- PreviewDto (Serialize): icons: IconPreviewDto[], htmlSnippet, warnings: string[]
- SavedFileDto (Serialize): name, sizeBytes
- SaveResultDto (Serialize): folder, files: SavedFileDto[], htmlSnippet, warnings: string[]

Source thumbnail rule: thumbnail = resize_square(&make_square(&master), min(256, side)) (no upscaling; no trimming), encoded with encode_png, wrapped as data URL.
Preview rule: build the Package with build_package; icons = every GeneratedFile that has width and height (these are the PNG files), each with its data URL. The .ico, .svg, .webmanifest and .html files are NOT part of icons.
Data URL format: "data:image/png;base64," + standard base64 (crate base64 0.22: `use base64::{Engine as _, engine::general_purpose::STANDARD};`).
Save flow order: load source -> build package (so errors appear BEFORE any dialog) -> folder dialog -> write_package. Dialog cancelled -> Ok(None).
Reveal: Err if the folder does not exist; otherwise reveal the file `favicon.ico` inside it with tauri_plugin_opener::reveal_item_in_dir (if favicon.ico does not exist, reveal the folder itself).

## 12. Phase 4 frontend decisions

Folder src/tools/favicons/: types.ts, ColorField.tsx, OptionsPanel.tsx, PreviewGrid.tsx, SnippetPanel.tsx, SavedPanel.tsx, index.tsx (default export `Favicons`), favicons.css (all classes start with `fav-`).
Behaviour:
- Empty state: shared DropZone + two short explanation paragraphs. Only the FIRST dropped file is used.
- Loaded state: compact DropZone (replace image), source card (thumbnail, file name, format, size, warnings), options column on the right (stacks below on narrow windows), preview grid, HTML snippet with Copy button, saved panel after saving.
- Preview: requested through `favicons_preview` 250 ms after any change (debounced); stale responses are ignored (cancelled flag in the effect cleanup); no request while a color field is invalid.
- Save: `favicons_save` (native folder dialog is opened by Rust). null result = user cancelled: do nothing. Changing any option or the source clears the saved panel.
- Copy snippet: navigator.clipboard first, then a hidden textarea + execCommand fallback, then a "select the text" message.
- Errors from Rust arrive as plain strings (ToolError). Show them in a red box with role="alert".
- Accessibility: every input has a label, hints linked with aria-describedby, aria-live for status text, visible focus ring.

## 13. Padding fix (OVERRIDES section 3 table, section 4 and section 12 where they differ)

Two padding options, both 0..=45 %, both clamped with apply_padding (NaN = 0):
- padding_percent / UI "Favicon padding" (default 0): favicon.ico, favicon-16x16.png, favicon-32x32.png (browser tabs).
- app_padding_percent / UI "Home screen icon padding" (default 10): apple-touch-icon.png (180, opaque on background_color), android-chrome-192x192.png, android-chrome-512x512.png (Safari and Chrome home screen shortcuts, installed web apps).
The maskable icon is NOT affected by either option (fixed: logo at 60% of the canvas). DTO field: appPaddingPercent (required). The preview shows icons up to 48 px enlarged to 96 px (pixelated) so padding is visible; the real size stays in the label.
Reason: iPhones round the corners of the Apple icon, so a logo touching the edge looks bad. The old Python program never padded Apple/Android icons.
