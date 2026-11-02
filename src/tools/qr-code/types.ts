/** QR Code Generator — types shared with the Rust commands. */

/** Wi-Fi security tokens understood by Rust. */
export type WifiSecurity = "wpa" | "wep" | "nopass";

/** Content as sent to Rust; Rust builds and validates the payload. */
export type QrContent =
  | { kind: "text"; value: string }
  | { kind: "website"; url: string }
  | {
      kind: "wifi";
      ssid: string;
      password: string;
      security: WifiSecurity;
      hidden: boolean;
    };

/** Which content tab is active. */
export type ContentTab = "text" | "website" | "wifi";

/** All generate/save options (Rust validates everything again). */
export interface QrOptions {
  content: QrContent;
  style: "sharp" | "round";
  foreground: string;
  background: string;
  transparent: boolean;
  errorCorrection: "L" | "M" | "Q" | "H";
  margin: number;
  pngSize: number;
}

/** Successful generate_qr result. */
export interface QrSvgResult {
  svg: string;
  sizeModules: number;
  totalModules: number;
  levelUsed: string;
  charCount: number;
  maxChars: number;
  /** Exact PNG side after Rust snapped it to whole pixels per module. */
  pngPixels: number;
  pngPixelsPerModule: number;
  warnings: string[];
}

/** Strict frontend hex check; Rust re-validates authoritatively. */
export function isHexColor(value: string): boolean {
  return /^#[0-9a-fA-F]{6}$/.test(value);
}

/** The website URL that would be encoded: trimmed, https:// added when the
 * user omitted the scheme (Rust applies the same rule). */
export function previewUrl(input: string): string {
  const trimmed = input.trim();
  if (!trimmed) return "";
  return /^[a-zA-Z]+:/.test(trimmed) ? trimmed : `https://${trimmed}`;
}

/** Limits mirrored from Rust validation (single source of truth for the UI). */
export const MIN_MARGIN = 1;
export const MAX_MARGIN = 10;
export const DEFAULT_MARGIN = 4;
export const MIN_PNG_SIZE = 256;
export const MAX_PNG_SIZE = 4096;
export const DEFAULT_PNG_SIZE = 1024;
