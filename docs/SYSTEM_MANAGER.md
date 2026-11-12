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

Tidy loose files into category folders. Two tabs: **Desktop** and
**Folder**.

- **Desktop tab** — organizes only the loose files of your Desktop folder
  into exactly three folders: **Apps** (programs and shortcuts),
  **Documents** (Word, PDF, Excel and similar) and **More** (everything
  else). Windows' virtual icons (This PC, Recycle Bin, …) are not files
  and are never touched.
- **Folder tab** — any folder you choose (or the **Use Downloads folder**
  quick button), with a **Sort by** selector:
  - **Format** — Downloads keeps its 7 categories (Documents, Images,
    Videos, Music, Archives, Installers, More); any other folder uses the
    5 content categories and leaves executables/runtime files and unknown
    types in place.
  - **Date** — folders named by **year** ("2023") or **year and month**
    ("2023-05"), flat. The date can come from the photo's own camera data
    (EXIF `DateTimeOriginal`, falling back to `DateTimeDigitized` — the
    last-edit time is never used), from the Windows created date, or from
    the modified date. Files whose date cannot be determined (including
    every non-photo in "Photo date taken" mode) go into a folder named
    exactly **`Date not found`**.
  - **Size** — fixed buckets with binary units: Under 1 MB, 1-10 MB,
    10-100 MB, 100 MB-1 GB, Over 1 GB.
- **Safety (all tabs):** top level only; real folders are never touched;
  hidden/system files (desktop.ini), Office `~$` lock files, links and
  reparse points, partial downloads (`.crdownload`, `.part`, …) and
  cloud-only OneDrive placeholders are skipped with a reason — placeholders
  are never opened, so nothing is downloaded. **Nothing is ever deleted or
  overwritten** ("report (2).pdf" names; a Windows move API that cannot
  replace). Every file is re-checked at run time (size + modified time).
- **Program files stay put in Date and Size modes** outside Downloads:
  exe, dll, sys, ini, msi, msix, appx, bat, cmd, ps1, vbs, com, scr, drv,
  cpl, ocx, lnk, url are left in place (moving program files by date or
  size could break a program if the wrong folder was picked). In the
  Downloads folder all non-skipped files may move.
- **Your Desktop folder cannot be chosen in the Folder tab** — use the
  Desktop tab for it.
- **Undo:** every move is journaled; the most recent run per target
  (desktop / downloads / custom) can be undone — reverse order, only EMPTY
  folders the tool created are removed, and files the user moved or
  deleted are reported "not found".
- **OneDrive honesty:** if the folder lives in OneDrive (including
  "OneDrive - Company" layouts) the preview says so; cloud-only files are
  skipped; OneDrive settings are never changed. Icons from the shared
  **Public Desktop** folder are left alone (Desktop tab).

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

A static, copy-only list: 8 main commands plus 9 in the collapsed
"More commands" drawer; no category headings. Multi-step commands have one
box per step, each with its own Copy button. A code box with more than six
lines starts collapsed: only the first six lines are visible (with a small
fade at the bottom) and a small More button under the box expands it and
turns into Less — Copy always copies the full code either way. Placeholders
are realistic sample text shown in green (angle brackets would break
PowerShell commands).
`winget upgrade --all` was removed on purpose (it can silently update tools
such as Python and break projects) — the list shows how to update one app
with `winget upgrade --id <Id> --exact` instead. The two installed-apps
commands (list them, or save the list as a CSV on the Desktop) are
PowerShell-only and were verified live on the development machine; the
save version ran with a temporary output path so no test file was left on
the Desktop. The drawer starts with four
hardware-check tips for checking a second-hand or suspicious laptop/PC:
graphics card hardware IDs, BIOS model/serial, RAM modules and total, and
drive size/model. Still static and copy-only: ERX Tools never runs them;
there is no clipboard plugin — copying uses the browser clipboard with a
plain fallback message.

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
