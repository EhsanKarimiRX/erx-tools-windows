/** Types and small helpers shared by the Quantum Resistance Vault screens.
 * Field names match the camelCase DTOs of src-tauri/src/tools/vault/commands.rs. */

export type PartSizeId = "off" | "500mb" | "1gb" | "2gb";

export interface PartSizeOption {
  id: PartSizeId;
  label: string;
}

export const PART_SIZE_OPTIONS: readonly PartSizeOption[] = [
  { id: "off", label: "Do not split (one file)" },
  { id: "500mb", label: "Parts of 500 MB" },
  { id: "1gb", label: "Parts of 1 GB" },
  { id: "2gb", label: "Parts of 2 GB" },
];

export const MIN_WORDS = 6;
export const MAX_WORDS = 12;
export const DEFAULT_WORDS = 10;

/** The clipboard is wiped this long after the passphrase was copied. */
export const CLIPBOARD_CLEAR_MS = 30_000;

export interface StrengthInfo {
  bits: number;
  groverBits: number;
  isWeak: boolean;
}

export interface PassphraseInfo {
  words: string[];
  /** Replacements left per word slot, in slot order. */
  replacementsLeft: number[];
  strength: StrengthInfo;
}

export interface RefreshResult {
  word: string;
  replacementsLeft: number;
}

export interface EncryptPlan {
  isFolder: boolean;
  itemCount: number;
  fileName: string;
  sizeBytes: number;
  outputDir: string;
  outputBasePath: string;
  conflicts: string[];
  compressByDefault: boolean;
  suggestSplit: boolean;
  freeBytes: number | null;
  requiredBytes: number;
}

export interface EncryptResult {
  cancelled: boolean;
  parts: string[];
  inputBytes: number;
  outputBytes: number;
  verified: boolean;
}

export interface DecryptPlan {
  isFolder: boolean;
  fileName: string;
  partCount: number;
  totalBytes: number;
  outputDir: string;
  outputPath: string;
  outputExists: boolean;
}

export interface DecryptResult {
  cancelled: boolean;
  outputPath: string;
  outputBytes: number;
}

export type VaultPhase = "tuning" | "deriving" | "verifying";

/** Events streamed by the Rust side while an operation runs. */
export type VaultEvent =
  | { event: "phase"; phase: VaultPhase }
  | { event: "progress"; done: number; total: number };

/** Turns anything thrown by invoke into a short display string. */
export function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

/** "1.5 GB" style sizes (binary units, as Windows shows them). */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "?";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const digits = unit === 0 || value >= 100 ? 0 : value >= 10 ? 1 : 2;
  return `${value.toFixed(digits)} ${units[unit]}`;
}

/** "2 min 5 s" style durations. */
export function formatDuration(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return "";
  const total = Math.round(seconds);
  if (total < 60) return `${total} s`;
  const minutes = Math.floor(total / 60);
  if (minutes < 60) return `${minutes} min ${total % 60} s`;
  const hours = Math.floor(minutes / 60);
  return `${hours} h ${minutes % 60} min`;
}
