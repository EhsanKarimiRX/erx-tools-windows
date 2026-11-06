/** The static Command Tips data (spec 5f): copy-only commands the user
 * runs himself — ERX Tools never executes them. Every command was verified
 * on the owner's machine (Windows 11 Pro, build 26300) on 2026-11-06; the
 * two badge decisions that reality overruled are recorded in
 * docs/PROMPT_5_PROGRESS.md: flushdns NEEDS elevation on this build, and
 * %USERPROFILE% only expands in Command Prompt. */

export type TipShell = "PowerShell" | "Command Prompt" | "Either";

export interface CommandTip {
  id: string;
  title: string;
  explanation: string;
  /** One line per command; copied together in order. */
  commands: string[];
  shell: TipShell;
  needsAdministrator: boolean;
  canTakeLong: boolean;
  usesInternet: boolean;
  /** The command contains a <placeholder> the user must replace. */
  hasPlaceholder: boolean;
}

export interface CommandTipGroup {
  heading: string;
  tips: CommandTip[];
}

export const COMMAND_TIP_GROUPS: readonly CommandTipGroup[] = [
  {
    heading: "Apps",
    tips: [
      {
        id: "winget-upgrade-list",
        title: "List app updates",
        explanation:
          "Shows which installed apps have updates. Uses the internet (winget does that; ERX Tools does not). Not every app appears.",
        commands: ["winget upgrade"],
        shell: "Either",
        needsAdministrator: false,
        canTakeLong: false,
        usesInternet: true,
        hasPlaceholder: false,
      },
      {
        id: "winget-upgrade-all",
        title: "Update all apps",
        explanation:
          "Slow, and it may show several permission (UAC) prompts while it works. Uses the internet.",
        commands: ["winget upgrade --all"],
        shell: "Either",
        needsAdministrator: false,
        canTakeLong: true,
        usesInternet: true,
        hasPlaceholder: false,
      },
    ],
  },
  {
    heading: "Files",
    tips: [
      {
        id: "folder-tree",
        title: "Show a folder tree",
        explanation: "Lists folders and files under a folder.",
        commands: ['tree "<folder path>" /f'],
        shell: "Either",
        needsAdministrator: false,
        canTakeLong: false,
        usesInternet: false,
        hasPlaceholder: true,
      },
      {
        id: "file-hash",
        title: "Check a downloaded file is intact",
        explanation:
          "Prints the SHA-256 hash to compare with the value on the website you downloaded it from.",
        commands: ['Get-FileHash "<file path>"'],
        shell: "PowerShell",
        needsAdministrator: false,
        canTakeLong: false,
        usesInternet: false,
        hasPlaceholder: true,
      },
    ],
  },
  {
    heading: "Repair",
    tips: [
      {
        id: "repair-system-files",
        title: "Repair Windows system files",
        explanation:
          "Run the first command, let it finish, then the second. Needs administrator and can take a long time.",
        commands: ["sfc /scannow", "DISM /Online /Cleanup-Image /RestoreHealth"],
        shell: "Either",
        needsAdministrator: true,
        canTakeLong: true,
        usesInternet: false,
        hasPlaceholder: false,
      },
    ],
  },
  {
    heading: "Network",
    tips: [
      {
        id: "flush-dns",
        title: "Flush the DNS cache",
        explanation:
          "Can help with internet or website problems. Needs an administrator window on this PC (verified on Windows 11).",
        commands: ["ipconfig /flushdns"],
        shell: "Either",
        needsAdministrator: true,
        canTakeLong: false,
        usesInternet: false,
        hasPlaceholder: false,
      },
      {
        id: "wifi-password",
        title: "Show a saved Wi-Fi password",
        explanation:
          "Shows the saved password of a network on THIS PC. Private — do not share the output. Run the first command to see the network names.",
        commands: [
          "netsh wlan show profiles",
          'netsh wlan show profile name="<network name>" key=clear',
        ],
        shell: "Either",
        needsAdministrator: false,
        canTakeLong: false,
        usesInternet: false,
        hasPlaceholder: true,
      },
    ],
  },
  {
    heading: "System info",
    tips: [
      {
        id: "battery-report",
        title: "Laptop battery health report",
        explanation:
          "Creates an HTML battery report in your user folder. Laptops only. The %USERPROFILE% part only works in Command Prompt (verified).",
        commands: ['powercfg /batteryreport /output "%USERPROFILE%\\battery-report.html"'],
        shell: "Command Prompt",
        needsAdministrator: false,
        canTakeLong: false,
        usesInternet: false,
        hasPlaceholder: false,
      },
      {
        id: "system-info",
        title: "System information",
        explanation: "Prints the Windows version, RAM, uptime and more.",
        commands: ["systeminfo"],
        shell: "Either",
        needsAdministrator: false,
        canTakeLong: false,
        usesInternet: false,
        hasPlaceholder: false,
      },
      {
        id: "drive-health",
        title: "Drives: size and health",
        explanation: "Lists drives with size, free space and health.",
        commands: ["Get-Volume"],
        shell: "PowerShell",
        needsAdministrator: false,
        canTakeLong: false,
        usesInternet: false,
        hasPlaceholder: false,
      },
      {
        id: "top-processes",
        title: "Top 10 CPU-hungry processes",
        explanation: "Shows the ten processes that used the most CPU time.",
        commands: ["Get-Process | Sort-Object CPU -Descending | Select-Object -First 10"],
        shell: "PowerShell",
        needsAdministrator: false,
        canTakeLong: false,
        usesInternet: false,
        hasPlaceholder: false,
      },
    ],
  },
];
