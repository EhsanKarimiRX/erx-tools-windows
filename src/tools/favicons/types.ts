/** Shapes exchanged with the Rust commands (camelCase, see commands.rs DTOs). */

export type SourceKind = "raster" | "svg";

export interface SourceInfo {
  path: string;
  fileName: string;
  kind: SourceKind;
  formatLabel: string;
  width: number;
  height: number;
  warnings: string[];
  previewDataUrl: string;
}

export interface FaviconsOptions {
  siteName: string;
  paddingPercent: number;
  appPaddingPercent: number;
  trimTransparent: boolean;
  backgroundColor: string;
  themeColor: string;
  includeMaskable: boolean;
}

export interface IconPreview {
  fileName: string;
  width: number;
  height: number;
  sizeBytes: number;
  dataUrl: string;
}

export interface PreviewResult {
  icons: IconPreview[];
  htmlSnippet: string;
  warnings: string[];
}

export interface SavedFile {
  name: string;
  sizeBytes: number;
}

export interface SaveResult {
  folder: string;
  files: SavedFile[];
  htmlSnippet: string;
  warnings: string[];
}

export const DEFAULT_OPTIONS: FaviconsOptions = {
  siteName: "",
  paddingPercent: 0,
  appPaddingPercent: 10,
  trimTransparent: true,
  backgroundColor: "#ffffff",
  themeColor: "#ffffff",
  includeMaskable: true,
};

export const SUPPORTED_FORMATS = "PNG, JPEG, WebP, BMP, GIF, TIFF, ICO, SVG";
export const PADDING_MIN = 0;
export const PADDING_MAX = 45;

const HEX_COLOR = /^#(?:[0-9a-fA-F]{3}|[0-9a-fA-F]{6})$/;

/** True for "#rgb" or "#rrggbb" (surrounding spaces allowed). */
export function isValidHex(value: string): boolean {
  return HEX_COLOR.test(value.trim());
}

/** Value for <input type="color">, which only accepts "#rrggbb". */
export function toPickerColor(value: string): string {
  const text = value.trim();
  if (!HEX_COLOR.test(text)) return "#000000";
  if (text.length === 4) {
    return `#${text[1]}${text[1]}${text[2]}${text[2]}${text[3]}${text[3]}`.toLowerCase();
  }
  return text.toLowerCase();
}

export function clampPadding(value: number): number {
  if (!Number.isFinite(value)) return PADDING_MIN;
  return Math.min(PADDING_MAX, Math.max(PADDING_MIN, Math.round(value)));
}

/** Turns anything thrown by invoke into a display string. */
export function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
