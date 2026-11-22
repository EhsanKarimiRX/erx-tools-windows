# System Manager

One tool, one scrolling page with three sections, in this fixed order:
**Awake → Quick Launch → Command Tips**.
Fully offline, no admin rights needed, English UI. Every section is honest:
no silent failures, no fake progress, plain-English reasons for anything
that did not happen.

## Device card (top of the page)

A read-only card above the three sections. It shows the processor (cores and threads), the graphics card, RAM (type, total and modules), the first internal drive, the Windows version, and battery health (or uptime on a PC without a battery). It is read again every time System Manager opens and with the small refresh button; nothing updates by itself.

- When there is more than one graphics card, drive or RAM module, the cell shows the first one plus "+N more" and a **More** button opens the full list.
- The discrete graphics card comes first; a PC without one shows the processor's graphics.
- Whatever Windows or the BIOS does not report is shown as "Not reported". The card never guesses.
- Empty RAM or drive slots are not shown: Windows cannot tell reliably.
- No administrator rights, no network, no computer name or serial numbers. See DECISIONS.md for how each value is read.

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

## Quick Launch

Ten square one-click buttons. The six tool buttons sit in one row and open
a Windows tool normally: Control Panel, Defragment and Optimize Drives,
Device Manager, Services, Desktop Icon Settings and Microsoft PC Manager.
Below a thin divider the four shell buttons, at the same size, are
centered under that row: **PowerShell**,
**PowerShell Run as administrator**, **Command Prompt** and **Command
Prompt Run as administrator**. Only the two **Run as administrator**
buttons use the purple gradient; they start the program elevated, so
Windows shows its permission (UAC) prompt first. Every other button is
neutral and starts its tool normally.

- The frontend sends only a fixed button id; the launch targets are
  hard-coded with full System32 paths, so a tampered PATH cannot hijack
  them.
- **Desktop Icon Settings** opens Windows' own page
  (`rundll32.exe shell32.dll,Control_RunDLL desk.cpl,,0`) — the same
  window as Settings → Personalization → Themes → Desktop icon settings,
  where the five classic icons (This PC, Recycle Bin, Control Panel,
  Network, User's Files) are shown or hidden. ERX Tools only opens it; it
  reads and writes no icon setting itself.
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
- Awake's power changes and the PC Manager launch were verified live on
  the development machine.
