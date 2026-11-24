/** Turns whatever a rejected invoke() produced into a NotesApiError.
 * Never calls String() on the value: an unknown error is not user text. */
import type { NotesApiError } from "./types";

export function toApiError(error: unknown): NotesApiError {
  if (typeof error === "object" && error !== null) {
    const candidate = error as { code?: unknown; message?: unknown };
    if (
      typeof candidate.code === "string" &&
      typeof candidate.message === "string"
    ) {
      return { code: candidate.code, message: candidate.message };
    }
  }
  return { code: "unknown", message: "Something went wrong." };
}

/** True when the error carries the given Rust error code. */
export function isCode(error: unknown, code: string): boolean {
  return toApiError(error).code === code;
}
