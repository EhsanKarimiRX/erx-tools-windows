/** QR Code Generator — types shared with the Rust commands. */

/** Content as sent to Rust; Rust builds and validates the payload. */
export type QrContent =
  | { kind: "text"; value: string }
  | { kind: "website"; url: string }
  | {
      kind: "wifi";
      ssid: string;
      password: string;
      security: "wpa" | "wep" | "nopass";
      hidden: boolean;
    };

/** All generate/save options (Rust validates everything again). */
export interface QrOptions {
  content: QrContent;
  style: "sharp" | "round";
  foreground: string;
  background: string;
  transparent: boolean;
  errorCorrection: "L" | "M" | "Q" | "H";
  margin: number;
}

/** Successful generate_qr result. */
export interface QrSvgResult {
  svg: string;
  sizeModules: number;
  totalModules: number;
  levelUsed: string;
  charCount: number;
  maxChars: number;
  warnings: string[];
}

/** Strict frontend hex check; Rust re-validates authoritatively. */
export function isHexColor(value: string): boolean {
  return /^#[0-9a-fA-F]{6}$/.test(value);
}
