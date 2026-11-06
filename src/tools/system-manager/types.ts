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
  | "timer_expired_while_closed";

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
