/** Mirror of the Rust DTOs in `src-tauri/src/tools/system_manager/awake/`
 * (core.rs + commands.rs). The Rust side decides what is valid; this copy
 * only shapes the wire format. */

/** Status as reported by the backend. The toggle shows ON only when
 * `state` is "on" — it is always derived from the backend. */
export interface AwakeStatus {
  state: "off" | "on";
  startedAtUnix: number | null;
  endsAtUnix: number | null;
  /** true = the display is also set to Never. */
  keepScreenOn: boolean;
  /** true = the app found a still-valid Awake from an earlier session. */
  resumedFromPreviousSession: boolean;
}

/** Why Awake switched itself off (snake_case from the Rust enum). */
export type AutoOffReason =
  | "manual_change"
  | "scheme_gone"
  | "timer_expired"
  | "timer_expired_while_closed"
  | "state_file_damaged";

export interface PendingAutoOff {
  reason: AutoOffReason;
  message: string;
}

export interface StatusResponse {
  status: AwakeStatus;
  pendingAutoOff: PendingAutoOff | null;
}

/** Drawer options sent with enable and set_options. */
export interface AwakeOptionsDto {
  keepScreenOn: boolean;
  endsAtUnix: number | null;
}

export interface RestoreReport {
  restored: number;
  kept: number;
  /** One plain-English sentence per key that could not be restored. */
  failed: string[];
  schemeGone: boolean;
}

export interface DisableResponse {
  status: AwakeStatus;
  report: RestoreReport;
}

/** The timer choices (spec R6); hours = 0 means "never". */
export const TIMER_CHOICES: ReadonlyArray<{ label: string; hours: number }> = [
  { label: "Never (stay on until I turn it off)", hours: 0 },
  { label: "After 1 hour", hours: 1 },
  { label: "After 2 hours", hours: 2 },
  { label: "After 4 hours", hours: 4 },
];

/** One Desktop Icons row, as read back from the registry by the backend
 * (desktop_icons/commands.rs). `visible` is always the read-back state. */
export interface DesktopIconState {
  id: "this_pc" | "recycle_bin" | "control_panel" | "network" | "user_files";
  name: string;
  visible: boolean;
}

// ----- Organize (mirror of organize/commands.rs DTOs) -----

/** The two tabs (Prompt 5h). */
export type OrganizeTab = "desktop" | "folder";
/** The backend target kind (manifest granularity). */
export type OrganizeTargetId = "desktop" | "downloads" | "custom";
export type SortMode = "format" | "date" | "size";
export type DateSourceId = "photo_taken" | "created" | "modified";
export type GranularityId = "year" | "year_month";

export interface OrganizeModeInput {
  mode: SortMode;
  dateSource?: DateSourceId;
  granularity?: GranularityId;
}

export interface OrganizePlannedFile {
  name: string;
  size: number;
  modifiedUnix: number;
  /** LOCAL (year, month) for Date mode; null otherwise or undetermined. */
  dateYm: [number, number] | null;
  /** The destination folder the scan decided. */
  category: string;
}

export interface OrganizeCategory {
  name: string;
  files: OrganizePlannedFile[];
}

export interface OrganizeSkippedFile {
  name: string;
  reason: string;
}

/** The read-only scan result. Nothing has moved yet. */
export interface ScanPlan {
  target: OrganizeTargetId;
  root: string;
  onOnedrive: boolean;
  mode: SortMode;
  categories: OrganizeCategory[];
  skipped: OrganizeSkippedFile[];
  /** Files deliberately left where they are (Custom / protected types). */
  leftInPlace: number;
}

export interface RunReport {
  moved: number;
  skipped: OrganizeSkippedFile[];
}

// ----- Quick Launch (mirror of quick_launch/commands.rs DTOs) -----

export interface PcManagerInfo {
  installed: boolean;
  family: string | null;
  appId: string | null;
}

export interface QuickLaunchInfo {
  /** "PowerShell 7" or "Windows PowerShell". */
  powershellLabel: string;
  pcManager: PcManagerInfo;
}
