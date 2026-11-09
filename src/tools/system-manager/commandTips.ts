/** The static Command Tips data (spec 5f, redesigned in Prompt 13): copy-only
 * commands the user runs himself — ERX Tools never executes them. Every
 * command was verified on the owner's machine (Windows 11 Pro, build 26300)
 * on 2026-11-06, except the "Update one app" entry: it is a new wording of
 * the verified `winget upgrade` family and was NOT executed here. The two
 * badge decisions that reality overruled are recorded in
 * docs/PROMPT_5_PROGRESS.md: flushdns NEEDS elevation on this build, and
 * %USERPROFILE% only expands in Command Prompt. `winget upgrade --all` was
 * removed on purpose: it can silently update tools such as Python and break
 * projects that depend on the old version. */

export type TipShell = "PowerShell" | "Command Prompt" | "Either";

/** A piece of a command line: plain text, or sample text the user must replace (shown in green). */
export type CommandPart = string | { placeholder: string };

export interface CommandStep {
  /** Small label above the code box, only for multi-step tips. */
  label?: string;
  parts: CommandPart[];
  /** If set, a note "Replace the green text with {replaceWith}" is shown under the code box. Must end with a period. */
  replaceWith?: string;
}

export interface CommandTip {
  id: string;
  title: string;
  explanation: string;
  steps: CommandStep[];
  shell: TipShell;
  needsAdministrator: boolean;
  canTakeLong: boolean;
  usesInternet: boolean;
}

/** The exact text that is shown in the code box AND copied for one step. */
export function stepText(step: CommandStep): string {
  return step.parts.map((part) => (typeof part === "string" ? part : part.placeholder)).join("");
}

export const COMMAND_TIPS_MAIN: CommandTip[] = [
  {
    id: "winget-upgrade-list",
    title: "List app updates",
    explanation:
      "Shows which installed apps have updates. It only lists them; nothing is installed. Uses the internet (winget does that; ERX Tools does not). Not every app appears.",
    steps: [{ parts: ["winget upgrade"] }],
    shell: "Either",
    needsAdministrator: false,
    canTakeLong: false,
    usesInternet: true,
  },
  {
    id: "winget-upgrade-one",
    title: "Update one app",
    explanation:
      "Updates ONE app. First run \"List app updates\" and copy the app's Id from the Id column. Update apps one at a time and only the ones you really need: updating a tool such as Python can break projects that depend on the old version. Windows may ask for permission (UAC). Uses the internet.",
    steps: [
      {
        parts: ["winget upgrade --id ", { placeholder: "Publisher.AppName" }, " --exact"],
        replaceWith:
          "the app's Id: copy it from the Id column of the \"List app updates\" result. If the Id looks cut off, make the window wider and run the list command again.",
      },
    ],
    shell: "Either",
    needsAdministrator: false,
    canTakeLong: false,
    usesInternet: true,
  },
  {
    id: "folder-tree",
    title: "Show a folder tree",
    explanation: "Lists the folders and files inside a folder. For a big folder the result can be very long.",
    steps: [
      {
        parts: ['tree "', { placeholder: "C:\\path\\to\\folder" }, '" /f'],
        replaceWith: "your own folder path. Keep the quotation marks.",
      },
    ],
    shell: "Either",
    needsAdministrator: false,
    canTakeLong: false,
    usesInternet: false,
  },
  {
    id: "battery-report",
    title: "Laptop battery health report",
    explanation:
      "Creates an HTML battery report named battery-report.html in your user folder. Laptops only. Use Command Prompt: the %USERPROFILE% part is not understood by PowerShell.",
    steps: [{ parts: ['powercfg /batteryreport /output "%USERPROFILE%\\battery-report.html"'] }],
    shell: "Command Prompt",
    needsAdministrator: false,
    canTakeLong: false,
    usesInternet: false,
  },
  {
    id: "system-info",
    title: "System information",
    explanation: "Prints the Windows version, RAM, uptime and more.",
    steps: [{ parts: ["systeminfo"] }],
    shell: "Either",
    needsAdministrator: false,
    canTakeLong: false,
    usesInternet: false,
  },
  {
    id: "wifi-password",
    title: "Show a saved Wi-Fi password",
    explanation:
      "Shows the saved password of a Wi-Fi network on THIS PC. Private: do not share the output. In the result of Step 2, look for the line \"Key Content\".",
    steps: [
      { label: "Step 1: list the saved networks", parts: ["netsh wlan show profiles"] },
      {
        label: "Step 2: show the password",
        parts: ['netsh wlan show profile name="', { placeholder: "NetworkName" }, '" key=clear'],
        replaceWith: "the network name from Step 1. Keep the quotation marks.",
      },
    ],
    shell: "Either",
    needsAdministrator: false,
    canTakeLong: false,
    usesInternet: false,
  },
];

export const COMMAND_TIPS_MORE: CommandTip[] = [
  {
    id: "gpu-info",
    title: "Graphics card details",
    explanation:
      "Shows your graphics card(s): name, maker, hardware ID (PNPDeviceID) and driver version. In the PNPDeviceID, look at the VEN_xxxx code: VEN_10DE = NVIDIA, VEN_1002 = AMD, VEN_8086 = Intel. If the card name says one thing but the VEN code says another, be suspicious. You can search the DEV_xxxx code on devicehunt.com or pci-ids.ucw.cz to see the real chip name. AdapterRAM is not reliable for cards with more than 4 GB of memory.",
    steps: [
      {
        parts: [
          "Get-CimInstance Win32_VideoController | Select-Object Name, AdapterCompatibility, PNPDeviceID, DriverVersion, AdapterRAM",
        ],
      },
    ],
    shell: "PowerShell",
    needsAdministrator: false,
    canTakeLong: false,
    usesInternet: false,
  },
  {
    id: "laptop-model",
    title: "Laptop model and serial number (from BIOS)",
    explanation:
      "Shows the maker, model and serial number stored in the BIOS (SMBIOS). Compare them with what the seller says, or enter the serial number on the brand's support site (Lenovo, HP, Dell, ASUS) to see the original specifications of that exact device. Step 3 shows the BIOS serial number, which support sites often ask for; the motherboard serial in Step 2 can be different or empty. A serial number is private: do not post it publicly.",
    steps: [
      {
        label: "Step 1: maker and model",
        parts: [
          "Get-CimInstance Win32_ComputerSystem | Select-Object Manufacturer, Model, SystemFamily, SystemSKUNumber",
        ],
      },
      {
        label: "Step 2: motherboard",
        parts: ["Get-CimInstance Win32_BaseBoard | Select-Object Manufacturer, Product, SerialNumber"],
      },
      {
        label: "Step 3: BIOS serial number",
        parts: ["Get-CimInstance Win32_BIOS | Select-Object Manufacturer, SMBIOSBIOSVersion, SerialNumber"],
      },
    ],
    shell: "PowerShell",
    needsAdministrator: false,
    canTakeLong: false,
    usesInternet: false,
  },
  {
    id: "ram-modules",
    title: "RAM modules and total size",
    explanation:
      "Step 1 lists every RAM module with its part number, size (Capacity, in bytes) and speed (in MHz). Step 2 adds the sizes up and prints the total in GB. If the total is lower than what the seller advertised, something is wrong.",
    steps: [
      {
        label: "Step 1: each module",
        parts: [
          "Get-CimInstance Win32_PhysicalMemory | Select-Object BankLabel, Manufacturer, PartNumber, Capacity, Speed, SerialNumber",
        ],
      },
      {
        label: "Step 2: total size in GB",
        parts: ["(Get-CimInstance Win32_PhysicalMemory | Measure-Object Capacity -Sum).Sum / 1GB"],
      },
    ],
    shell: "PowerShell",
    needsAdministrator: false,
    canTakeLong: false,
    usesInternet: false,
  },
  {
    id: "disk-info",
    title: "SSD / hard drive model and size",
    explanation:
      "Step 1 shows each physical drive: model, type (SSD or HDD), connection (NVMe, SATA, USB) and size in bytes. Step 2 shows the disks as Windows sees them. Divide Size by 1,000,000,000 to get the GB written on the box (1 TB is about 1,000,000,000,000 bytes). Windows only shows what the drive reports: a fake drive can report a false size and then corrupt files once its real space is full. If you see \"Access denied\", open PowerShell as administrator and run the command again.",
    steps: [
      {
        label: "Step 1: physical drives",
        parts: ["Get-PhysicalDisk | Select-Object FriendlyName, Model, MediaType, BusType, Size, SerialNumber"],
      },
      {
        label: "Step 2: disks and size",
        parts: ["Get-Disk | Select-Object Number, FriendlyName, Size, PartitionStyle"],
      },
    ],
    shell: "PowerShell",
    needsAdministrator: false,
    canTakeLong: false,
    usesInternet: false,
  },
  {
    id: "file-hash",
    title: "Check a downloaded file is intact",
    explanation:
      "Prints the SHA-256 hash of a file, to compare with the value on the website you downloaded it from.",
    steps: [
      {
        parts: ['Get-FileHash "', { placeholder: "C:\\path\\to\\file.zip" }, '"'],
        replaceWith: "your own file path. Keep the quotation marks.",
      },
    ],
    shell: "PowerShell",
    needsAdministrator: false,
    canTakeLong: false,
    usesInternet: false,
  },
  {
    id: "drive-health",
    title: "Drives: size and health",
    explanation: "Lists drives with size, free space and health.",
    steps: [{ parts: ["Get-Volume"] }],
    shell: "PowerShell",
    needsAdministrator: false,
    canTakeLong: false,
    usesInternet: false,
  },
  {
    id: "top-processes",
    title: "Top 10 CPU-hungry processes",
    explanation: "Shows the ten processes that used the most CPU time.",
    steps: [{ parts: ["Get-Process | Sort-Object CPU -Descending | Select-Object -First 10"] }],
    shell: "PowerShell",
    needsAdministrator: false,
    canTakeLong: false,
    usesInternet: false,
  },
  {
    id: "repair-system-files",
    title: "Repair Windows system files",
    explanation:
      "Run Step 1, let it finish, then run Step 2. Needs an administrator window and can take a long time.",
    steps: [
      { label: "Step 1: check system files", parts: ["sfc /scannow"] },
      { label: "Step 2: repair the Windows image", parts: ["DISM /Online /Cleanup-Image /RestoreHealth"] },
    ],
    shell: "Either",
    needsAdministrator: true,
    canTakeLong: true,
    usesInternet: false,
  },
  {
    id: "flush-dns",
    title: "Flush the DNS cache",
    explanation:
      "Can help with internet or website problems. Needs an administrator window on this PC (verified on Windows 11).",
    steps: [{ parts: ["ipconfig /flushdns"] }],
    shell: "Either",
    needsAdministrator: true,
    canTakeLong: false,
    usesInternet: false,
  },
];
