/** Maximum number of files in one batch (spec: default 200, tested at 100). */
export const MAX_FILES = 200;

/** Short list of supported input formats, for hints. */
export const SUPPORTED_INPUT = "JPEG, PNG, WebP, BMP, GIF, TIFF";

export type OutputFormat = "jpeg" | "png" | "webp" | "bmp" | "tiff";

export interface ConvertOptions {
  format: OutputFormat;
  /** JPEG quality, 1-100. WebP output is lossless in v1 (no quality). */
  jpegQuality: number;
  /** Background used when flattening transparency, "#rrggbb". */
  background: string;
  /** null = same folder as each original file. */
  outputFolder: string | null;
}

export type FileStatus =
  | "ready"
  | "waiting"
  | "converting"
  | "done"
  | "failed"
  | "skipped";

export interface FileEntry {
  /** Absolute path; the stable key sent back to Rust for conversion. */
  path: string;
  fileName: string;
  sizeBytes: number;
  /** Detected from content by Rust, e.g. "JPEG". */
  format: string;
  animated: boolean;
  status: FileStatus;
  note: string | null;
}

/** Mirrors the Rust `InspectedFile` struct. */
export interface InspectedFile {
  path: string;
  fileName: string;
  sizeBytes: number;
  kind: "image" | "unsupported" | "invalid";
  format: string | null;
  animated: boolean;
  reason: string | null;
}

/** Human-readable byte size, e.g. "12.4 MB". */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const kb = bytes / 1024;
  if (kb < 1024) return `${kb.toFixed(kb < 10 ? 1 : 0)} KB`;
  const mb = kb / 1024;
  if (mb < 1024) return `${mb.toFixed(mb < 10 ? 1 : 0)} MB`;
  return `${(mb / 1024).toFixed(1)} GB`;
}
