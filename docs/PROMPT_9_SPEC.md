# PROMPT 9 — NAME/LOGO NOTICE + UPDATE BUTTON
## ERX Tools for Windows · steps 9a, 9b, 9c, 9d, 9e

> **STATUS: READY TO BUILD.** All design decisions are made by the owner (section 2). Follow the steps in order. Do not skip ahead. Do not "improve" the design.

---

## 0. READ THIS FIRST — rules and resume protocol

**Rules (apply to ALL steps):**
- **R1 Never `git push`.** The owner pushes personally. Local commits only, message style as in `git log` (e.g. `Prompt 9b: update check pure logic`).
- **R2 Task list rule.** Before writing ANY final report, mark EVERY task in your task list as completed (use TaskUpdate), including the very last "report" task. The owner watches that checklist; an unticked item looks like unfinished work.
- **R3 Network rule.** After this prompt, the ONLY network access in the whole app is the update check, which runs in Rust, only when the user clicks the update button. No automatic check at startup, no background checks, no downloads, no installs. The webview never makes network requests (the CSP and Tauri capabilities must stay exactly as they are now).
- **R4 No new crates.** This prompt pre-approves ZERO new Cargo crates. The only allowed Cargo change is adding a **feature flag** to an existing `windows-sys` dependency (see 9c). If you think a new crate is needed: STOP and ask the owner.
- **R5 Honest UI.** Every state shows a plain-English message. No fake progress. Never claim "up to date" unless GitHub actually answered with a release whose version is not newer.
- **R6 Do not invent facts.** Anything marked **[VERIFY]** must be checked on the real machine (compile, run, read the file). Numbers in this prompt that come from memory (like WinHTTP error codes) are HINTS: use the named constants from the crate and say what you verified.
- **R7 English only** in UI, messages and comments.
- **R8 Gates after 9b, 9c, 9d and 9e** (all must pass; fix, do not suppress): `npm run build`; in `src-tauri`: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test` (baseline **374** passed — may only grow); ARM64 check `cargo check --target aarch64-pc-windows-msvc`. **Caution learned in Prompt 8:** the Tauri build script does not watch icon files; if you changed anything the build script reads, force it to re-run (`cargo clean -p <package>`) so the check is real.
- **R9 Code style.** Mirror the project's conventions (read how other tools/commands are structured, named, registered in `lib.rs`, and tested). No `unwrap()`/`expect()` in non-test code. Keep pure logic separate from Windows calls so it is unit-testable. Every `unsafe` block gets a one-line `// SAFETY:` comment. Do not refactor unrelated code.
- **R10 Stop and ask** if something is not covered here and changes behavior the owner will see, or if a step seems to require breaking a rule.

**Resume protocol:**
1. At the start of every session check whether `docs/PROMPT_9_PROGRESS.md` exists. If it exists: read it, run `git status` and `git log -5 --oneline`, and continue at the first step not marked DONE (if one is IN PROGRESS, inspect `git diff` and finish it; never redo DONE steps). If not: fresh start with 9a.
2. First action of 9a: save the full text of THIS prompt, verbatim, to `docs/PROMPT_9_SPEC.md`, and create `docs/PROMPT_9_PROGRESS.md` (a table: 9a, 9b, 9c, 9d, 9e with status NOT STARTED / IN PROGRESS / DONE / BLOCKED, plus sections "Verified facts", "Open questions").
3. Update the progress file after every micro-step.
4. **Stops:** STOP and wait for the owner after **9a**, after **9c**, and after **9e**. Between 9a→9b and 9c→9d... see each step. Never start a step after a STOP until the owner says "continue".

---

## 1. Context (facts from earlier work)

- Project: Tauri 2 + Rust backend (`windows-sys 0.61.x`, `windows 0.62`, `serde_json` already a dependency) + React 19/TypeScript frontend. Repo `C:\dev\erxtools`, GitHub repo `EhsanKarimiRX/erx-tools-windows` — **now PUBLIC**, MIT license, README.md in the root, `branding/` folder with the owner's logo files, app version currently **0.1.0**, **no GitHub release has been published yet**.
- The home screen shows the text "Version 0.1.0" at the **bottom-left**. The backend for the home screen lives in `src-tauri/src/tools/app_info.rs` (find how the version is produced; **[VERIFY]**).
- Quick Launch (System Manager) already opens things with `ShellExecute` from Rust — reuse its helper if there is one.
- Baseline: 374 Rust tests, all gates green, last commit is `e01b250` (Prompt 8) or later — check `git log`.

---

## 2. Decisions (made by the owner — do not reopen)

**Branding:** the source code stays MIT, but the **name "ERX Tools" and the logo are NOT free to reuse**. A notice in the README and in `branding/NOTICE.md` says forks must use their own name and logo.

**Update button:**
1. Location: the existing version text at the bottom-left of the home screen becomes a small **rounded rectangle ("pill")** containing the version text and, inside the pill, a **circular button** with an **up-arrow** icon.
2. **The check happens only when the user clicks the circular button.** Never automatically.
3. Circle colors (and a text/tooltip for each, so color is never the only signal):
   - **Gray** — not checked yet (this is the state at every app start)
   - **Blue** — a newer version exists
   - **Green** — the installed version is the newest
   - **Red** — the check failed (for example no internet)
   While checking: neutral gray circle with a small spinner.
4. Nothing is downloaded or installed automatically. When an update exists, the user can click a button that opens the release page in his browser.
5. The state is kept in memory only (resets to gray at every start). Nothing is stored on disk.
6. Honest wording about the network (README, UI): one HTTPS request to GitHub; GitHub can see the user's IP address and the app version, like any web server; nothing about the user or their files is sent.

**Technical design:**
- The request is made from **Rust**, not from the webview. HTTP client: **WinHTTP** (built into Windows) through the flat C API in `windows-sys` (feature `Win32_Networking_WinHttp`). Reasons: no new crates, uses Windows' own TLS and certificate store and system proxy settings, nothing extra to ship, safe on ARM64. (If the needed functions are not in `windows-sys`, use the same functions from the `windows` crate that is already a dependency — also no new crate. If neither works: STOP and ask.)
- Endpoint: `GET https://api.github.com/repos/EhsanKarimiRX/erx-tools-windows/releases/latest`. Only that one host and path family. The result is only read, never executed.

---

# 9a — Branding notice (documents only)

**9a-1.** Do the resume-protocol setup (save the spec, create the progress file).

**9a-2. Create `branding/NOTICE.md`** with exactly this text:

```markdown
# Branding notice

The source code of ERX Tools for Windows is released under the MIT License (see `LICENSE` in the repository root).

The following are **not** covered by the MIT License. All rights in them are reserved by the project owner (EhsanKarimiRX):

- the name "ERX Tools" and "ERX Tools for Windows"
- the ERX Tools logo and app icons: the files in this `branding/` folder and in `src-tauri/icons/`

**You may:** fork the code and change it; mention the name to say that your project is based on or compatible with ERX Tools (for example "a fork of ERX Tools"); show the logo when linking to or reviewing the original project.

**You may not, without written permission:** use the name or the logo for your own product, fork, installer, website or store listing, or in any way that suggests your version is the official ERX Tools.

**If you fork this project:** give it its own name and its own logo — replace the files in `branding/` and `src-tauri/icons/`, and change `productName`, `identifier` and the window title in `src-tauri/tauri.conf.json`.

Questions: please open an issue on the project's GitHub page.
```

**9a-3. Edit `README.md`** (open it first and find the exact current text; **do not change anything else**):
1. Insert this section immediately BEFORE the existing `## License` heading:
```markdown
## Name and logo

The source code is released under the MIT License (see below). The **ERX Tools** name and the ERX Tools **logo** (the files in the `branding/` folder and the app icons) are **not** covered by that license. You are welcome to fork the code and change it, but please give your version its own name and its own logo, and do not present it as the official ERX Tools. Details: [branding/NOTICE.md](branding/NOTICE.md).
```
2. Replace the single line under `## License` (currently `Released under the [MIT License](LICENSE).`) with:
```markdown
The source code is released under the [MIT License](LICENSE). The name and logo are not part of that license (see [Name and logo](#name-and-logo)).
```
3. Check that both links resolve (`branding/NOTICE.md` exists; the anchor `#name-and-logo` matches the heading).

**9a-4.** Run `git diff --stat` and show it in the report. No code gates are needed (documents only), but run `npm run build` once to be safe. Make ONE local commit: `Prompt 9a: name and logo notice (README + branding/NOTICE.md)`. Mark all tasks completed. Report: the exact two README changes (diff), and the commit hash. **STOP** and wait for the owner (he reads the wording).

---

# 9b — Pure logic (no network, no UI)

Create the Rust module `src-tauri/src/update_check/` (follow how existing top-level modules are declared in `lib.rs`; if the project convention puts everything under `tools/`, follow that instead and say so). Files: `mod.rs`, `version.rs`, `release.rs`, `result.rs`. Tests live in each file under `#[cfg(test)]`, like the rest of the project.

**9b-1. `version.rs` — SemVer parsing and comparison (no new crate; write it yourself).**
```rust
pub struct Version { pub major: u64, pub minor: u64, pub patch: u64, pub pre: Vec<PreId> /* empty = a normal release */ }
pub enum PreId { Num(u64), Text(String) }
pub fn parse_version(text: &str) -> Option<Version>
impl Ord for Version  // SemVer precedence
```
Rules:
- Trim whitespace; accept one leading `v` or `V`; accept `MAJOR.MINOR.PATCH` (exactly three numeric parts, `u64`; leading zeros like `01` are accepted); optional `-` followed by dot-separated pre-release identifiers (non-empty; characters `[0-9A-Za-z-]`); optional `+build` suffix which is IGNORED. Anything else → `None` (empty string, `v`, `1`, `1.2`, `1.2.3.4`, `a.b.c`, negative numbers, empty identifiers like `1.2.3-`, `1.2.3-a..b`).
- Precedence: compare major, minor, patch numerically; a version WITH a pre-release is LOWER than the same version without one; pre-release identifiers compare left to right: numeric identifiers compare numerically and are LOWER than text identifiers; text identifiers compare in ASCII order; if all shared identifiers are equal, the longer list is higher. `+build` never matters.
Tests (minimum): `0.2.0 > 0.1.0`; `0.10.0 > 0.9.0` (numeric, not text); `1.0.0 > 1.0.0-beta.1`; `1.0.0-beta.2 > 1.0.0-beta.1`; `1.0.0-beta.11 > 1.0.0-beta.2`; `1.0.0-alpha < 1.0.0-beta`; `1.0.0-1 < 1.0.0-alpha`; `1.0.0-alpha < 1.0.0-alpha.1`; `v0.2.0 == 0.2.0`; `0.2.0+build5 == 0.2.0`; equal versions compare Equal; all invalid inputs above return `None`; `" v1.2.3 "` parses.

**9b-2. `release.rs` — read GitHub's answer.**
```rust
pub struct ReleaseInfo { pub tag: String, pub url: String }
pub enum ReleaseParse { Usable(ReleaseInfo), NotUsable /* draft or prerelease */, Invalid }
pub fn parse_release_json(body: &[u8]) -> ReleaseParse
pub fn is_allowed_release_url(url: &str) -> bool
pub const RELEASES_PAGE_URL: &str = "https://github.com/EhsanKarimiRX/erx-tools-windows/releases";
```
- Parse with `serde_json` into a small struct with ONLY these fields: `tag_name` (string), `html_url` (string), `draft` (bool, default false), `prerelease` (bool, default false). Ignore every other field. Wrong types / missing `tag_name` / not JSON / empty → `Invalid`. `draft == true` or `prerelease == true` → `NotUsable`.
- `is_allowed_release_url`: true only if the string starts with EXACTLY `https://github.com/EhsanKarimiRX/erx-tools-windows/`, has length ≤ 300, and contains no whitespace or control characters. Everything else false. Tests (minimum): the real releases URL and a tag URL → true; `http://` version → false; `https://github.com.evil.com/EhsanKarimiRX/erx-tools-windows/` → false; `https://github.com/EhsanKarimiRX/erx-tools-windows-evil/` → false; `https://github.com/OtherUser/erx-tools-windows/` → false; URL with a space, newline or NUL → false; 301-character URL → false; empty → false; different letter case in the owner name → false.
- If a usable release's `html_url` is NOT allowed by the check, the result still counts as an update but the page to open falls back to `RELEASES_PAGE_URL`.
Tests for `parse_release_json` (minimum): valid typical body; missing `tag_name`; `tag_name` is a number; `draft: true`; `prerelease: true`; empty body; garbage bytes; extra unknown fields ignored; huge nested JSON does not panic.

**9b-3. `result.rs` — the decision, the result type and the messages.**
```rust
#[derive(Serialize)] #[serde(tag = "kind", rename_all = "camelCase")]
pub enum UpdateCheckResult {
  UpToDate { current: String, latest: String },
  UpdateAvailable { current: String, latest: String },
  NoRelease { current: String },
  Failed { message: String },
}
pub enum HttpOutcome { Response { status: u32, body: Vec<u8> }, Error(HttpErrorKind) }
pub enum HttpErrorKind { CannotReach, TimedOut, Tls, TooLarge, Other }
pub fn decide(current: &str, outcome: &HttpOutcome) -> (UpdateCheckResult, Option<String> /* page URL to remember */)
```
Rules for `decide`:
- `Error(CannotReach | TimedOut)` → `Failed("Couldn't reach GitHub. Check your internet connection and try again.")`.
- `Error(Tls)` → `Failed("Couldn't make a secure connection to GitHub. Check your PC's date and time and any security software, then try again.")`.
- `Error(TooLarge | Other)` → `Failed("GitHub's answer could not be understood.")`.
- `Response` status `404` → `NoRelease`. Status `403` or `429` → `Failed("GitHub is limiting requests right now. Please try again in a few minutes.")`. Any status other than 200 → `Failed("GitHub returned an unexpected answer (HTTP <status>).")`.
- Status 200: `parse_release_json`. `Invalid` → `Failed("GitHub's answer could not be understood.")`. `NotUsable` → `NoRelease`. `Usable(info)`: parse `info.tag` with `parse_version`; `None` → `Failed("The newest release has a version name ERX Tools cannot read (<tag>).")`. Parse `current` too; if `current` is unparseable → `Failed("This app's own version number could not be read.")`. If latest > current → `UpdateAvailable` (and return the page URL: `info.url` if `is_allowed_release_url`, else `RELEASES_PAGE_URL`); otherwise → `UpToDate`. In results show versions WITHOUT a leading `v` (normalize: `major.minor.patch` plus `-pre` if any).
Tests (minimum): every branch above, including 403, 429, 500, 301, 404, 200+garbage, 200+draft, tag `v0.2.0` vs current `0.1.0` → available; tag `0.1.0` vs `0.1.0` → up to date; tag `0.0.9` vs `0.1.0` → up to date (a lower tag is NOT an update); tag `v1.0.0-beta.1` vs current `1.0.0` → up to date; tag `banana` → Failed; the remembered URL falls back to the releases page when `html_url` is not allowed; serialization of each result to JSON has the shape `{"kind":"updateAvailable","current":"0.1.0","latest":"0.2.0"}` (camelCase tag values).

**9b-4. Version consistency test (guard against forgetting to bump a version).** Add one Rust test that reads, at compile time with `include_str!`, `src-tauri/tauri.conf.json` and the root `package.json` (paths relative to the test file; **[VERIFY]** they resolve), parses them with `serde_json`, and asserts that `tauri.conf.json`'s `version`, `package.json`'s `version` and `env!("CARGO_PKG_VERSION")` are all equal. Expected today: all `0.1.0`. This test must fail loudly if a future release bumps only one of them.

**9b-5. Where does the home screen get "Version 0.1.0"?** Find out and write the answer in the progress file ("Verified facts"). The update check MUST compare against the same value the home screen shows, produced by ONE function (for example `current_version()` returning `app.package_info().version.to_string()`). If the home screen currently uses a hardcoded string, fix it to use the real version from the backend (that is the only allowed fix outside the new module).

**9b-6.** Run the gates (R8). Local commit `Prompt 9b: update check pure logic`. Update the progress file, mark all tasks completed. **Do not stop here; continue to 9c.**

---

# 9c — Network client (WinHTTP), commands, debug test switch

**9c-1. Add the feature.** In `src-tauri/Cargo.toml` add the feature `Win32_Networking_WinHttp` to the existing `windows-sys` dependency (nothing else). **[VERIFY]** the WinHTTP functions exist in the vendored `windows-sys` source (`~/.cargo/registry/src/*/windows-sys-0.61*/src/Windows/Win32/Networking/WinHttp/mod.rs`). If not, use the same API from the `windows` crate (add the same feature there). Record the finding.

**9c-2. `http.rs` in the same module — trait + real client.**
```rust
pub trait HttpGet: Send + Sync { fn get(&self, host: &str, path: &str) -> HttpOutcome; }
pub struct WinHttpClient { pub user_agent: String }
```
`WinHttpClient::get` implementation, in THIS order (flat C API; all handles are `HINTERNET`):
1. Convert user agent, host, path and header text to NUL-terminated UTF-16.
2. `WinHttpOpen(user_agent, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, no proxy name, no proxy bypass, 0)` → session. Null → read `GetLastError`, classify (see below).
3. `WinHttpSetTimeouts(session, resolve 5000, connect 5000, send 10000, receive 10000)` (milliseconds).
4. `WinHttpConnect(session, host, 443, 0)` → connection.
5. `WinHttpOpenRequest(connection, "GET", path, null version, null referrer, default accept types, WINHTTP_FLAG_SECURE)` → request. **Never** set any option that ignores certificate errors. Do not change the redirect policy.
6. `WinHttpSendRequest(request, headers, header length, null body, 0, 0, 0)` with the headers text exactly: `Accept: application/vnd.github+json\r\nX-GitHub-Api-Version: 2022-11-28\r\n`.
7. `WinHttpReceiveResponse(request, null)`.
8. Status code: `WinHttpQueryHeaders(request, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, no name, &mut u32 buffer, &mut size, no index)`.
9. Body: loop `WinHttpQueryDataAvailable` / `WinHttpReadData` in chunks of at most 8192 bytes until 0 bytes are available. If the total would exceed **256 KiB** → stop and return `Error(TooLarge)`.
10. Close all three handles in reverse order using a small RAII guard struct (`impl Drop` calling `WinHttpCloseHandle`) so every early return closes them.
- **Wall-clock guard:** run the whole `get` on a worker thread and wait for it with `std::sync::mpsc::Receiver::recv_timeout(15 seconds)`; on timeout return `Error(TimedOut)` (the abandoned thread ends by itself because WinHTTP has its own timeouts).
- **Error classification** (pure function, unit-tested): map the `GetLastError` code to `HttpErrorKind` using the NAMED constants from the crate — `ERROR_WINHTTP_TIMEOUT` → `TimedOut`; `ERROR_WINHTTP_NAME_NOT_RESOLVED`, `ERROR_WINHTTP_CANNOT_CONNECT`, `ERROR_WINHTTP_CONNECTION_ERROR` → `CannotReach`; `ERROR_WINHTTP_SECURE_FAILURE` (and other `ERROR_WINHTTP_SECURE_*` / certificate codes you find) → `Tls`; everything else → `Other`. (Hint numbers from memory, **[VERIFY]** against the crate: 12002, 12007, 12029, 12030, 12175.) Tests: one representative code per class, plus an unknown code → `Other`.
- User agent string: `ERX-Tools/<current version> (+https://github.com/EhsanKarimiRX/erx-tools-windows)`.

**9c-3. Tauri commands** (register them exactly like other commands in `lib.rs`; no capability/permission changes):
```rust
#[tauri::command] async fn check_for_updates(app: AppHandle, state: State<UpdateState>) -> UpdateCheckResult
#[tauri::command] fn open_update_page(state: State<UpdateState>) -> Result<(), String>
```
- `UpdateState` is Tauri managed state holding `Mutex<Option<String>>` = the page URL remembered by the last `UpdateAvailable` result.
- `check_for_updates`: run the blocking client call with `tauri::async_runtime::spawn_blocking`; get the current version from the SAME function as the home screen (9b-5); call `decide`; store/clear the remembered URL (cleared whenever the result is not `UpdateAvailable`); return the result. Never panic; a panicked/failed worker becomes `Failed("GitHub's answer could not be understood.")`.
- `open_update_page`: takes NO arguments from the frontend. Opens the remembered URL if there is one, otherwise `RELEASES_PAGE_URL`, through `ShellExecuteW` with verb `"open"` (reuse the Quick Launch helper if it exists). Re-check `is_allowed_release_url` on whatever is opened. On failure return a plain-English error: "Couldn't open your browser. You can visit github.com/EhsanKarimiRX/erx-tools-windows/releases yourself."

**9c-4. Debug-only test switch (so the owner can see all states before any release exists).** Inside `check_for_updates`, wrapped in `#[cfg(debug_assertions)]` so it does NOT exist in release builds: if the environment variable `ERX_FAKE_UPDATE` is set, skip the network and return a fake result after a 700 ms delay: value `available` → `UpdateAvailable { current, latest: "9.9.9" }` (and remember `RELEASES_PAGE_URL`); `uptodate` → `UpToDate { current, latest: current }`; `norelease` → `NoRelease`; `failed` → `Failed("Couldn't reach GitHub. Check your internet connection and try again.")`. Any other value → ignored (real check). Add a comment explaining that it is for development only.

**9c-5. Tests.** Non-ignored: classification function; command-level logic using a **fake** `HttpGet` (a struct implementing the trait that returns canned outcomes): 200 with a newer tag → `UpdateAvailable`; 404 → `NoRelease`; error kinds → `Failed`; the remembered-URL logic (set on available, cleared on every other result). `#[ignore]` live tests (run with `cargo test update -- --ignored --nocapture`): (1) the real `WinHttpClient` against `api.github.com` and the real path above — today we expect HTTP **404** (no release published yet) → print the result and record it in "Verified facts"; if you get 200 instead, print the tag; (2) the real client against a host that cannot connect (for example host `127.0.0.1`, which has no HTTPS server on port 443) → expect `CannotReach` or `TimedOut`; record the real `GetLastError` code you observe. Do NOT make live tests part of the normal test run.

**9c-6.** Gates (R8). Local commit `Prompt 9c: WinHTTP update client and commands`. Update the progress file; mark all tasks completed. **STOP and report** (format in Appendix B). Say plainly: "Live test result: <what GitHub answered>". Wait for the owner.

---

# 9d — Home screen button (frontend)

**9d-1. Find the home screen** component that shows `Version …` (bottom-left) and its CSS. Read how it gets the version. Plan the change with the smallest edits.

**9d-2. New component `UpdateBadge`** (own file next to the home screen; name and folder following project conventions) that REPLACES the plain version text with:
- A **pill** (rounded rectangle, same look/colors family as the rest of the app; use existing CSS variables/theme) containing the text `Version 0.1.0` (the real version) and, at the right inside the pill, a **circular button** ~22 px with a white icon on a colored filled circle.
- States and circle colors (define CSS variables for these colors; make sure the white icon has enough contrast on each; check how it looks in both light and dark themes if the app has both):
  - `idle` — **gray** (`#6b7280`), icon: up arrow. Tooltip + `aria-label`: "Check for updates".
  - `checking` — gray, small spinning ring instead of the arrow (CSS animation; with `prefers-reduced-motion: reduce` show a static "…" instead of spinning). `aria-label`: "Checking for updates…". Clicks are ignored while checking.
  - `available` — **blue** (`#2563eb`), icon: up arrow. `aria-label`: "Update available".
  - `uptodate` — **green** (`#16a34a`), icon: check mark. `aria-label`: "You have the latest version".
  - `failed` — **red** (`#dc2626`), icon: exclamation mark. `aria-label`: "Update check failed".
  - `norelease` — **gray** like idle but with the message below (icon: check mark is NOT used; keep the up arrow).
  Icons from `lucide-react` (**[VERIFY]** the exact names exist in the installed version, for example `ArrowUp`, `Check`, `TriangleAlert` or `AlertTriangle`); the circle itself is a styled `button` element.
- Behavior: clicking the circle in `idle`, `uptodate`, `norelease` or `failed` calls `invoke("check_for_updates")`, shows `checking`, then the result state and a small **message popover** anchored above the pill (the pill is at the bottom-left, so open upward; keep it fully inside the window). Clicking the circle in `available` toggles the popover. The popover closes on outside click or Escape and returns focus to the circle button. It has `role="status"` and `aria-live="polite"` so the result is announced.
- Popover texts (English, exact):
  - `uptodate`: "You have the latest version (<current>)."
  - `available`: "Version <latest> is available. You have <current>." + a button **"Open download page"** (calls `invoke("open_update_page")`; if it returns an error show that text in the popover) + gray small text: "ERX Tools never downloads or installs updates by itself."
  - `norelease`: "No published release was found on GitHub yet."
  - `failed`: the `message` returned by Rust, unchanged.
- The state lives in React state only. No `localStorage`, no persistence. No automatic call at startup.
- TypeScript types mirror the Rust enum exactly (`kind: "upToDate" | "updateAvailable" | "noRelease" | "failed"`).
- The pill must not shift the layout of the home screen tiles and must look fine at the app's smallest window size.

**9d-3.** `npm run build` must pass (TypeScript strict). Re-run the Rust gates only if you touched Rust. Local commit `Prompt 9d: update badge on the home screen`. Update the progress file, mark all tasks completed. **Do not stop; continue to 9e.**

---

# 9e — Documents, audit, final verification

**9e-1. README — make the network statements true.** Open `README.md`, find the exact current text and replace ONLY these three places:
1. In "Why ERX Tools", the bullet starting `- **Offline by design.**` becomes:
`- **Offline by design.** No telemetry and no remote content. The only network request is the update check, and only when you click it (see Privacy and security).`
2. In "Privacy and security", the first bullet (currently: "The app itself makes no network calls, sends no telemetry and loads no remote content. The webview has a strict Content-Security-Policy and only Tauri's core default permissions.") becomes:
`- The only network request ERX Tools makes is the **update check**, and only when you click the update button next to the version number on the home screen. It is one HTTPS request to GitHub asking for the latest release number. Nothing is downloaded or installed automatically, and nothing about you or your files is sent. Like any web server, GitHub can see your IP address and the app version. Everything else works offline; the app sends no telemetry and loads no remote content. The webview has a strict Content-Security-Policy and only Tauri's core default permissions.`
3. In "Security rules for every change", add a new last bullet: `- The update check (written in Rust, in the update module) is the only allowed network access. Do not add other network calls without the owner's explicit approval.`
If a quoted sentence is not found exactly, find the equivalent and report the difference. Do not edit anything else in the README.

**9e-2. DECISIONS.md.** Add an entry (follow the file's existing style) "Update check (Prompt 9)": manual only; first and only network exception; done in Rust through WinHTTP (no new crates, system TLS/proxy); one GET to the GitHub releases API; no download/install; state in memory only; CSP/capabilities unchanged; draft/prerelease releases are ignored by design, so beta builds for friends must be published as normal (non-pre-release) releases with a version tag like `v0.2.0`; the release tag must match the app version (tests guard the three version files); debug-only `ERX_FAKE_UPDATE` switch for development.

**9e-3. Audit (put results in the report):** (a) `git diff` of `Cargo.toml`/`Cargo.lock` since the start of this prompt — expected: one feature flag, no new crates in `Cargo.lock`; (b) `tauri.conf.json` and `src-tauri/capabilities/` unchanged (empty `git diff` against the commit before 9a); (c) grep the whole `src/` frontend for `fetch(`, `XMLHttpRequest`, `WebSocket`, `http://`, `https://` — expect no network code (URL strings only in comments/README text are fine; list any hit); (d) grep `src-tauri/src` for `WinHttp` — it must appear ONLY in the update module; (e) confirm `ERX_FAKE_UPDATE` is inside `#[cfg(debug_assertions)]`.

**9e-4. Final gates** (R8) incl. ARM64 (force the build script re-run as noted in R8). Local commit `Prompt 9e: docs and final verification`. Update the progress file (all DONE). Mark all tasks completed. **STOP and report.**

**Owner test checklist (put in your report, adjusted to reality; include the exact commands):**
1. Run `npm run tauri dev`. At the bottom-left of the home screen the version text is now inside a rounded rectangle with a **gray** circular arrow button.
2. Click it (internet on): since no release exists yet, a popover says "No published release was found on GitHub yet." and the circle stays gray.
3. Turn the internet off (or disconnect Wi-Fi) and click: the circle turns **red** with the message about not reaching GitHub. Turn the internet on again.
4. See the other colors without any release, using the debug switch. In PowerShell, in the project folder:
   - `$env:ERX_FAKE_UPDATE="available"; npm run tauri dev` → click the circle → **blue**, message "Version 9.9.9 is available…" and an "Open download page" button (it opens the project's Releases page).
   - `$env:ERX_FAKE_UPDATE="uptodate"; npm run tauri dev` → **green**.
   - `$env:ERX_FAKE_UPDATE="failed"; npm run tauri dev` → **red**.
   - Afterwards clear it: `Remove-Item Env:ERX_FAKE_UPDATE`.
5. Check the README "Name and logo", "Why ERX Tools" and "Privacy and security" wording.

---

## APPENDIX A — progress file template (`docs/PROMPT_9_PROGRESS.md`)

```markdown
# Prompt 9 — Name/logo notice + update button: progress
Last updated: <date/time>
## How to resume
Read docs/PROMPT_9_SPEC.md (full prompt). Continue at the first step not DONE. Never git push.
## Status
| Step | Description | Status | Notes |
|---|---|---|---|
| 9a | Branding notice (docs) | NOT STARTED | |
| 9b | Pure logic (semver, JSON, decide, version test) | NOT STARTED | |
| 9c | WinHTTP client, commands, debug switch | NOT STARTED | |
| 9d | Home screen UpdateBadge | NOT STARTED | |
| 9e | Docs, audit, final gates | NOT STARTED | |
## Gates (last run)
## Verified facts
## Open questions
```

## APPENDIX B — report format (after 9a, 9c, 9e)

1. What was done (3–6 lines). 2. Gates (build / fmt / clippy / test count / ARM64). 3. Files created/changed. 4. New dependencies/features/permissions (or "none"). 5. Verified facts and what could NOT be verified (including the real GitHub live-test answer in 9c). 6. Owner test checklist (9e) or "none". 7. Known limits. 8. Questions for the owner. 9. Progress file updated: yes; commit hashes; **Pushed: NO**. 10. All tasks in the task list are marked completed. 11. "Waiting for the owner."