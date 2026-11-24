/** Types of the Secure Notes window.
 * Field names match the camelCase DTOs of src-tauri/src/tools/notes/commands.rs
 * and session.rs. Nothing in this file is stored or logged. */

/** Whether a notes file exists and whether it is open. */
export interface NotesStatus {
  hasFile: boolean;
  unlocked: boolean;
}

/** The words of a new passphrase for display, with their strength. */
export interface WordsView {
  words: string[];
  /** Replacements left per word slot, in slot order. */
  replacementsLeft: number[];
  entropyBits: number;
  weak: boolean;
}

/** One replaced word for display. */
export interface RefreshView {
  word: string;
  replacementsLeft: number;
}

/** How an unlock ended. */
export interface UnlockOutcome {
  usedBackup: boolean;
}

/** One row of the note list. */
export interface NoteListItem {
  id: string;
  title: string;
  preview: string | null;
  created: number;
  modified: number;
}

/** One note with its text. */
export interface NoteView {
  id: string;
  title: string;
  body: string;
  created: number;
  modified: number;
}

/** How the note list shows each note (serde rename_all = "kebab-case"). */
export type PreviewMode = "titles" | "first-line";

/** The settings shown to the user. */
export interface SettingsView {
  preview: PreviewMode;
  autoLockMinutes: number;
}

/** What locking did. */
export interface LockReport {
  wasUnlocked: boolean;
  finalSaveFailed: boolean;
}

/** How the open notes compare with the file on disk (serde rename_all = "lowercase"). */
export type SaveState = "saved" | "pending" | "failed";

/** What one timer tick did. */
export interface TickOutcome {
  autosaved: boolean;
  autosaveFailed: boolean;
  locked: LockReport | null;
}

/** The payload of the autosave event. */
export interface AutosaveEvent {
  ok: boolean;
}

/** The payload of the auto-locked event. */
export interface AutoLockedEvent {
  finalSaveFailed: boolean;
}

/** What changing the passphrase did. */
export interface ChangePassphraseResult {
  oldBackupGone: boolean;
}

/** An error of a Secure Notes command, as the frontend sees it. */
export interface NotesApiError {
  code: string;
  message: string;
}

/** How long Rust keeps the copied words on the clipboard. Display only: Rust
 * does the clearing. Must match CLEAR_AFTER in
 * src-tauri/src/tools/notes/clipboard.rs. */
export const CLIPBOARD_CLEAR_SECONDS = 30;

/** Smallest and largest number of words the create screen offers. */
export const MIN_WORDS = 5;
export const MAX_WORDS = 12;
export const DEFAULT_WORDS = 8;
