# System Manager

One tool, one scrolling page with five sections, in this fixed order:
**Awake → Desktop Icons → Organize → Quick Launch → Command Tips**.
Fully offline, no admin rights needed, English UI. Every section is honest:
no silent failures, no fake progress, plain-English reasons for anything
that did not happen.

## Awake

Keeps the PC awake during long, expensive computer runs.

- **How it works:** while Awake is ON, the active power plan's Sleep,
  Hibernate and Turn-off-display timeouts are set to **Never** (both
  plugged in and on battery). Turning it off restores the exact original
  values. The original values are written to a state file in the app data
  folder **before** anything is changed.
- **Why this design (owner decision):** with the usual "execution state"
  API, a crash of ERX Tools would send Windows straight back to normal
  rules and the PC could sleep, losing the run. With the power-plan
  approach, a crash simply leaves the PC awake — the cheap failure mode.
- **Safety rules:** the app re-checks the power plan every 5 seconds (and
  when the window regains focus). If you change a timeout by hand in
  Windows, Awake switches itself off, keeps YOUR change, and tells you
  (banner + Windows notification). Restore never overwrites a manual
  change. If the app is closed or crashes while Awake is on, the next
  launch detects the state file and resumes or repairs it.
- **Options (collapsible drawer):** "Let the screen turn off normally"
  (default: the screen stays on too), and an optional auto-off timer
  (1/2/4 hours) — see limits below.
- **Closing the app while Awake is on** shows three choices: restore and
  close / keep Awake on and close / cancel.
- **Honest limits:** Awake only blocks *automatic* sleep and screen-off.
  Clicking Sleep or shutting down still works; closing a laptop lid
  follows the Windows lid setting. The timer only runs while ERX Tools is
  open (the state file keeps it, and the next launch reports a timer that
  finished while the app was closed). No helper processes are used.

## Desktop Icons

Show/hide switches for the five classic desktop icons (This PC, Recycle
Bin, Control Panel, Network, User's Files) via the one HKCU registry key
Windows itself uses — the same settings as Windows Settings →
Personalization → Themes → Desktop icon settings.

- A switch shows the state **read back from the registry** after a change;
  a failed change reverts the switch with a red message.
- A missing registry value means the *Windows default* per icon (only the
  Recycle Bin shows by default) — the common bug this handles correctly.
- **Refresh behavior:** the section sends one association-changed
  broadcast plus a best-effort refresh command to the desktop view window.
  On the owner's Windows 11 build a repaint may still need a manual **F5**
  on the desktop; the UI says so. Changes apply to your Windows account
  only.

## Organize

Tidy loose files into category folders — one engine, three targets:
**Desktop** (Apps / Documents / More), **Downloads** (Documents / Images /
Videos / Music / Archives / Installers / More), and **Custom folder…**
(content types only; executables, libraries, scripts and unknown types are
left in place on purpose).

- **Safety:** top level only; real folders are never touched; hidden or
  system files (desktop.ini), Office `~$` lock files, links/reparse points,
  partial downloads (`.crdownload`, `.part`, …) and cloud-only OneDrive
  files are skipped with a reason. **Nothing is ever deleted or
  overwritten** — name conflicts get "report (2).pdf" style names; moves
  use a Windows API that cannot replace an existing file.
- **Preview first:** a read-only scan explains in plain words what will
  happen before anything moves; the run re-checks every file (size +
  modified time) and skips anything changed since the preview.
- **Undo:** every move is journaled; the most recent run per target can be
  undone (reverse order; only EMPTY folders the tool created are removed;
  files the user moved or deleted are reported "not found").
- **Custom folder protection:** drive roots, the Windows folder, Program
  Files, ProgramData, the user profile root itself and anything under
  AppData are refused with a plain message.
- **OneDrive honesty:** if the folder lives in OneDrive (including
  "OneDrive - Company" layouts), the preview says so; online-only files
  are skipped; OneDrive settings are never changed.
- Icons from the shared **Public Desktop** folder are not part of your
  Desktop folder and are left alone.

## Quick Launch

One-click buttons: Control Panel, Defragment and Optimize Drives (shows
its own UAC), Command Prompt ± "Run as administrator", PowerShell ±
"Run as administrator", Device Manager, Services, and Microsoft PC
Manager.

- The frontend sends only a fixed button id; the launch targets are
  hard-coded with full System32 paths, so a tampered PATH cannot hijack
  them.
- PowerShell 7 is preferred when installed; otherwise Windows PowerShell —
  a caption under the button always says which one will start.
- A dismissed UAC prompt shows a neutral "Cancelled.", not an error.
- **Microsoft PC Manager:** if installed, the button opens it (verified
  live end-to-end on the development machine via the package registry and
  the app's own manifest). If not installed, the button says so honestly
  and offers the Microsoft Store search page (no hard-coded product IDs).

## Command Tips

A static, copy-only list of 11 verified commands (grouped Apps / Files /
Repair / Network / System info) with badges for shell, administrator,
duration and internet use. ERX Tools never runs them; there is no clipboard
plugin — copying uses the browser clipboard with a plain fallback message.

Two badge decisions came from live verification on the development machine
(Windows 11 Pro, build 26300) rather than the usual assumptions:
`ipconfig /flushdns` **needs an administrator window** on this build, and
`%USERPROFILE%` only expands in **Command Prompt** (not PowerShell).

## Status and testing

- Tested on Windows 11 Pro (build 26300, x64); ARM64 build is checked but
  Windows 10 is untested.
- Awake's power changes, the Desktop Icons round trip and the PC Manager
  launch were verified live on the development machine; Organize's safety
  rules are covered by scratch-folder tests, and the real folders are only
  ever organized by the user, by hand.
- Wording review: every user-visible string is listed in
  docs/SYSTEM_MANAGER_UI_TEXT.md for the owner to edit in one place.
