# Progress

## Current status

**Prompt 1 (foundation) — complete.** The app shell, tool registry, square-tile
home screen, 4 placeholder tools, Rust command skeleton, security baseline,
docs, git and GitHub are all in place.

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

1. **Prompt 2 — Image Converter**: real tool implementation; first user of
   `ToolError` (remove its `#[allow(dead_code)]` then), file dialogs and
   image encoding happen in Rust commands.
2. Later tools: QR Code Generator, Folder Icon Changer, Encrypted Notes
   (needs Persian/RTL text support; secret state only in Rust).
3. Installer work (separate prompts): NSIS x64 + ARM64, icons, versioning,
   optional code signing and CI.

## Open questions

- None currently.
