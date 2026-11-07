# PROMPT 7 — PRE-PUBLIC CLEANUP — PROGRESS

Started: 2026-11-07. Rules kept: no `git push`, no history rewrite, no app code changes (`src/`, `src-tauri/src/`).

## Status

| Step | Description | Status |
|------|-------------|--------|
| 7a | Read-only audit (A1–A8) | DONE |
| 7a report | Report table + recommended actions, STOP for owner | DONE — owner answered all decision questions |
| 7b-1 | README: copy README.new.md verbatim into README.md, delete README.new.md | DONE |
| 7b-2 | LICENSE: MIT, `Copyright (c) 2026 EhsanKarimiRX` | DONE |
| 7b-3 | Metadata (package.json, Cargo.toml, tauri.conf.json) | DONE |
| 7b-4 | Docs paths replacement (owner approved option A) | DONE |
| 7b-5 | .gitignore additions (per 7a-A5) | DONE |
| 7b-6 | Gates: npm run build; cargo fmt/clippy/test; ARM64 check | DONE — all green |
| 7b-7 | Local commit(s), no push | DONE |
| 7b report | Report + owner reminders, STOP | DONE |

**PROMPT 7 COMPLETE.** Waiting for the owner to push and make the repo public.

## Owner decisions (received 2026-11-07)

1. Emails: nothing to do (noreply address already in use). 2. No secrets found. 3. Keep history as is — NEVER rewrite (`.mem4.py` blob stays). 4. Docs path scrub option A (working tree only). 5. Keep the internal PROMPT_*/PROGRESS docs. 6. Add `.env` / `.env.*` to .gitignore. 7. 7b defaults: README verbatim, MIT LICENSE, metadata fill. Later addition: one allowed README wording change in Credits (see 7b-1).

## 7b changes (all verified after the owner's interruption, before commit)

- **README.md** — replaced completely with the owner's `README.new.md`, copied verbatim via `cp` (119 lines; source file then deleted and confirmed untracked). One owner-authorized wording change: Credits table text "in the z.ai desktop app" → "in Z Code (the z.ai desktop app)" (exactly 1 occurrence before, 0 after, 1 new). No other wording touched.
- **LICENSE** — new file, standard MIT text, `Copyright (c) 2026 EhsanKarimiRX`.
- **package.json** — added `description`, `author`, `license`, `repository` (git URL), `homepage`. Name/version/private untouched.
- **src-tauri/Cargo.toml** — `description` updated to "A small, offline toolbox for Windows." (was the inconsistent older wording); added `license = "MIT"`, `repository`. `authors`/version/edition untouched. No lockfile change (metadata only).
- **src-tauri/tauri.conf.json** — `bundle` gained `publisher`, `copyright`, `shortDescription`, `category: "Utility"`. All four VERIFIED against the official Tauri 2 config schema (https://schema.tauri.app/config/2 — fetched and inspected: all exist in BundleConfig; "Utility" is in the documented category value list). `productName`, `version`, `identifier` untouched.
- **docs/PROMPT_5_PROGRESS.md, docs/PROMPT_6_PROGRESS.md** — every `C:\Users\EhsanRX` replaced with `C:\Users\<user>` (2 files, 3 occurrences). Post-scrub grep of the whole tracked tree: **0 hits for "EhsanRX"**.
- **.gitignore** — added `.env` and `.env.*` (nothing such exists today; hygiene only).
- **docs/PROMPT_7_PROGRESS.md** — this file (committed per the owner's decision to keep internal docs).

## Gates (2026-11-07, run from the start after the interruption)

- npm run build: **PASS** (421 ms)
- cargo fmt --check: **PASS**
- cargo clippy -- -D warnings: **PASS** (zero warnings)
- cargo test: **374 passed, 0 failed, 18 ignored** (live tests; ≥ 374 required ✓)
- ARM64 check: **PASS** (cargo check --target aarch64-pc-windows-msvc)

## 7a findings (kept for the record)

- A1 history: 44 commits, single author+committer `EhsanKarimiRX <EhsanKarimiRX@users.noreply.github.com>` (GitHub noreply form — no personal email exposed).
- A2/A3 secrets: none in working tree or full history (hard tokens 0 hits; emails only false positives). `.mem4.py` scratch blob stays in history per owner decision (contains a local username path, no secret).
- A5: no tracked file > 1 MB; images = app icons only; no sensitive file types tracked.
- A7: docs clean of credentials; PROMPT_5/6_SPEC+PROGRESS are internal agent docs (owner chose to keep them public).
- A8: all README.new.md link targets exist (LICENSE was created by 7b).

## Owner follow-ups after this commit

1. Push personally: `git push` (local main is ahead of origin; this commit rides on top of the 13 already-local commits).
2. GitHub About text/topics: already set (per owner).
3. Settings → General → Danger Zone → Change visibility → Make public.
4. After it is public: open the repo in a private/incognito window; check README and LICENSE render.
