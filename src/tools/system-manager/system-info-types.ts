/** Mirror of the Rust DTOs in src-tauri/src/tools/system_manager/system_info/
 * (card.rs and core.rs). Rust decides every text; this copy only shapes the
 * wire format. */

export interface SystemCardCell {
  value: string;
  detail: string;
}

export interface SystemCardBattery {
  /** 0..100, or null when the battery does not report it. */
  healthPercent: number | null;
  detail: string;
}

export interface SystemCardMoreItem {
  name: string;
  detail: string;
}

export interface SystemCardMoreGroup {
  title: string;
  items: SystemCardMoreItem[];
}

/** null = "Not reported" (battery null = this PC has no battery). */
export interface SystemCardData {
  processor: SystemCardCell | null;
  graphics: SystemCardCell | null;
  memory: SystemCardCell | null;
  storage: SystemCardCell | null;
  os: SystemCardCell | null;
  battery: SystemCardBattery | null;
  uptime: string | null;
  more: SystemCardMoreGroup[];
}
