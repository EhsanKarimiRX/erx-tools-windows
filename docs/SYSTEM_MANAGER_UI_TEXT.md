# System Manager — every user-visible string

One row per string, grouped by section, so the owner can review and edit
wording in ONE place. **The agent does not change any wording after this
file is produced; the owner's edits here are the source of truth** (they
will be applied to the code as a follow-up).

Columns: where the string appears (element), the exact current text, and —
for strings produced by Rust — the file that owns them.

## Tile and page

| Element | Text |
|---|---|
| Home tile name / page title | System Manager |
| Tile description (tooltip) | Awake, desktop icons, organizer and quick launchers. |

## Awake (AwakeSection.tsx)

| Element | Text |
|---|---|
| Card title | Awake |
| Card subtitle | Keeps this PC from sleeping and the screen from turning off — for long runs. |
| Toggle label | Keep PC awake |
| Status while checking | Checking Awake… |
| Status OFF | Awake is off. |
| Status ON (screen kept on) | Awake is ON — sleep and screen-off are set to Never (plugged in and on battery). |
| Status ON (screen normal) | Awake is ON — sleep is set to Never (plugged in and on battery). The screen can still turn off. |
| Countdown (hours + minutes) | Turns off in {h} h {m} min |
| Countdown (minutes only) | Turns off in {m} min |
| Yellow note (while ON) | You can close ERX Tools and Awake will stay on. Open ERX Tools later to turn Awake off and restore your original settings. |
| Drawer toggle | Options |
| Checkbox | Let the screen turn off normally |
| Checkbox helper | By default Awake also keeps the screen on, because some runs (for example image processing) need it. Tick this if you only want to stop the PC from sleeping. |
| Select label | Turn Awake off automatically |
| Select option (default) | Never (stay on until I turn it off) |
| Select option | After 1 hour |
| Select option | After 2 hours |
| Select option | After 4 hours |
| Select helper | The timer only runs while ERX Tools is open. |
| Info note 1 | How it works: Awake temporarily sets the Windows sleep and screen timeouts to Never and puts your original values back when you turn it off. |
| Info note 2 | Awake only blocks automatic sleep and screen-off. Clicking Sleep or shutting down still works, and closing a laptop lid follows your Windows lid setting. |
| Banner dismiss button | Dismiss |
| Banner: resumed session | Awake was left on from an earlier session. |
| Banner: turned off cleanly | Awake is off. {n} setting(s) restored. — plus, if any: {n} change(s) you made yourself was/were kept. |
| Banner: power plan vanished (disable path) | The Windows power plan Awake was using no longer exists. Nothing needed restoring. |
| Close dialog title | Awake is still on |
| Close dialog text | Awake is keeping this PC awake. What should happen? |
| Close dialog timer note | Note: the timer only runs while ERX Tools is open. |
| Close dialog button 1 | Restore settings and close |
| Close dialog button 2 | Keep Awake on and close |
| Close dialog button 3 | Cancel |

### Awake — backend-generated strings (awake/engine.rs, commands.rs)

| Situation | Text |
|---|---|
| A setting was changed manually in Windows (auto-off banner + notification) | Awake was switched off because the sleep or screen settings were changed manually in Windows. Your changes were kept. |
| The timer ran out while the app was open | Awake timer finished. Your settings were restored. |
| The timer ran out while the app was closed | The Awake timer finished while ERX Tools was closed. Your settings were restored. |
| The power plan no longer exists (auto-off) | Awake was switched off because the Windows power plan it changed no longer exists. Your other settings are untouched. |
| The state file was unreadable and was reset | Awake's state file was damaged and has been reset. Your power settings were not touched by Awake, but if the sleep settings look wrong, set them again in Windows. (Notification variant: …Your power settings were not touched by Awake.) |
| A restore step failed (appended to the messages above) | Some settings could not be restored automatically; turning Awake off again will retry. |
| Enable: already on | Awake is already on. |
| Enable: nothing readable | Awake could not read any of the sleep or hibernate timeouts, so it cannot keep this PC awake. Nothing was changed. |
| Enable failed, rolled back | Awake could not be turned on: {reason}. Your original power settings were restored. — plus, if the rollback itself failed: However, these keys could not be rolled back automatically: {list}. Please check your power settings. |
| Options: not on | Awake is not on, so there are no options to change. |
| Read/write failure (example shape) | Awake could not change sleep after (plugged in) (Windows error code 5, access denied). |

## Desktop Icons (DesktopIconsSection.tsx)

| Element | Text |
|---|---|
| Card title | Desktop Icons |
| Card subtitle | Show or hide the standard desktop icons. |
| Info note 1 (top, above the button) | Changes apply to your Windows account only. If the desktop does not update by itself, click an empty spot on the desktop and press F5. |
| Button (moved above the switches in Prompt 13; subtle blue outline) | Open Windows desktop icon settings |
| Button error | Couldn't open the Windows settings window: {message} |
| Warning note (yellow, directly under the button) | When you hide an icon, Windows may move your other desktop icons into the free spot. Windows does the same in its own settings window. |
| Loading text | Checking the desktop icons… |
| Row names (from Rust, desktop_icons/core.rs) | This PC · Recycle Bin · Control Panel · Network · User's Files (your user folder) |
| Row error | Couldn't change this icon: {message} |
| Gray note (directly under the switches; replaces the old combined note) | ERX Tools never moves or saves your icon positions. If your desktop layout changes, it is caused by Windows itself, not by ERX Tools. |
| Gray note (last) | This changes the same settings as Windows Settings → Personalization → Themes → Desktop icon settings. |
| Read-back mismatch (Rust) | The setting was written but reads back as shown/hidden. Please try again. |
| Written but unreadable (Rust) | The setting was written but could not be read back. |
| Settings-open failure (Rust) | Couldn't open the Windows desktop icon settings. ({OS error}) |

## Organize (OrganizeSection.tsx + organize/*.rs) — RESTRUCTURED in Prompt 5h

| Element | Text | Status |
|---|---|---|
| Card title | Organize | unchanged |
| Card subtitle | Tidy loose files into folders. Nothing is deleted. | unchanged |
| Tab segments (CHANGED from three targets) | Desktop · Folder | CHANGED |
| Desktop tab description (NEW) | Moves the loose files on your Desktop into three folders: Apps (programs and shortcuts), Documents (Word, PDF, Excel and similar) and More (everything else). Windows icons such as This PC and Recycle Bin are not touched. | NEW |
| Folder: no folder chosen | No folder chosen yet. | unchanged |
| Folder: picker button | Choose folder… | unchanged |
| Folder: quick button (NEW) | Use Downloads folder | NEW |
| Sort-by label (NEW) | Sort by | NEW |
| Sort-by segments (NEW) | Format · Date · Size | NEW |
| Mode sentence: Format (NEW) | Groups files into folders such as Documents, Images, Videos, Music and Archives. | NEW |
| Mode sentence: Date (NEW) | Groups files into folders by year (or by year and month). Files without a date go into a folder named “Date not found”. | NEW |
| Mode sentence: Size (NEW) | Groups files into folders by size: Under 1 MB, 1-10 MB, 10-100 MB, 100 MB-1 GB and Over 1 GB. | NEW |
| Date option label (NEW) | Which date? | NEW |
| Date option values (NEW) | Photo date taken · Date created · Date modified | NEW |
| Photo-taken note (NEW) | Read from the photo itself (camera data). Files that are not photos, or photos without this data, go to “Date not found”. | NEW |
| Created/modified note (NEW) | These dates come from Windows and can change when files are copied or edited. | NEW |
| Granularity label (NEW) | Folders by | NEW |
| Granularity values (NEW) | Year · Year and month | NEW |
| Subfolder note (NEW, always visible in Folder tab) | Only files directly inside this folder are organized. Existing sub-folders are left untouched. | NEW |
| Protected-types note (NEW, Folder tab, Date/Size, non-Downloads) | Program files (such as .exe and .dll) and shortcuts are left in place in this folder. | NEW |
| Scan button | Scan | unchanged |
| Scan progress (NEW) | Scanning… {n} files checked | NEW |
| Preview title (CHANGED: folder name) | Ready to organize: {Desktop / <folder name>} | CHANGED |
| Preview: nothing to do | There is nothing to move — every loose file here is skipped or left in place. | unchanged |
| Preview: what will happen (CHANGED) | ERX Tools will move {n} loose file(s) into {m} folder(s) — Format: `:`; Date: `named by year:` / `named by year and month:`; Size: `by size:` | CHANGED |
| Category row | {name} — {n} file(s) | unchanged |
| File list overflow | and {n} more… | unchanged |
| Safety note (CHANGED example name) | Nothing is deleted or overwritten. Folders are never touched. If a name already exists, the file is renamed like "report (2).pdf". (Date mode: "photo (2).jpg".) | CHANGED |
| Skipped summary row | {n} file(s) will be skipped (see why) | unchanged |
| Left-in-place note (CHANGED wording) | {n} file(s) stay(s) where it is/they are (programs and unknown types are not moved here). | CHANGED |
| OneDrive note (now any chosen folder) | This folder is stored in OneDrive. ERX Tools can organize it and OneDrive will sync the changes. Online-only files are skipped. ERX Tools never changes any OneDrive settings. | unchanged scope |
| Public Desktop note (Desktop tab only) | Icons from the shared Public Desktop folder are not part of your Desktop folder and are left alone. | unchanged |
| Undo promise | You can undo this afterwards. | unchanged |
| Run buttons | Organize now · Cancel | unchanged |
| Progress | Organizing… {done} of {total} files | unchanged |
| Report title | Done: {n} file(s) moved, {m} file(s) skipped. | unchanged |
| Report note | Nothing was deleted. You can undo this run below. | unchanged |
| Undo buttons (CHANGED condition) | Undo last organize · Undo last organize in {folder name} | unchanged wording |
| Undo note | Only the most recent run for this target can be undone. | unchanged |
| Undo result | Undo finished: {summary} (+ any notes/failures appended) | unchanged |
| Folder tab: Desktop refused (NEW, Rust) | This is your Desktop. Use the Desktop tab for it. | NEW |
| Folder tab: Downloads quick path (Rust, no UI text) | resolves via the known-folder API | NEW |

### Organize — skip/failure reasons (Rust, shown after file names) — unchanged
| Reason | Text |
|---|---|
| Hidden or system attribute / desktop.ini | hidden or system file |
| Office lock file | temporary Office lock file |
| Symlink or reparse point | a link or reparse point |
| Partial download | still downloading |
| OneDrive cloud-only | online-only (OneDrive) |
| Gone between preview and run | no longer there |
| Size/time mismatch | changed since preview |
| Category name taken by a file | name conflict |
| Locked or access denied | in use or protected |
| All 999 names taken | no free name found |
| Folder creation failed | could not create the folder: {error} |
| Move failed otherwise | failed (Windows error code {n}) |
| Undo: destination gone | not found (in the undo summary "{n} not found") |
| Undo summary shape | {n} file(s) restored, {n} not found, {n} failed, {n} folder(s) kept (not empty) |

## Quick Launch (QuickLaunchSection.tsx + quick_launch/*.rs)

| Element | Text |
|---|---|
| Card title | Quick Launch |
| Card subtitle | Open common Windows tools with one click. |
| Buttons | Control Panel · Defragment and Optimize Drives · Command Prompt · PowerShell · Device Manager · Services · Microsoft PC Manager |
| PC Manager when missing | Microsoft PC Manager (not installed) |
| Secondary button (cmd, PowerShell) | Run as administrator |
| PowerShell caption | Will start: {PowerShell 7 / Windows PowerShell} |
| Success status | {label} opened. |
| UAC dismissed | Cancelled. |
| PC Manager explanation (click when missing) | Microsoft PC Manager is a Microsoft app that is not included with Windows. You can get it from the Microsoft Store. |
| Store button | Open Microsoft Store page |
| Store failed | Couldn't open the Microsoft Store. You can search for 'Microsoft PC Manager' there yourself. It may also not be available in your region or on managed PCs. |
| Info note | Opening these tools does not need administrator rights. Some tools ask for permission themselves when they start, for example Defragment and Optimize Drives. |
| Launch failure (Rust) | Windows could not start it (error code {n}). |
| Missing in System32 (Rust) | '{name}' was not found in System32. |
| PC Manager not installed (Rust guard) | Microsoft PC Manager is not installed. |

## Command Tips (CommandTipsSection.tsx + commandTips.ts)

| Element | Text |
|---|---|
| Card title | Command Tips |
| Card subtitle | Handy Windows commands. Copy one, then paste it into PowerShell or Command Prompt (the buttons above open them). |
| Badges | {PowerShell / Command Prompt / Either} · Needs administrator · Can take a long time · Uses the internet |
| Copy button | Copy → Copied (2 s) |
| Copy failed | Couldn't copy. Select the text and press Ctrl+C. |
| More button (per code box, only when it has more than 6 lines) | More → Less; collapsed shows the first 6 lines with a small fade at the bottom. Copy always copies the full code, collapsed or not. |
| Placeholder note (per step, under the code box) | Replace the green text with {replaceWith} — the words "green text" are themselves shown in green |
| Drawer toggle (closed by default) | More commands (9) |
| Closing note | These commands are only text. ERX Tools never runs them for you. |

There are NO category headings anymore. Placeholders are realistic sample
text shown in green inside the code box (no angle brackets anywhere).
Multi-step commands show one code box per step, each with its own Copy
button and a small step label. A code box with more than 6 lines starts
collapsed (first 6 lines + fade + the More button above); the expanded/
collapsed state is per box and resets when the section re-renders.

### Main tips (COMMAND_TIPS_MAIN — 8 tips, in this order)

| Title | Explanation | Badges | Steps (exact copy text per step; green sample text in bold) |
|---|---|---|---|
| List app updates | Shows which installed apps have updates. It only lists them; nothing is installed. Uses the internet (winget does that; ERX Tools does not). Not every app appears. | Either · Uses the internet | `winget upgrade` |
| Update one app | Updates ONE app. First run "List app updates" and copy the app's Id from the Id column. Update apps one at a time and only the ones you really need: updating a tool such as Python can break projects that depend on the old version. Windows may ask for permission (UAC). Uses the internet. | Either · Uses the internet | `winget upgrade --id ` + **Publisher.AppName** + ` --exact` — note: Replace the green text with the app's Id: copy it from the Id column of the "List app updates" result. If the Id looks cut off, make the window wider and run the list command again. |
| List installed apps | Shows a table of all apps installed for your Windows account: classic desktop programs and Microsoft Store apps. Useful when you want to rebuild a PC after a Windows reinstall. It only reads information; nothing is changed. The Source column tells you if an app is a Desktop program or a Store app. Store apps show their technical package name (for example Microsoft.WindowsCalculator) and have no install date. Built-in Windows system components and portable apps (apps that were never installed) are not listed. It can take a few seconds. Use PowerShell: this does not work in Command Prompt. If the table looks cut off, make the window wider and run it again. | PowerShell | the 17-line script in the fenced block "Copy text: List installed apps" below (starts collapsed in the app: More shows all of it) |
| Save installed apps to a file | Does the same as "List installed apps" (desktop programs and Microsoft Store apps), but saves the result as InstalledPrograms.csv on your Desktop instead of showing it. Open the file with Excel or Notepad. Keep it as a backup before you reinstall Windows. If a file with the same name is already on the Desktop, it is replaced. Same limits as "List installed apps". Use PowerShell: this does not work in Command Prompt. | PowerShell | the 19-line script in the fenced block "Copy text: Save installed apps to a file" below (starts collapsed in the app: More shows all of it) |
| Show a folder tree | Lists the folders and files inside a folder. For a big folder the result can be very long. | Either | `tree "` + **C:\path\to\folder** + `" /f` — note: Replace the green text with your own folder path. Keep the quotation marks. |
| Laptop battery health report | Creates an HTML battery report named battery-report.html in your user folder. Laptops only. Use Command Prompt: the %USERPROFILE% part is not understood by PowerShell. | Command Prompt | `powercfg /batteryreport /output "%USERPROFILE%\battery-report.html"` |
| System information | Prints the Windows version, RAM, uptime and more. | Either | `systeminfo` |
| Show a saved Wi-Fi password | Shows the saved password of a Wi-Fi network on THIS PC. Private: do not share the output. In the result of Step 2, look for the line "Key Content". | Either | Step 1: list the saved networks → `netsh wlan show profiles`; Step 2: show the password → `netsh wlan show profile name="` + **NetworkName** + `" key=clear` — note: Replace the green text with the network name from Step 1. Keep the quotation marks. |

Copy text: List installed apps (COMMAND_TIPS_MAIN):

```text
$UninstallKeys = @(
    "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*",
    "HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*",
    "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*"
)

$Desktop = Get-ItemProperty $UninstallKeys -ErrorAction SilentlyContinue |
    Where-Object { $_.DisplayName } |
    Select-Object @{n='Name';e={$_.DisplayName}}, @{n='Version';e={$_.DisplayVersion}}, Publisher, InstallDate, @{n='Source';e={'Desktop'}}

$Store = Get-AppxPackage -ErrorAction SilentlyContinue |
    Where-Object { -not $_.IsFramework -and -not $_.IsResourcePackage -and -not $_.NonRemovable } |
    Select-Object Name, @{n='Version';e={"$($_.Version)"}}, @{n='Publisher';e={$_.Publisher -replace '^CN="?([^,"]+).*','$1'}}, @{n='InstallDate';e={$null}}, @{n='Source';e={'Store'}}

@($Desktop) + @($Store) |
    Sort-Object Name |
    Format-Table -AutoSize
```

Copy text: Save installed apps to a file (COMMAND_TIPS_MAIN):

```text
$UninstallKeys = @(
    "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*",
    "HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*",
    "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*"
)

$Desktop = Get-ItemProperty $UninstallKeys -ErrorAction SilentlyContinue |
    Where-Object { $_.DisplayName } |
    Select-Object @{n='Name';e={$_.DisplayName}}, @{n='Version';e={$_.DisplayVersion}}, Publisher, InstallDate, @{n='Source';e={'Desktop'}}

$Store = Get-AppxPackage -ErrorAction SilentlyContinue |
    Where-Object { -not $_.IsFramework -and -not $_.IsResourcePackage -and -not $_.NonRemovable } |
    Select-Object Name, @{n='Version';e={"$($_.Version)"}}, @{n='Publisher';e={$_.Publisher -replace '^CN="?([^,"]+).*','$1'}}, @{n='InstallDate';e={$null}}, @{n='Source';e={'Store'}}

$DesktopFolder = [Environment]::GetFolderPath("Desktop")

@($Desktop) + @($Store) |
    Sort-Object Name |
    Export-Csv (Join-Path $DesktopFolder "InstalledPrograms.csv") -NoTypeInformation -Encoding UTF8
```

### "More commands" drawer (COMMAND_TIPS_MORE — 9 tips, closed by default)

| Title | Explanation | Badges | Steps (exact copy text per step; green sample text in bold) |
|---|---|---|---|
| Graphics card details | Shows your graphics card(s): name, maker, hardware ID (PNPDeviceID) and driver version. In the PNPDeviceID, look at the VEN_xxxx code: VEN_10DE = NVIDIA, VEN_1002 = AMD, VEN_8086 = Intel. If the card name says one thing but the VEN code says another, be suspicious. You can search the DEV_xxxx code on devicehunt.com or pci-ids.ucw.cz to see the real chip name. AdapterRAM is not reliable for cards with more than 4 GB of memory. | PowerShell | `Get-CimInstance Win32_VideoController | Select-Object Name, AdapterCompatibility, PNPDeviceID, DriverVersion, AdapterRAM` |
| Laptop model and serial number (from BIOS) | Shows the maker, model and serial number stored in the BIOS (SMBIOS). Compare them with what the seller says, or enter the serial number on the brand's support site (Lenovo, HP, Dell, ASUS) to see the original specifications of that exact device. Step 3 shows the BIOS serial number, which support sites often ask for; the motherboard serial in Step 2 can be different or empty. A serial number is private: do not post it publicly. | PowerShell | Step 1: maker and model → `Get-CimInstance Win32_ComputerSystem | Select-Object Manufacturer, Model, SystemFamily, SystemSKUNumber`; Step 2: motherboard → `Get-CimInstance Win32_BaseBoard | Select-Object Manufacturer, Product, SerialNumber`; Step 3: BIOS serial number → `Get-CimInstance Win32_BIOS | Select-Object Manufacturer, SMBIOSBIOSVersion, SerialNumber` |
| RAM modules and total size | Step 1 lists every RAM module with its part number, size (Capacity, in bytes) and speed (in MHz). Step 2 adds the sizes up and prints the total in GB. If the total is lower than what the seller advertised, something is wrong. | PowerShell | Step 1: each module → `Get-CimInstance Win32_PhysicalMemory | Select-Object BankLabel, Manufacturer, PartNumber, Capacity, Speed, SerialNumber`; Step 2: total size in GB → `(Get-CimInstance Win32_PhysicalMemory | Measure-Object Capacity -Sum).Sum / 1GB` |
| SSD / hard drive model and size | Step 1 shows each physical drive: model, type (SSD or HDD), connection (NVMe, SATA, USB) and size in bytes. Step 2 shows the disks as Windows sees them. Divide Size by 1,000,000,000 to get the GB written on the box (1 TB is about 1,000,000,000,000 bytes). Windows only shows what the drive reports: a fake drive can report a false size and then corrupt files once its real space is full. If you see "Access denied", open PowerShell as administrator and run the command again. | PowerShell | Step 1: physical drives → `Get-PhysicalDisk | Select-Object FriendlyName, Model, MediaType, BusType, Size, SerialNumber`; Step 2: disks and size → `Get-Disk | Select-Object Number, FriendlyName, Size, PartitionStyle` |
| Check a downloaded file is intact | Prints the SHA-256 hash of a file, to compare with the value on the website you downloaded it from. | PowerShell | `Get-FileHash "` + **C:\path\to\file.zip** + `"` — note: Replace the green text with your own file path. Keep the quotation marks. |
| Drives: size and health | Lists drives with size, free space and health. | PowerShell | `Get-Volume` |
| Top 10 CPU-hungry processes | Shows the ten processes that used the most CPU time. | PowerShell | `Get-Process | Sort-Object CPU -Descending | Select-Object -First 10` |
| Repair Windows system files | Run Step 1, let it finish, then run Step 2. Needs an administrator window and can take a long time. | Either · Needs administrator · Can take a long time | Step 1: check system files → `sfc /scannow`; Step 2: repair the Windows image → `DISM /Online /Cleanup-Image /RestoreHealth` |
| Flush the DNS cache | Can help with internet or website problems. Needs an administrator window on this PC (verified on Windows 11). | Either · Needs administrator | `ipconfig /flushdns` |
