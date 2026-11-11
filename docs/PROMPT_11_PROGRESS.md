# PROMPT 11 PROGRESS

PHASE 1 done (spec created). PHASE 2 done (Rust core: source.rs, render.rs,
ico.rs, package.rs; 43 tests). PHASE 3 done (rename site_icons->favicons,
commands.rs with 5 commands registered in lib.rs, allow(dead_code) removed;
54 favicons tests). PHASE 4 done (frontend: src/tools/favicons/ with 7 files
+ favicons.css, registry.ts entry). TODO Phase 5: manual GUI test by the
owner, docs (README tool list, PROGRESS.md, DECISIONS.md,
docs/releases/NEXT.md). NOTE: 2 pre-existing clippy
errors in folder_icon/perf.rs (collapsible_if, owner's work) still block
`cargo clippy -- -D warnings`. (A third error, dead_code in
folder_icon/notification.rs, was fixed by the owner's own commit between
Phase 2 and Phase 3.)

Notes from Phase 2 (still true):

- Dependency added: `resvg 0.49` (default-features = false, features =
  ["raster-images"]). `cargo tree --duplicates` reports tiny-skia v0.12.0
  compiled twice (same version, two feature sets: once with the `png`
  feature via image/ico chain, once via resvg) — reported, not fixed.
- usvg: local-file image hrefs are disabled by overwriting
  `options.image_href_resolver.resolve_string` with a resolver returning
  None (same policy as usvg's own `Tree::from_data_nested`); data: URIs
  keep the default resolver.
- EXIF orientation IS applied via `ImageDecoder::orientation()` +
  `DynamicImage::apply_orientation` (image 0.25.10 provides both; JPEG,
  TIFF and WebP decoders implement it).
- The spec's resize_square reference was adapted only where the compiler
  required it: `Rgba<u8>` has no Index impl, so pixels are accessed via
  `.0[c]`; the a==0 branch is expressed with `checked_div` to satisfy
  clippy (identical result: zero-alpha pixels stay all-zero).

Notes from Phase 3:

- `encode_png` in package.rs is now `pub` (used by the thumbnail helper in
  commands.rs). `normalize_hex_color` is used only by tests, so it carries
  `#[cfg(test)]` (the one item adjusted for dead_code; no module-wide
  allow remains).
- `pub mod favicons;` sits before `pub mod folder_icon;` in tools/mod.rs —
  the rename broke rustfmt's module sort order, fixed by hand (rustfmt is
  never run on tools/mod.rs).
- Test totals after Phase 3: 500 passed / 0 failed / 30 ignored; the 54
  favicons tests are 43 core + 11 commands tests.
- `cargo fmt -- --check` diffs are ONLY in folder_icon files (apply.rs,
  eligibility.rs, mod.rs, perf.rs, perf_bench_test.rs, shell_custom.rs,
  shortcut.rs) plus none in favicons — pre-existing owner state, reported
  only.

PHASE 5 done: padding fix (second option app_padding_percent, default 10,
for Apple/Android icons; enlarged small icons in the preview; 8 new tests),
docs (favicons-manual-checklist.md, README, PROGRESS.md, DECISIONS.md,
releases/NEXT.md). Waiting for the owner's manual test. TODO before
release: regenerate THIRD_PARTY_LICENSES.md (resvg added); clean the 2
pre-existing clippy errors in folder_icon/perf.rs.
