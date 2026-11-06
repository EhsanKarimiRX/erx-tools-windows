PROMPT 5 — SYSTEM MANAGER
ERX Tools for Windows · new tool · steps 5a to 5g

STATUS: READY TO BUILD. All design questions are answered by the owner. Nothing in this prompt is "open". If something is not covered, follow rule G14 (stop and ask).

0. READ THIS FIRST — resume protocol

This prompt is long on purpose and is split into lettered steps (5a, 5b, ... 5g). The owner can hit a usage limit at any moment and will continue the next day, so the work MUST be resumable.

At the start of every session (new chat or continuation):

Check whether docs/PROMPT_5_PROGRESS.md exists.
It exists → read it fully, read docs/PROMPT_5_DECISIONS.md, run git status and git log -5 --oneline. Continue from the first step that is not marked DONE. If a step is marked IN PROGRESS, inspect the working tree (git status, git diff) to see what already exists, then finish that step. Do NOT restart a step from zero and do NOT redo DONE steps.
It does not exist → this is a fresh start. Begin with step 5a.
Re-read section 2 (Global rules) every session. They apply to every step.
After every lettered step, and after every numbered sub-step inside 5b and 5d, update docs/PROMPT_5_PROGRESS.md BEFORE you write your report.
When a lettered step is finished: all gates green → local commit → report (format in Appendix C) → STOP and wait for the owner. Do not begin the next lettered step until the owner says to continue (for example "continue" or "OK 5c").

The owner is a non-programmer. He verifies by hand on his Windows 11 machine. Every step ends with a short owner test checklist in plain language.

1. Project context (so you do not need to guess)
ERX Tools for Windows — lightweight, fast, fully OFFLINE toolbox for Windows 10+. No network calls, no telemetry, strict CSP, minimal Tauri permissions.
Local path C:\dev\erxtools. Remote origin = github.com/EhsanKarimiRX/erx-tools-windows. App identifier com.erxtools.windows.
Stack: Tauri 2 (WebView2), React 19 + TypeScript (strict) + Vite, lucide-react icons, Rust with windows-sys 0.61. Targets: Windows 10 AND 11, x64 + ARM64.
Tools are isolated folders, lazy-loaded only when opened. Read docs/ADDING_A_TOOL.md. Frontend registration: src/tools/_template + src/tools/registry.ts. Backend mirror: src-tauri/src/tools (mod.rs, error.rs, app_info.rs).
Existing tools: Image Converter, QR Code Generator, Folder Icon Changer (FINAL, accepted at commit fbb413d), Encrypted Notes (placeholder tile only).
Quality gates (all must be green after every lettered step):
npm run build (TypeScript strict + production build)
in src-tauri: cargo fmt --check, cargo clippy -- -D warnings, cargo test (baseline 202 Rust tests; the count may only grow)
the ARM64 check that was used in earlier prompts (find the exact command in docs/, in package.json, or in git log; run the same one)
windows-sys 0.61 quirks already learned: EnumChildWindows takes a bare HWND; edition 2024 needs explicit unsafe {} blocks inside unsafe fn; Path::eq_ignore_ascii_case does not exist; std Prefix::UNC has 2 fields. Cargo feature names for windows-sys modules must be added to Cargo.toml as needed (compile errors tell you which).
Owner's machine: Windows 11 (build 26300). You run code directly on it. That means your tests can change the owner's real settings — see rule G9.
2. Global rules (apply to ALL steps)

G1 — Never git push. The owner pushes personally. After a lettered step is complete and all gates are green, make a LOCAL commit following the message style in git log (for example Prompt 5b: Awake). Never push, never force anything.

G2 — Honest UI. No silent failures. No fake progress bars. Every batch operation ends with an honest report (succeeded / failed / skipped, with reasons). If something is not possible or not guaranteed, the UI says so in plain words.

G3 — Nothing destructive. Never delete or overwrite user files. Never rename user folders (the rename idea was removed from Folder Icon by the owner; never reintroduce it). Organize only MOVES loose files, and always has Undo. The only thing this tool may ever delete is an EMPTY category folder that the tool itself created (during Undo).

G4 — Offline. Our app makes no network calls. Do not add network-capable crates. Do not loosen the CSP. Keep Tauri capabilities/permissions minimal and list every permission, plugin, Cargo dependency and windows-sys feature you add in your step report. (Opening the Microsoft Store from a button is Windows doing it, not our app making a network call; Command Tips only shows text for the user to run himself.)

G5 — Windows 10 AND 11, x64 AND ARM64. Anything unavailable on one of them must degrade with an honest message, not an error.

G6 — No admin, no self-elevation. The app never asks for admin itself. The ONLY elevation is the explicit "Run as administrator" buttons in Quick Launch (5e), and the UAC prompts that some launched Windows tools show on their own.

G7 — English only. All UI text is English. No i18n framework. Use the draft wording given in this prompt; the owner reviews final wording at step 5g.

G8 — Do not invent facts. CLSIDs, power-setting GUIDs, Store IDs, package names, command-line flags: every value in this prompt marked [VERIFY] must be checked on the real machine (registry read, powercfg /query, running the command) BEFORE you rely on it. If it cannot be verified, say so in your report; never silently guess.

G9 — Protect the owner's real data during your tests.

Before any live test that touches system settings (power plan, desktop icon registry values), snapshot the original values into docs/PROMPT_5_PROGRESS.md (section "Owner snapshots — do not lose"), and after the test verify the settings equal the snapshot again. If anything fails, restore manually and tell the owner exactly what you restored.
NEVER run Organize against the owner's real Desktop or Downloads yourself. Use a scratch folder under the system temp directory. The owner tests on his real folders by hand.
Do not auto-launch admin (UAC) variants during your tests.

G10 — Gates after each lettered step. See section 1. Fix everything before reporting. Do not "allow" warnings.

G11 — Code structure. One UI section = one React component file + one Rust module. Keep pure logic (decisions, classification, state machines, parsing) separate from Windows API calls, with the Windows calls behind a small trait so unit tests can use a fake. Mirror the folder/file layout, naming, command-registration, capability and test conventions of the Folder Icon tool (read it first). Do not refactor unrelated code. Do not add UI libraries; reuse the existing styling approach.

G12 — Error handling. Use the existing error.rs pattern. No unwrap() / expect() in non-test code. User-facing error messages are plain English, one sentence, saying what happened and what the user can do.

G13 — Tests. Every pure-logic module gets unit tests (list given per step). Tests that touch the real Windows system are #[ignore]d and run only on purpose (cargo test -- --ignored), following G9.

G14 — Stop and ask. If a situation is not covered here and it changes behavior the owner will see, or if a step seems to require breaking a rule above, STOP and ask the owner in your report instead of guessing.

3. Final design (all decided by the owner)

System Manager = ONE new tool tile ("System Manager") opening ONE scrolling page with these sections, in this exact order:

Order	Section	Build step
1	Awake (first, most used)	5b
2	Desktop Icons (toggles)	5c
3	Organize (Desktop / Downloads / Custom folder)	5d
4	Quick Launch	5e
5	Command Tips (bottom-most, text only)	5f

Steps: 5a foundation, 5b Awake, 5c Desktop Icons, 5d Organize, 5e Quick Launch, 5f Command Tips, 5g final polish + verification.

Important owner decisions that differ from the earlier draft (do NOT implement the old draft):

No winget buttons at all. There is no "Check updates" / "Update" feature. winget is only mentioned as a copyable command in Command Tips (5f).
Awake does NOT use SetThreadExecutionState. It temporarily sets the Windows power-plan timeouts to Never and restores them (reason in Appendix A — read it).
No helper processes anywhere.
The Task Manager and Disk Cleanup shortcuts are NOT included. Device Manager and Services ARE included.
4. THE STEPS
5a — Foundation (tool scaffold + resume documents)

Goal: an empty "System Manager" tile/page wired exactly like other tools, plus the resume documents. No features yet.

5a-1. Study the existing code (read-only). Read: docs/ADDING_A_TOOL.md, src/tools/_template, src/tools/registry.ts, src-tauri/src/tools/mod.rs, error.rs, app_info.rs, and BOTH the frontend and backend folders of Folder Icon Changer (layout, naming, how commands are registered in the Tauri builder, how capabilities are declared in src-tauri/capabilities/*, where tests live, how styles are done). Also look at how Image Converter emits progress events to the frontend (you will reuse that pattern for events in 5b and 5d). Write a 10-line summary of the conventions at the top of docs/PROMPT_5_PROGRESS.md under "Conventions found".

5a-2. Save this prompt. Save the complete text of this prompt, verbatim, to docs/PROMPT_5_SPEC.md (if the owner already placed that file there, leave it as is). Do not summarize it.

5a-3. Create docs/PROMPT_5_DECISIONS.md with the exact content of Appendix A.

5a-4. Create docs/PROMPT_5_PROGRESS.md from the template in Appendix B.

5a-5. Scaffold the tool.

Frontend folder: src/tools/system-manager/ (mirror Folder Icon's naming style — if Folder Icon uses folder-icon, use system-manager). Backend folder: src-tauri/src/tools/system_manager/ (mirror Folder Icon's backend naming, probably snake_case).
Register the tile: name System Manager, short description "Awake, desktop icons, organizer and quick launchers." Choose a lucide-react icon: prefer MonitorCog; if the installed version does not have it, use Settings2.
Lazy-loaded like the other tools (follow ADDING_A_TOOL.md).
Page layout: a header, then a vertical stack of section cards. Create small shared components INSIDE the tool folder (check first whether the project already has equivalents and reuse them): SectionCard (title, one-line subtitle, children), InfoNote (small gray text), WarnNote (yellow note), Banner (dismissible message, variants: info / warning / error).
For now the page shows only the header and one gray line: "System Manager is being built section by section." (remove that line in 5g).

5a-6. Gates, local commit, report, STOP.

Done when: the System Manager tile appears on Home, opens an almost-empty page, goes back to Home; lazy-loading still works; cargo test still reports at least 202 passing; all gates green.

Owner test checklist 5a: (1) Open the app → there is a new "System Manager" tile. (2) Click it → page opens with the header. (3) Go back → Home works. (4) Other tools still open normally.

5b — Awake
Concept (read carefully)

When Awake is ON, the app changes three Windows power-plan settings, for both power sources (plugged in = AC, on battery = DC), of the currently active power scheme to Never (value 0):

Sleep after
Hibernate after
Turn off display after (skipped if the user ticked "Let the screen turn off normally")

When Awake is turned OFF, the original values are put back.

Why this approach (the owner's decision — keep this reasoning in the code comments of the Awake module): the owner uses Awake while a long, expensive computer run is in progress. If ERX Tools crashes (for example under heavy CPU load) while using the "execution state" API, Windows immediately goes back to normal rules, the PC sleeps and the run is lost — an unrecoverable, costly failure. With the power-plan approach, if ERX Tools crashes, the PC simply stays awake and at worst the screen stays on until one setting is fixed — cheap and recoverable. Therefore: do NOT use SetThreadExecutionState. Do NOT use a helper process.

Everything that makes this approach safe (all mandatory):

The original values are saved to a state file on disk BEFORE any setting is changed.
If the app crashed or was closed with Awake on, the next launch detects it from the state file and handles it.
Restore is per setting and never overwrites a manual change (rule R3 below).
If the user changes sleep/screen settings by hand in Windows while Awake is on, Awake switches itself off and notifies the user (rule R4).
Settings table [VERIFY every GUID with powercfg /aliases and powercfg /query SCHEME_CURRENT <subgroup> on the owner's machine before using it]
Name used in code	Subgroup GUID	Setting GUID	Unit
sleep (Sleep after)	238C9FA8-0AAD-41ED-83F4-97BE242C8F20 (SUB_SLEEP)	29F6C1DB-86DA-48C5-9FDB-F2B67B1F44DA (STANDBYIDLE)	seconds, 0 = Never
hibernate (Hibernate after)	238C9FA8-0AAD-41ED-83F4-97BE242C8F20 (SUB_SLEEP)	9D7815A6-7EE4-497E-8888-515A05F02364 (HIBERNATEIDLE)	seconds, 0 = Never
display (Turn off display after)	7516B95F-F776-4464-8C53-06167F40CC99 (SUB_VIDEO)	3C0BC021-C8A8-4E07-A973-6B14CBCB2B7E (VIDEOIDLE)	seconds, 0 = Never

So there are 6 keys: sleep_ac, sleep_dc, hibernate_ac, hibernate_dc, display_ac, display_dc.

State file

Location: Tauri app data directory (use the same method other tools use for app data; see 5a-1), file awake_state.json. Write it atomically (write awake_state.json.tmp fully, then rename over the real file). Example:

json
{
  "version": 1,
  "scheme_guid": "381b4222-f694-41f0-9685-ff5bb260df2e",
  "started_at_unix": 1762400000,
  "ends_at_unix": null,
  "keep_screen_on": true,
  "originals": {
    "sleep_ac": 1800, "sleep_dc": 900,
    "hibernate_ac": 0, "hibernate_dc": 0,
    "display_ac": 600, "display_dc": 300
  },
  "applied": {
    "sleep_ac": 0, "sleep_dc": 0,
    "hibernate_ac": 0, "hibernate_dc": 0,
    "display_ac": 0, "display_dc": 0
  }
}
originals[key] may be null if that value could not be read (then that key is never touched; applied[key] is also null).
applied[key] is the value we set, or null if we do not touch that key (for example the display keys when "Let the screen turn off normally" is ticked).
keep_screen_on = true means "display is also kept on (set to Never)". It is the DEFAULT (owner decision: the run may be image-related, so the screen must stay on by default).
ends_at_unix is null unless the optional auto-off timer is set.
Rules (the state machine)

R1 — Write state first. On enable: (1) read the active scheme GUID, (2) read all 6 original values, (3) write the state file, (4) only then change power values.

R2 — Apply and verify. For each key with a non-null applied value, write it with the power API, call PowerSetActiveScheme once, then read every changed key back and verify it equals the applied value. If anything fails: roll back the keys already changed to their originals, delete the state file, and return a plain-English error. Never leave a half-applied state without telling the user.

R3 — Restore never overwrites manual changes. Restore works per key: if the key's CURRENT value equals applied[key] → write originals[key] back. If it differs → leave it alone (the user changed it) and count it as "kept". Keys with applied = null are never touched. After restore, call PowerSetActiveScheme, delete the state file, and return a report { restored, kept, failed }. Restore always targets the scheme GUID stored in the state file (NOT whatever scheme is active now). If that scheme no longer exists, skip and report it.

R4 — Reconcile (detect manual changes and crashes). One function reconcile() compares the state file with reality: (a) is the active scheme GUID still the stored one? (b) does every key with a non-null applied still hold the applied value? Result: StillOn, Changed (any difference, including a different active scheme), or SchemeGone.

It runs: at app start (if a state file exists), every 5 seconds while Awake is ON (a single background thread), and when the window regains focus.
Changed / SchemeGone → Awake switches itself OFF: run restore (R3), then emit the event awake://auto-off with reason manual_change and show the notification (see "Notifications"). The user's manual changes are kept.
At app start with a state file: StillOn → show Awake as ON, flagged resumedFromPreviousSession = true with an info banner ("Awake was left on from an earlier session."). If ends_at_unix has already passed and it is StillOn → restore normally and notify "The Awake timer finished while ERX Tools was closed. Your settings were restored."
Our own writes must update the expected values, so our own changes are never mistaken for manual ones (R5).

R5 — Options while ON apply instantly (no Apply button).

Toggling "Let the screen turn off normally": ON-state → restore the two display keys to their originals (only if current == applied, per R3), set their applied to null, rewrite state file. Toggling it back: set both display keys to 0, set applied to 0, rewrite state file, verify.
Changing the timer: update ends_at_unix and rewrite the state file.

R6 — Timer. Optional, default "Never" (stay on until I turn it off). Choices: Never / 1 hour / 2 hours / 4 hours. The check runs in the same 5-second background thread. On expiry: restore (R3) and notify ("Awake timer finished. Your settings were restored."). The timer only runs while ERX Tools is open (the UI says so; see wording). If the window is closed with the timer set, the state file keeps ends_at_unix and the next launch handles it (R4).

R7 — Closing the app while ON. Intercept the window close in Rust (on_window_event → WindowEvent::CloseRequested → api.prevent_close(); chain with any existing handler in lib.rs/main.rs), emit event awake://close-requested, and let the frontend show a dialog with 3 choices. The frontend then calls the command awake_exit(choice):

restore_and_close → restore (R3), then app.exit(0).
keep_and_close → leave everything as is and app.exit(0) (state file stays; next launch resumes).
cancel → do nothing, the window stays open. If Awake is OFF, closing works normally with no dialog. (Crash / Task Manager kill / shutdown: nothing can be shown; the settings stay on Never — this is the intended crash behavior — and the next launch handles it via R4.)

R8 — Concurrency. One Mutex-protected Awake runtime in Tauri managed state. Every command (enable, disable, options, exit) and the background thread lock it. No two operations may interleave.

Windows API layer
Use windows-sys Win32_System_Power: PowerGetActiveScheme, PowerSetActiveScheme, PowerReadACValueIndex, PowerReadDCValueIndex, PowerWriteACValueIndex, PowerWriteDCValueIndex. Pass 0/null for the UserRootPowerKey. PowerGetActiveScheme returns a GUID pointer that must be freed with LocalFree. These functions return a Win32 error code (0 = success). Add the needed windows-sys feature names to Cargo.toml.
Writing the active scheme for the current user should NOT need admin (Windows Settings does it without UAC). [VERIFY] If a write returns access denied (for example because Group Policy locks the plan), the enable fails cleanly with a plain message, nothing is left changed.
If a needed function is missing from windows-sys 0.61 or misbehaves: STOP and ask (G14). Do not silently switch to spawning powercfg.exe.
Wrap all of this in trait PowerBackend { active_scheme(); read(scheme, key); write(scheme, key, value); activate(scheme); } with WindowsPower (real) and FakePower (in-memory, for tests, able to simulate failures and "manual changes").
Notifications (owner requirement)

When Awake switches itself off (manual change, timer finished, partial restore), the user must be told:

In-app banner (mandatory): persistent until dismissed. Event awake://auto-off with { reason, message }.
Native Windows notification (best effort): trigger it from Rust with tauri-plugin-notification (check whether a notification mechanism already exists in the project first). Adding this plugin is allowed ONLY if it needs no network access and no broad permissions; it is local-only. If it cannot be added cleanly, STOP and ask — the in-app banner alone is not enough for the owner's request, so do not silently skip it.
Message for manual change: "Awake was switched off because the sleep or screen settings were changed manually in Windows. Your changes were kept."
UI (draft wording — the owner will review at 5g)

Card title Awake. Subtitle: "Keeps this PC from sleeping and the screen from turning off — for long runs."

Layout, top to bottom:

One big toggle, label "Keep PC awake". Simple, first-glance, nothing else visible besides the status line.
Status line. OFF: "Awake is off." ON: "Awake is ON — sleep and screen-off are set to Never (plugged in and on battery)." If the screen option is ticked: "Awake is ON — sleep is set to Never (plugged in and on battery). The screen can still turn off."
Yellow note (WarnNote) visible ONLY while ON: "You can close ERX Tools and Awake will stay on. Open ERX Tools later to turn Awake off and restore your original settings."
A collapsible "Options" drawer (chevron arrow, closed by default) containing:
Checkbox "Let the screen turn off normally" with helper text: "By default Awake also keeps the screen on, because some runs (for example image processing) need it. Tick this if you only want to stop the PC from sleeping."
Select "Turn Awake off automatically": options "Never (stay on until I turn it off)" [default], "After 1 hour", "After 2 hours", "After 4 hours". Helper text: "The timer only runs while ERX Tools is open." While ON with a timer, show the remaining time ("Turns off in 1 h 42 min").
Changes in the drawer apply immediately, including while Awake is ON (R5).
Honest gray notes (InfoNote), always visible:
"How it works: Awake temporarily sets the Windows sleep and screen timeouts to Never and puts your original values back when you turn it off."
"Awake only blocks automatic sleep and screen-off. Clicking Sleep or shutting down still works, and closing a laptop lid follows your Windows lid setting."
Banners area above the toggle for: resumed-from-previous-session (info), auto-off events (warning, persistent), errors (error).
Close dialog (R7): title "Awake is still on". Text: "Awake is keeping this PC awake. What should happen?" Buttons: "Restore settings and close", "Keep Awake on and close", "Cancel". If a timer is set add: "Note: the timer only runs while ERX Tools is open."

TypeScript status type (adapt names to project style):

ts
type AwakeStatus = {
  state: "off" | "on";
  startedAtUnix: number | null;
  endsAtUnix: number | null;
  keepScreenOn: boolean;               // true = display also set to Never
  resumedFromPreviousSession: boolean;
};

Commands (names may follow project style): awake_get_status, awake_enable(options), awake_disable, awake_set_options(options), awake_exit(choice). Events: awake://changed, awake://auto-off, awake://close-requested. Reuse the event pattern from Image Converter; add only the minimum permissions needed.

Sub-steps

5b-1 — Pure logic + state file (no Windows API). Types, the 6-key model, state-file serialize/parse (versioned, tolerant of missing/extra fields, reject garbage without panicking), atomic write, restore_plan(originals, applied, current) (pure function implementing R3), reconcile_decision(state, current_values, active_scheme) (pure, R4), timer expiry check. Define PowerBackend + FakePower. Tests (minimum): state round-trip; corrupted file handled; atomic write leaves no half file; restore restores untouched keys; restore keeps manually changed keys; restore skips null keys; reconcile → StillOn / Changed / SchemeGone; different active scheme → Changed; timer expiry boundary; enable with a failing write rolls back everything (using FakePower); enable writes the state file BEFORE the first write (use FakePower to assert order).

5b-2 — Windows backend. WindowsPower implementing the trait. Verify the GUID table [VERIFY]. Add one #[ignore] live test (see 5b-5).

5b-3 — Runtime, commands, background thread, exit handling. Managed state + Mutex (R8), the commands, the 5-second thread (reconcile + timer), events, notification, close interception (R7), app-start state-file handling (R4).

5b-4 — UI. As specified above. Confirm the toggle never lies: it shows ON only if the backend says ON.

5b-5 — Live verification on the owner's machine (follow G9 exactly).

Snapshot: run powercfg /getactivescheme, powercfg /query SCHEME_CURRENT SUB_SLEEP, powercfg /query SCHEME_CURRENT SUB_VIDEO; paste the relevant values into the progress file ("Owner snapshots").
Run the ignored live test: enable → read back with powercfg /query and confirm Never → disable → confirm values equal the snapshot. Use a guard (Drop) so the restore runs even if the test fails.
Simulate a manual change: with Awake ON, run powercfg /change standby-timeout-ac 20 (minutes). Confirm reconcile switches Awake off, restores the other keys, and KEEPS the 20-minute value. Then set standby-timeout-ac back to the snapshot value yourself and verify.
Simulate a crash: Awake ON → kill the app process → restart → confirm "resumed" banner → turn off → confirm values equal the snapshot.
Final: confirm all power values equal the snapshot. Record the result in the progress file.

5b-6 — Gates, local commit, report, STOP.

Done when: all of the above verified; tests listed pass; gates green.

Owner test checklist 5b:

Open System Manager → Awake card is first. Toggle it on → status says ON, yellow note appears.
Open Windows Settings → System → Power: sleep and screen should show "Never" for both plugged in and battery. Toggle Awake off → your original values are back.
Turn on, open Options, tick "Let the screen turn off normally" → in Windows Settings the screen timeout returns to your original, sleep stays Never.
Turn on, then change the sleep setting by hand in Windows Settings → within ~5 seconds Awake switches off and you get a banner and a Windows notification.
Turn on, close the app → dialog with 3 choices. Try "Keep Awake on and close", reopen → app shows ON with the "left on" banner. Turn off → settings restored.
Set the timer to 1 hour (just check the countdown shows; you do not have to wait).
5c — Desktop Icons (toggles)

Goal: on/off switches for the classic system desktop icons, in one place, with the desktop updating immediately.

Registry facts (HKCU only, no admin): key HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\HideDesktopIcons\NewStartPanel. For each icon a REG_DWORD named by its CLSID: 1 = hidden, 0 = shown. Create the key if missing (RegCreateKeyExW). Use windows-sys Win32_System_Registry. Do NOT touch ClassicStartMenu or any other key. Do NOT spawn reg.exe or PowerShell.

The 5 icons and CLSIDs [VERIFY each — see verification below]:

UI name	CLSID	Default when the value is ABSENT
This PC	{20D04FE0-3AEA-1069-A2D8-08002B30309D}	hidden
Recycle Bin	{645FF040-5081-101B-9F08-00AA002F954E}	shown
Control Panel	{5399E694-6CE5-4D6D-8792-F8F34E1E2E6F}	hidden
Network	{F02C1A0D-BE21-4350-88B0-7367FC96EF3C}	hidden
User's Files (your user folder)	{59031a47-3f72-44a7-89c5-5595fe6b30ee}	hidden

IMPORTANT logic (a common bug): "value absent" does NOT mean "shown". It means "Windows default for that icon" (table above). Therefore:

Reading: visible = match value { Some(0) => true, Some(_) => false, None => default_visible_for(icon) }.
Writing: ALWAYS write an explicit 0 or 1, never delete the value.
Put this in a pure function with unit tests (all 5 icons × value 0/1/absent).

Reading state: read the registry every time the section is opened AND when the window regains focus, so manual changes in Windows Settings are reflected. After each write, read the value back; the switch shows the read-back state. If the write or read-back fails, the switch returns to its previous position and a red message appears under that row ("Couldn't change this icon: …").

Refresh — the desktop must update without logoff. This is the risky part (the Folder Icon work proved that heavy shell-notification layers can break desktop refresh on Windows 11). Procedure — try in this order, test each on the real desktop, and keep the smallest layer that works:

SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, null, null) — once.
If the icon does not appear/disappear: find the desktop view window (class Progman → child SHELLDLL_DefView; on some builds it is under a WorkerW window) and request a refresh. A known technique is SendMessageW(defview, WM_COMMAND, 0x7103, 0) — [VERIFY] it works on this machine; do not assume.
As a last resort post VK_F5 to the desktop list view. Record in the progress file which layer worked on the owner's Windows 11 build. Because the owner cannot be sure the refresh is instant on every PC, the UI shows this gray note: "Changes apply to your Windows account only. If the desktop does not update by itself, click an empty spot on the desktop and press F5."

UI (draft wording): card Desktop Icons, subtitle "Show or hide the standard desktop icons." Five rows: icon, name, switch. The gray note above. A second small gray line: "This changes the same settings as Windows Settings → Personalization → Themes → Desktop icon settings."

Verification of CLSIDs [VERIFY]: (1) read reg query "HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\HideDesktopIcons\NewStartPanel" and note which CLSIDs already exist (Windows only creates a value after the user has changed that icon at least once); (2) the DECISIVE test is the owner's: he toggles each icon in Windows Settings → Desktop icon settings and confirms the app mirrors it, then toggles in the app and confirms the desktop changes. If any CLSID is wrong, fix it. (3) Snapshot the five values (or "absent") BEFORE you test, and restore them afterwards (G9).

Tests (minimum): visibility function (5 icons × 0/1/absent); CLSID table has 5 unique entries; write-then-readback logic with a fake registry backend (trait RegistryBackend); failure path returns an error without panicking. One #[ignore] live test using the snapshot/restore protocol.

Gates, local commit, report, STOP.

Owner test checklist 5c:

Open Desktop Icons: the switches match what Windows shows now.
Switch "This PC" on → the icon appears on the desktop (if not, press F5 on the desktop and tell me; I need to know).
Switch each of the 5 icons on and off.
In Windows Settings → Personalization → Themes → Desktop icon settings, change one → come back to the app → the switch matches.
Leave the icons the way you like them.
5d — Organize

Goal: ONE organizing engine with THREE targets — Desktop, Downloads, Custom folder… — with a plain-language preview before anything moves, an honest report afterwards, and Undo.

Owner priorities: (1) nothing is ever deleted or overwritten; (2) before applying, explain in very simple words what is going to happen; (3) be honest about OneDrive.

Shared safety rules (all targets)
Top level only. Read the folder non-recursively. Never enter sub-folders.
Only loose files are moved. Real folders are never touched, renamed or moved.
Never moved: files with the Hidden or System attribute (this covers desktop.ini); files whose name starts with ~$ (Office lock files); symlinks and any reparse point (files or folders); files with partial-download extensions .crdownload .part .partial .download .opdownload .tmp; cloud-only OneDrive placeholder files (attributes FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS 0x400000, FILE_ATTRIBUTE_RECALL_ON_OPEN 0x40000 or FILE_ATTRIBUTE_OFFLINE 0x1000) — skip them with reason "online-only (OneDrive)".
Never overwrite. If the destination name exists, use  (2),  (3), … before the extension (report.pdf → report (2).pdf), up to 999 tries. Do NOT use std::fs::rename blindly — on Windows it can replace an existing file. Move with MoveFileExW(src, dst, 0) (flags 0: no MOVEFILE_REPLACE_EXISTING, no copy). All moves stay inside the same folder tree/volume, so a plain rename is enough. On ERROR_ALREADY_EXISTS (183) / ERROR_FILE_EXISTS (80) try the next suffix. On ERROR_SHARING_VIOLATION (32), ERROR_LOCK_VIOLATION (33) or ERROR_ACCESS_DENIED (5): skip the file and report "in use or protected". Any other error: report as failed with the Windows message.
Existing category folders are reused. If the target already has a real folder named Documents (for example), files go into it. If a category name is already taken by a FILE, or the folder is a reparse point/junction, that category cannot be used: its files are skipped with reason "name conflict".
Category folders are created with std::fs::create_dir (not create_dir_all) only when missing; every folder we create is recorded for Undo.
Paths come from Windows, never hardcoded: use SHGetKnownFolderPath with FOLDERID_Desktop and FOLDERID_Downloads (free the string with CoTaskMemFree). Desktop may live inside OneDrive; Downloads may be on another drive. Add the needed windows-sys features (Win32_UI_Shell, Win32_System_Com, Win32_Storage_FileSystem, …).
Windows' virtual desktop icons (This PC, Recycle Bin…) are not files, so the organizer cannot touch them. Icons coming from the shared Public Desktop folder are NOT part of the user's Desktop folder and are left alone.
Categories

Match extensions case-insensitively using the text after the LAST dot. No extension = unknown.

Desktop target — 3 categories:

Apps: exe, msi, msix, appx, lnk, url, appref-ms
Documents: pdf, doc, docx, odt, rtf, txt, md, xls, xlsx, ods, csv, ppt, pptx, odp, epub
More: every other file

Downloads target — 7 categories:

Documents: same list as above
Images: jpg, jpeg, png, gif, bmp, webp, tif, tiff, ico, svg, heic, heif, avif
Videos: mp4, mkv, avi, mov, wmv, flv, webm, m4v, mpg, mpeg
Music: mp3, wav, flac, aac, ogg, m4a, wma, opus
Archives: zip, rar, 7z, tar, gz, bz2, xz, tgz, iso
Installers: exe, msi, msix, msixbundle, appx, appxbundle
More: every other file

Custom folder target — 5 categories, content types only: Documents, Images, Videos, Music, Archives (same extension lists as Downloads). Everything else stays where it is — executables (exe, msi…), dll, sys, ini, scripts and all unknown types. Reason: moving runtime files could break a program if the user picked the wrong folder. Files left in place are reported as "left in place", not as errors.

Custom folder safety: the folder picker must reuse whatever folder-selection mechanism Folder Icon already uses (do not add a new dialog plugin if one exists). REFUSE with a plain message ("This folder is protected. Please choose a normal folder.") when the canonical path is: a drive root (C:\), the Windows folder or anything under it, Program Files, Program Files (x86), ProgramData, the user's profile root itself (C:\Users\<name>), or AppData (and anything under those).

Flow
User picks a target (Desktop / Downloads / Custom…).
Scan (read-only) → returns a plan: for each category the list of files that WOULD move (name, size), the skipped files with reasons, the "left in place" count, and on_onedrive (true if the folder path is under the OneDrive folder: compare against env OneDrive, OneDriveConsumer, OneDriveCommercial, or a path segment named OneDrive…).
Preview in plain words (see wording) with an expandable file list per category (cap ~200 names, then "and N more").
User clicks Organize now (or Cancel).
Execute. The backend re-checks each planned file (still exists, same size and modified time). A file that changed since the preview is skipped with reason "changed since preview"; files that appeared after the preview are never moved. Show a progress count (reuse the Image Converter event pattern); no fake percentages.
Honest report: "X moved, Y skipped" plus the list of skipped files with reasons.
Undo last organize button for this target (only the most recent run per target is undoable — say so in a gray note).

Preview wording (draft):

Ready to organize: Desktop ERX Tools will move 18 loose files into 3 folders on your Desktop: • Documents — 9 files • Apps — 5 files • More — 4 files Nothing is deleted or overwritten. Folders are never touched. If a name already exists, the file is renamed like "report (2).pdf". 3 files will be skipped (2 are hidden or in use, 1 is online-only in OneDrive). You can undo this afterwards. [Organize now] [Cancel]

OneDrive note (shown for Desktop when on_onedrive): "Your Desktop is stored in OneDrive. ERX Tools can organize it and OneDrive will sync the changes. Online-only files are skipped. ERX Tools never changes any OneDrive settings." Public Desktop gray note (Desktop target): "Icons from the shared Public Desktop folder are not part of your Desktop folder and are left alone."

Undo manifest

One append-only JSON Lines file per target kind in the app data dir: organize_desktop.jsonl, organize_downloads.jsonl, organize_custom.jsonl. A new run on the same target replaces that target's previous manifest. Lines:

{"type":"run","version":1,"target":"desktop","root":"C:\\Users\\...\\Desktop","started_at":1762400000}
{"type":"mkdir","path":"C:\\...\\Desktop\\Documents"}
{"type":"move","from":"C:\\...\\Desktop\\a.pdf","to":"C:\\...\\Desktop\\Documents\\a.pdf"}
{"type":"end","moved":18,"skipped":3}

Append and flush one line per move, immediately after the move succeeds, so a crash never loses track of a moved file. A missing end line (crash) must still be undoable. organize_custom.jsonl stores the root; show the Undo button for Custom only together with its folder name ("Undo last organize in <folder name>").

Undo procedure: read the manifest, process move lines in REVERSE order. For each: if to exists and from is free → move it back (again with flags 0, never overwrite). If from is now occupied → restore as name (2).ext and report it. If to no longer exists (user moved/deleted it) → skip and report "not found". Afterwards, for each mkdir path (reverse order) call std::fs::remove_dir — it only succeeds if the folder is EMPTY, which is exactly the safety wanted; if it fails, leave it. Delete the manifest after a fully successful undo; after a partial one, rewrite it with only the entries that still need undoing. Show an honest undo report ("17 restored, 1 not found").

UI (draft wording)

Card Organize, subtitle "Tidy loose files into folders. Nothing is deleted." A segmented control: Desktop | Downloads | Custom folder…. Custom shows the chosen path with a "Choose folder…" button. Below: Scan button → preview panel → Organize/Cancel → report panel → Undo button. Disable the target switcher while a run is in progress.

Sub-steps

5d-1 — Pure logic. Category tables (per target), classify(file_name, target) -> Destination { Category(name) | LeaveInPlace | Skip(reason) }, skip-rule predicates operating on a plain struct (name, attributes, is_reparse, is_dir), unique_name(base, exists_fn), plan-building from a list of entries, protected-path check for custom folders (pure, takes a path + a list of protected roots). Tests (minimum): every category list ↔ extension case-insensitivity; no extension; .tar.gz → by last extension; Desktop .png → More; Downloads .exe → Installers; Custom .exe/.dll/.ini/.xyz → LeaveInPlace; skip rules (hidden, system, desktop.ini, ~$, partial extensions, reparse, cloud-only); unique_name produces (2), (3)…; protected paths refused (drive root, Windows, Program Files, profile root, AppData); a normal folder accepted.

5d-2 — Scan. Known folder resolution; real directory listing with symlink_metadata and Windows attributes; on_onedrive; command organize_scan(target, custom_path?). Tests with a scratch directory under std::env::temp_dir() (create a unique subfolder; clean up; do NOT add the tempfile crate unless it is already a dev-dependency): files of every kind, a hidden file, a sub-folder (must be ignored), a .crdownload.

5d-3 — Execute + manifest + undo. organize_run, organize_undo, JSONL writer/reader, progress events. Tests in scratch dirs: all files moved to the right folders; collision → (2); never overwrites (create a pre-existing file in the destination and assert its content is unchanged); locked file skipped (hold a file open with exclusive sharing and assert it is skipped, not failed hard); existing category folder reused; category name taken by a file → skipped with "name conflict"; manifest written line by line; undo restores everything and removes only EMPTY folders we created; undo when a file was deleted/moved by the user; undo when the original name is occupied; crashed run (manifest without end) is undoable; a file changed after the scan is skipped.

5d-4 — UI, OneDrive and Public Desktop notes, report/undo panels, gates, local commit, report, STOP.

Owner test checklist 5d (do this on COPIES first if you are nervous):

Make a test folder with a few documents, images, a zip and an .exe. Choose Custom folder → Scan → the preview describes in plain words what will happen. The .exe and unknown files are "left in place".
Organize now → files go into Documents/Images/Archives. Check nothing was deleted.
Undo last organize → everything is back and the empty folders are gone.
Try Desktop: Scan → read the preview → Organize → check the three folders → Undo.
Try Downloads the same way (a partially downloaded file must be skipped).
Try choosing C:\Windows as a custom folder → it must be refused.
5e — Quick Launch

Goal: one-click buttons that open common Windows tools. Honest about what happens when a tool is missing.

Buttons (draft labels):

Control Panel
Defragment and Optimize Drives
Command Prompt (+ small secondary button "Run as administrator")
PowerShell (+ small secondary button "Run as administrator")
Device Manager
Services
Microsoft PC Manager

Security design (mandatory): the frontend sends only an ID from a fixed enum, never a path or a command string. Rust maps each ID to a hard-coded allowlisted target. Unknown ID → error. IDs: control_panel, defrag, cmd, cmd_admin, powershell, powershell_admin, device_manager, services, pc_manager, pc_manager_store.

Targets (use full paths from the system directory: GetSystemDirectoryW or %SystemRoot%\System32, so a poisoned PATH cannot hijack them):

control.exe
dfrgui.exe (this tool shows UAC by itself — that is normal)
cmd.exe
PowerShell: prefer PowerShell 7 (pwsh.exe) if it exists, otherwise Windows PowerShell (%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe). Look for pwsh.exe in %ProgramFiles%\PowerShell\7\, in %LOCALAPPDATA%\Microsoft\WindowsApps\ (Store alias), and by scanning the PATH entries manually. Do NOT spawn where. Pure function choose_powershell(exists_fn, env_values) with unit tests. Show under the button a small caption saying which one will start ("PowerShell 7" or "Windows PowerShell").
devmgmt.msc, services.msc (full path in System32; opening an .msc via ShellExecute open works through MMC).
Launch with ShellExecuteExW (SHELLEXECUTEINFOW), verb "open", or "runas" for the two admin variants, SW_SHOWNORMAL. If GetLastError is 1223 (ERROR_CANCELLED, the user dismissed UAC) show a neutral "Cancelled" message, not an error. [VERIFY by cancelling a UAC prompt]. Any other failure → plain error ("Couldn't open Command Prompt: <Windows message>").
We cannot know whether the tool really finished opening after a successful call; the UI only claims "Opened" when the launch call succeeded, and shows no fake status.

Microsoft PC Manager (not preinstalled — owner decision):

Installed → the button opens it.
Not installed → the button label reads "Microsoft PC Manager (not installed)"; clicking it shows an inline message: "Microsoft PC Manager is a Microsoft app that is not included with Windows. You can get it from the Microsoft Store." plus a button "Open Microsoft Store page". If opening the Store fails: "Couldn't open the Microsoft Store. You can search for 'Microsoft PC Manager' there yourself. It may also not be available in your region or on managed PCs."
Do NOT hard-code a Store product ID or package name from memory (G8). Procedure:
Detection (no process spawning): it is a Store (MSIX) app. Enumerate the subkeys of HKCU\Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\Repository\Packages and look for a package whose name contains PCManager (case-insensitive) [VERIFY on a machine where it is installed]. A full package name has the shape Name_Version_Arch_ResourceId_PublisherId; the family name is Name_PublisherId.
Launch: find the application id by reading the package's AppxManifest.xml (the package key has a PackageRootFolder value [VERIFY]) and extracting <Application … Id="…"> with simple string search (no new XML crate), then run explorer.exe shell:AppsFolder\<FamilyName>!<AppId> via ShellExecute. [VERIFY] this opens the app.
Store link: use the search deep link ms-windows-store://search/?query=Microsoft%20PC%20Manager (needs no product ID). If you can verify the real product page link (for example from the official Microsoft listing) you may use ms-windows-store://pdp/?productid=<ID> instead — but only if verified. Open it with ShellExecute from Rust (no frontend opener permission needed).
If the owner's machine does not have PC Manager installed, you cannot live-test the "installed" path. Implement it from the verified registry layout if you can, mark it "installed path NOT live-tested" in the progress file, and ask the owner in your report to install PC Manager from the Store and run the checklist below. If this approach does not work within reasonable effort: STOP and ask (G14); do not invent a workaround with PowerShell scripts.
Pure function pc_manager_state(installed_packages) -> Installed{family, app_id} | NotInstalled with unit tests.

Honest note under the buttons (InfoNote): "Opening these tools does not need administrator rights. Some tools ask for permission themselves when they start, for example Defragment and Optimize Drives."

Tests (minimum): ID → target mapping table (every ID, and unknown ID rejected); choose_powershell (pwsh present in each location / absent / both); admin IDs use verb runas; PC Manager state function; protocol URI constants are exactly the allowlisted strings. Do not auto-launch admin variants in tests (G9). Launching Control Panel or cmd once for a live smoke test is fine; ask the owner to test the rest.

Gates, local commit, report, STOP.

Owner test checklist 5e:

Click Control Panel, Device Manager, Services, Defragment and Optimize Drives (a UAC prompt is expected for this one).
Click Command Prompt and PowerShell. Check the caption says which PowerShell starts.
Click "Run as administrator" for cmd and PowerShell → UAC appears → Allow → an elevated window opens ("Administrator" in the title). Try again and press No on UAC → the app shows "Cancelled", not an error.
Microsoft PC Manager: if it is NOT installed, click it → you see the explanation and the Store button; the Store opens. Install it, reopen ERX Tools → the button now opens the app.
5f — Command Tips (bottom-most section, frontend only)

Goal: a static list of useful commands the user can copy and run himself in PowerShell/Command Prompt. This replaces the old winget buttons idea: the owner prefers doing it in PowerShell.

No backend. Put the data in one typed file (e.g. commandTips.ts) so tips are easy to edit later.

UI (draft): card Command Tips, subtitle "Handy Windows commands. Copy one, then paste it into PowerShell or Command Prompt (the buttons above open them)." Each tip is a row/card with: title, one-line explanation, the command in a monospace box with a Copy button, and small badges: shell (PowerShell, Command Prompt or Either), Needs administrator (if true), Can take a long time (if true), and Uses the internet (winget only). The command text must be selectable (user-select: text). Copy feedback: the button briefly changes to "Copied". If navigator.clipboard.writeText fails, fall back to a hidden <textarea> + document.execCommand('copy'); if that also fails show "Couldn't copy. Select the text and press Ctrl+C." Do not add a Tauri clipboard plugin. Where a command contains a placeholder, show it exactly as <folder path> style and add a gray caption "Replace the part in < > before running."

The tips (written from memory — YOU MUST VERIFY every one on the owner's machine, G8). For each tip: run it (or its /? / -? help for the dangerous or long ones), confirm the syntax and decide the badges from reality. Do NOT run sfc /scannow, DISM … /RestoreHealth or winget upgrade --all — verify their syntax with sfc /?, DISM /? / DISM /Online /Cleanup-Image /? and winget upgrade --help only. Do not print or save any Wi-Fi password during verification: run only netsh wlan show profiles and netsh wlan show profile name="<name>" key=clear /?-style help, not the real key output.

#	Title	Command(s)	Shell	Notes
1	List app updates	winget upgrade	Either	Shows which installed apps have updates. Uses the internet (winget does it; ERX Tools does not). Not every app appears.
2	Update all apps	winget upgrade --all	Either	Slow; may show several permission (UAC) prompts. Uses the internet.
3	Show a folder tree	tree "<folder path>" /f	Either	Lists folders and files under a folder.
4	Laptop battery health report	powercfg /batteryreport /output "%USERPROFILE%\battery-report.html"	Command Prompt	Creates an HTML report in your user folder. Laptops only. (%USERPROFILE% only works in Command Prompt — verify; decide the shell badge from reality.)
5	Repair Windows system files	sfc /scannow then DISM /Online /Cleanup-Image /RestoreHealth	Either	Needs administrator; can take a long time.
6	Flush the DNS cache	ipconfig /flushdns	Either	Can help with internet/website problems. Decide the admin badge from reality.
7	System information	systeminfo	Either	Prints Windows version, RAM, uptime and more.
8	Drives: size and health	Get-Volume	PowerShell	Lists drives with size, free space and health.
9	Check a downloaded file is intact	Get-FileHash "<file path>"	PowerShell	Prints the SHA-256 hash to compare with the website's value.
10	Top 10 CPU-hungry processes	Get-Process | Sort-Object CPU -Descending | Select-Object -First 10	PowerShell	In the UI show a real `
11	Show a saved Wi-Fi password	netsh wlan show profiles then netsh wlan show profile name="<network name>" key=clear	Either	Shows the saved password of a network on THIS PC. Private — do not share the output. Decide the admin badge from reality.

Group them with small headings if it looks cleaner (for example "Apps", "Files", "Repair", "Network", "Info") — keep it simple. At the end of the card add a gray line: "These commands are only text. ERX Tools never runs them for you."

Tests/gates: npm run build green. If the project has any frontend test setup, add a tiny test that every tip has a non-empty title/command and valid badges; if it does not, do not add a test framework. Local commit, report, STOP.

Owner test checklist 5f: (1) Scroll to the bottom → tips are listed with badges. (2) Click Copy on 3 different tips and paste into Notepad → text is exact (including <…> placeholders). (3) Open PowerShell with the Quick Launch button, paste Get-Volume → it works. (4) Tell me if any command should be added or removed.

5g — Final polish and verification
Remove the "being built section by section" placeholder line.
Verify the section order on the page: Awake, Desktop Icons, Organize, Quick Launch, Command Tips.
Produce docs/SYSTEM_MANAGER_UI_TEXT.md: a table of EVERY user-visible string in System Manager (section, element, text) so the owner can review and edit wording in one place. Do not change wording on your own after that file is produced; wait for the owner's edits.
Run ALL gates incl. the ARM64 check. Report the final cargo test count (must be ≥ 202 + the new tests).
Audit and list in the report: new Cargo dependencies, new windows-sys features, new Tauri plugins/capabilities/permissions, any CSP change (expected: none), any network-capable code (expected: none; grep for http, fetch(, reqwest, TcpStream).
Update docs: add System Manager to any tool list in README/docs/ (follow how Folder Icon is documented); add a short docs/SYSTEM_MANAGER.md describing each section, the Awake design and the known limits (lid close, timer only while app is open, Public Desktop, OneDrive, PC Manager installed-path testing status, desktop refresh behavior on the owner's build).
Mark all steps DONE in docs/PROMPT_5_PROGRESS.md. Local commit (never push). Final report. STOP. The owner reviews wording and pushes personally.
APPENDIX A — content for docs/PROMPT_5_DECISIONS.md (create it with exactly this text in step 5a-3)
markdown
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
APPENDIX B — template for docs/PROMPT_5_PROGRESS.md (create it in step 5a-4)
markdown
# Prompt 5 — System Manager: progress

Last updated: <date/time> by the agent.

## How to resume
Read docs/PROMPT_5_SPEC.md (full prompt) and docs/PROMPT_5_DECISIONS.md. Continue at the first step not marked DONE. Never git push.

## Status
| Step | Description | Status | Notes |
|---|---|---|---|
| 5a | Foundation (tile, docs) | NOT STARTED | |
| 5b-1 | Awake: pure logic + state file | NOT STARTED | |
| 5b-2 | Awake: Windows power backend | NOT STARTED | |
| 5b-3 | Awake: runtime, commands, thread, exit | NOT STARTED | |
| 5b-4 | Awake: UI | NOT STARTED | |
| 5b-5 | Awake: live verification | NOT STARTED | |
| 5c | Desktop Icons | NOT STARTED | |
| 5d-1 | Organize: pure logic | NOT STARTED | |
| 5d-2 | Organize: scan | NOT STARTED | |
| 5d-3 | Organize: execute, manifest, undo | NOT STARTED | |
| 5d-4 | Organize: UI | NOT STARTED | |
| 5e | Quick Launch | NOT STARTED | |
| 5f | Command Tips | NOT STARTED | |
| 5g | Final polish + verification | NOT STARTED | |
(Status values: NOT STARTED / IN PROGRESS / DONE / BLOCKED — waiting for owner)

## Gates (last run)
- npm run build: 
- cargo fmt --check: 
- cargo clippy -- -D warnings: 
- cargo test (count): 
- ARM64 check: 

## Conventions found (filled in 5a-1)

## Owner snapshots — do not lose
(Original power values, desktop icon registry values, etc. recorded BEFORE live tests.)

## Verified facts ([VERIFY] items)
(GUIDs, CLSIDs, which desktop-refresh layer worked, PC Manager package details, command results...)

## Open issues / questions for the owner

## Dependencies / permissions / windows-sys features added so far
APPENDIX C — report format after every lettered step
What was done (3–6 lines, plain English).
Gates: results of build / fmt / clippy / test (with count) / ARM64.
Files created/changed (short list).
New dependencies / permissions / windows-sys features (or "none").
Verified facts and anything that could not be verified ([VERIFY] items).
Owner test checklist (copy the step's checklist, adjusted to what really happened).
Known limits / honest caveats.
Questions for the owner (only if a G14 situation occurred).
Progress file updated: yes. Local commit: hash and message. Pushed: NO (never).
"Waiting for the owner to say continue."
FINAL REMINDERS FOR THE AGENT
Start by following section 0. Then do 5a only. Stop after each lettered step.
Verify every [VERIFY] value on the machine. Do not guess GUIDs, CLSIDs, package names or Store IDs.
Never touch the owner's real settings or folders in your tests without the snapshot/restore protocol (G9).
Never git push. Never delete or overwrite user files. Never rename user folders.
Keep the UI honest, English-only and simple.