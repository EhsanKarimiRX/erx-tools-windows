/** Maximum number of files in one list (the same limit is enforced in Rust). */
export const MAX_FILES = 200;

/** Short list of supported formats, for hints. */
export const SUPPORTED_FORMATS = "JPEG, PNG, WebP, GIF, BMP, TIFF, HEIC, MP4, MOV, MKV";

/** Mirrors the Rust `Finding` struct. */
export interface Finding {
  category: string;
  label: string;
  detail: string | null;
}

export type ScanKind = "found" | "clean" | "unsupported";

/** Mirrors the Rust `ScannedFile` struct. */
export interface ScannedFile {
  path: string;
  fileName: string;
  sizeBytes: number;
  kind: ScanKind;
  format: string | null;
  findings: Finding[];
  kept: string[];
  reason: string | null;
}

export type FileStatus =
  | "ready"
  | "waiting"
  | "working"
  | "done"
  | "nothing"
  | "failed"
  | "skipped";

/** One row of the list: the scan result plus the state of the cleaning. */
export interface FileEntry extends ScannedFile {
  status: FileStatus;
  /** Bytes written so far and in total while the file is being cleaned. */
  done: number;
  total: number;
  /** What was removed (set when the file is cleaned). */
  removed: Finding[];
  pixelsVerified: boolean;
  outputPath: string | null;
  outputSize: number | null;
  sourceSize: number | null;
  note: string | null;
}

/** Mirrors the Rust `ProgressEvent` pushed through the channel. */
export interface ProgressEvent {
  index: number;
  status: "working" | "done" | "nothing" | "failed" | "skipped";
  done: number;
  total: number;
  outputPath: string | null;
  outputSize: number | null;
  sourceSize: number | null;
  removed: Finding[];
  kept: string[];
  pixelsVerified: boolean;
  reason: string | null;
}

/** Mirrors the Rust `BatchSummary` returned when the batch finishes. */
export interface BatchSummary {
  cleaned: number;
  alreadyClean: number;
  failed: number;
  cancelled: number;
}

/** Mirrors the Rust `OptionsDto`. */
export interface Options {
  keepOrientation: boolean;
  keepColorProfile: boolean;
  /** null = same folder as each original file. */
  outputFolder: string | null;
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

/** The file name part of an absolute path (Windows or POSIX separators). */
export function fileBaseName(path: string): string {
  const index = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  return index >= 0 ? path.slice(index + 1) : path;
}
