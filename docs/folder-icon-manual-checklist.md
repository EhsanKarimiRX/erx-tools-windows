# Manual test checklist — Folder Icon Changer (Prompt 4)

Windows 11 only. Fill the Result column with ✔ or ✘ (plus a short note on
what failed). Rows 1–4 cover the designer, rows 5–12 the apply/restore
core (the focus of checkpoint 4B), rows 13–19 the full experience.

| # | Test | Result |
|---|------|--------|
| 1 | Pick a preset color: previews update immediately | [ ] |
| 2 | Type an invalid HEX: inline error, last valid color kept | [ ] |
| 3 | Each pattern renders; pattern color, size and opacity work | [ ] |
| 4 | Small previews (16/32/48) are recognizable, pattern not noisy | [ ] |
| 5 | Apply to a new folder in C:\dev\test-folders: Explorer shows the icon (also after F5) | [ ] |
| 6 | Apply to a subfolder on my Desktop: Desktop shows the icon | [ ] |
| 7 | Rename and move that folder: the icon stays | [ ] |
| 8 | Apply a different color to the same folder: icon changes without logging off | [ ] |
| 9 | Restore: default icon returns; our hidden files are gone | [ ] |
| 10 | Folder with spaces, Persian letters or a long name works | [ ] |
| 11 | Folder with an existing desktop.ini (custom InfoTip): InfoTip is preserved | [ ] |
| 12 | Drop a file, C:\Windows, C:\, Downloads, a network path (if available): all show the blocked message | [ ] |
| 13 | Drop 5 folders (1 blocked): correct per-folder results; Explorer does not freeze or flicker badly | [ ] |
| 14 | Read-only or no-permission folder: friendly error, no admin prompt | [ ] |
| 15 | Folder on a USB drive (if I have one) | [ ] |
| 16 | Light and dark theme look good | [ ] |
| 17 | Keyboard-only usage works | [ ] |
| 18 | The app never asks for administrator rights | [ ] |
| 19 | Windows 10 | Not tested (no device) |

Notes for rows 5–12 (checkpoint 4B focus):

- Row 5/8 check the targeted Shell refresh: the icon should appear (and
  change) WITHOUT a logoff. If Explorer needs an F5 every time, that is a
  finding — note it.
- Row 7 checks the RELATIVE IconResource path: the icon must survive a
  rename/move of the folder. If it breaks, that is a finding — the tool
  then falls back to absolute paths (recorded in DECISIONS.md).
- Row 12: in 4B there is no drag & drop yet — the folder picker can open
  the locations instead (e.g. pick C:\Windows directly, pick a file via
  the picker is not possible — use row 12 fully in 4C).
