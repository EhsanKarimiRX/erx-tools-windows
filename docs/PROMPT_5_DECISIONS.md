# Prompt 5 — System Manager: decisions log

Owner decisions, with reasons. Do not reverse any of these without asking the owner.

## Structure
- One tool tile "System Manager" with five sections in this UI order: Awake, Desktop Icons, Organize, Quick Launch, Command Tips.
- Awake is first because the owner expects it to be the most used section.
- Build order = UI order (5b Awake, 5c Desktop Icons, 5d Organize, 5e Quick Launch, 5f Command Tips). Each section is usable on its own.

## Awake — power-plan timeouts, NOT SetThreadExecutionState
- Decision: while Awake is ON, the active power plan's Sleep, Hibernate and Display-off timeouts are set to Never (AC and battery). Turning Awake off restores the original values.
- The earlier draft used SetThreadExecutionState (PowerToys Awake style). REJECTED by the owner.
- Reason (owner): the owner uses Awake while a long, expensive computer run is in progress. Compare the worst cases:
  - Execution-state approach: if ERX Tools crashes (for example under heavy CPU load), Windows immediately returns to normal rules, the PC may go to sleep and the run is lost. Expensive and unrecoverable.
  - Power-plan approach: if ERX Tools crashes, the PC simply stays awake; at worst the screen stays on until one setting is changed back. Cheap and recoverable.
  - Therefore the power-plan approach is the safer failure mode for the owner's real use.
- Consequences and mitigations: original values are saved to a state file on disk BEFORE any change; the next launch detects an unfinished Awake and handles it; restore is per setting and never overwrites a manual change; a manual change in Windows Settings switches Awake off and notifies the user.
- Default: the display is also set to Never (a run may be image-related and need the screen on). Option "Let the screen turn off normally" lets the display follow the normal timeout.
- The simple toggle is all the user sees; Options live in a collapsible drawer. Default timer = none (stays on until turned off). Optional timer 1/2/4 hours, which only runs while the app is open.
- Closing the app while Awake is on shows 3 choices: restore and close / keep Awake on and close / cancel. A yellow note under the toggle tells the user they may close the app and Awake stays on.
- Honest limits shown in the UI: Awake blocks only automatic sleep/screen-off; manual Sleep still sleeps; the laptop lid follows the Windows lid setting.
- No helper processes.

## Desktop Icons
- Five icons: This PC, Recycle Bin, Control Panel, Network, User's Files.
- The owner's need: Windows buries this setting; he wants easy access. Technical detail (absent value = Windows default per icon) is handled by the agent.

## Updates / winget
- NO winget update buttons in the app. The owner prefers PowerShell for this.
- Instead, the "Command Tips" section shows copyable commands (including `winget upgrade`).
- This also removes winget table parsing and keeps the app fully offline.

## Organize
- One engine, three targets: Desktop, Downloads, Custom folder.
- Owner priorities: nothing is ever deleted; a very simple preview explains what will happen BEFORE applying; honesty about OneDrive.
- Preview before applying: accepted. Undo for the last run per target: accepted.
- Desktop uses 3 categories (Apps / Documents / More); Downloads uses 7; Custom folder uses content types only and leaves executables, runtime files and unknown types in place.
- Real folders are never touched. Cloud-only OneDrive files are skipped. ERX Tools never changes OneDrive settings; if the Desktop is in OneDrive, the panel says so.
- Undo may remove only EMPTY category folders created by the tool itself.

## Quick Launch
- Buttons inside the panel (no desktop shortcuts): Control Panel, Defragment and Optimize Drives, Command Prompt, PowerShell, Device Manager, Services, Microsoft PC Manager.
- Disk Cleanup NOT included (PC Manager's cleanup is better). Task Manager NOT included (already easy to reach).
- Admin variants only for Command Prompt and PowerShell.
- PowerShell: prefer PowerShell 7 if installed, else Windows PowerShell.
- PC Manager is not preinstalled: if installed, open it; if not, explain honestly and offer the Microsoft Store page. No winget install.

## Command Tips
- Static, copy-only list at the very bottom. ERX Tools never runs these commands.

## Global
- Windows 10 and 11. No permanent admin. No silent failures. Honest reports. English UI. Final wording reviewed by the owner at 5g. The owner pushes to git; the agent never does.
