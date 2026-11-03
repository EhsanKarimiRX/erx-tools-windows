/**
 * TypeScript mirror of the Folder Icon settings, whose single source of
 * truth is `src-tauri/src/tools/folder_icon/settings.rs`. This file holds
 * the same wire shape and constants; Rust validates everything and never
 * trusts these values.
 */

/** Pattern drawn on the front panel of the folder. */
export type PatternKind = "none" | "vertical" | "horizontal" | "checker";

/** Raw settings as the UI holds and sends them (Rust validates). */
export interface IconSettingsInput {
  /** Front panel color, `#RRGGBB`. */
  baseColor: string;
  pattern: PatternKind;
  /** Pattern color, `#RRGGBB` (used only when pattern is not "none"). */
  patternColor: string;
  /** Pattern size step, 1 (thin) to 5 (thick). */
  patternSize: number;
  /** Pattern opacity in percent, 10 to 100. */
  patternOpacity: number;
}

export const PATTERN_SIZE_MIN = 1;
export const PATTERN_SIZE_MAX = 5;
export const PATTERN_OPACITY_MIN = 10;
export const PATTERN_OPACITY_MAX = 100;

export const PATTERN_KINDS: readonly PatternKind[] = [
  "none",
  "vertical",
  "horizontal",
  "checker",
];

/** Strict hex check, mirroring the Rust parser: `#RRGGBB` or short `#RGB`. */
export function isHexColor(value: string): boolean {
  return /^#(?:[0-9a-fA-F]{3}|[0-9a-fA-F]{6})$/.test(value.trim());
}

/** Expands `#RGB` to `#RRGGBB`; returns null for anything invalid. */
export function expandHex(value: string): string | null {
  const text = value.trim();
  if (/^#[0-9a-fA-F]{6}$/.test(text)) return text.toLowerCase();
  if (/^#[0-9a-fA-F]{3}$/.test(text)) {
    const [r, g, b] = text.slice(1);
    return `#${r}${r}${g}${g}${b}${b}`.toLowerCase();
  }
  return null;
}

/** The 12 preset colors of the color row (spec 4.2). */
export const PRESET_COLORS: readonly { hex: string; name: string }[] = [
  { hex: "#0078D4", name: "Blue" },
  { hex: "#4CC2FF", name: "Light blue" },
  { hex: "#107C10", name: "Green" },
  { hex: "#F7C600", name: "Yellow" },
  { hex: "#F7630C", name: "Orange" },
  { hex: "#E81123", name: "Red" },
  { hex: "#EA005E", name: "Pink" },
  { hex: "#8E44AD", name: "Purple" },
  { hex: "#8E562E", name: "Brown" },
  { hex: "#9B9B9B", name: "Gray" },
  { hex: "#3B3B3B", name: "Dark gray" },
  { hex: "#F3F3F3", name: "White" },
];
