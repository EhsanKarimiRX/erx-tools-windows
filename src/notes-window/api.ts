/** One typed wrapper per Secure Notes command. Every wrapper rejects with a
 * NotesApiError and never logs or stores its arguments. Argument names are the
 * camelCase forms of the Rust parameter names (Tauri converts them). */
import { invoke } from "@tauri-apps/api/core";
import { toApiError } from "./errors";
import type {
  ChangePassphraseResult,
  LockReport,
  NoteListItem,
  NotesStatus,
  NoteView,
  PreviewMode,
  RefreshView,
  SaveState,
  SettingsView,
  UnlockOutcome,
  WordsView,
} from "./types";

export async function notesStatus(): Promise<NotesStatus> {
  try {
    return await invoke<NotesStatus>("notes_status");
  } catch (error) {
    throw toApiError(error);
  }
}

export async function generateWords(count: number): Promise<WordsView> {
  try {
    return await invoke<WordsView>("notes_generate_words", { count });
  } catch (error) {
    throw toApiError(error);
  }
}

export async function refreshWord(slot: number): Promise<RefreshView> {
  try {
    return await invoke<RefreshView>("notes_refresh_word", { slot });
  } catch (error) {
    throw toApiError(error);
  }
}

export async function confirmWordsSaved(): Promise<void> {
  try {
    return await invoke<void>("notes_confirm_words_saved");
  } catch (error) {
    throw toApiError(error);
  }
}

export async function retypeWords(typed: string): Promise<boolean> {
  try {
    return await invoke<boolean>("notes_retype_words", { typed });
  } catch (error) {
    throw toApiError(error);
  }
}

export async function clearWords(): Promise<void> {
  try {
    return await invoke<void>("notes_clear_words");
  } catch (error) {
    throw toApiError(error);
  }
}

export async function finishCreate(): Promise<void> {
  try {
    return await invoke<void>("notes_finish_create");
  } catch (error) {
    throw toApiError(error);
  }
}

export async function unlockNotes(typed: string): Promise<UnlockOutcome> {
  try {
    return await invoke<UnlockOutcome>("notes_unlock", { typed });
  } catch (error) {
    throw toApiError(error);
  }
}

export async function lockNotes(force: boolean): Promise<LockReport> {
  try {
    return await invoke<LockReport>("notes_lock", { force });
  } catch (error) {
    throw toApiError(error);
  }
}

export async function notesActivity(): Promise<void> {
  try {
    return await invoke<void>("notes_activity");
  } catch (error) {
    throw toApiError(error);
  }
}

export async function saveState(): Promise<SaveState> {
  try {
    return await invoke<SaveState>("notes_save_state");
  } catch (error) {
    throw toApiError(error);
  }
}

export async function saveNow(): Promise<void> {
  try {
    return await invoke<void>("notes_save_now");
  } catch (error) {
    throw toApiError(error);
  }
}

export async function getSettings(): Promise<SettingsView> {
  try {
    return await invoke<SettingsView>("notes_get_settings");
  } catch (error) {
    throw toApiError(error);
  }
}

export async function setSettings(
  preview: PreviewMode,
  autoLockMinutes: number,
): Promise<void> {
  try {
    return await invoke<void>("notes_set_settings", { preview, autoLockMinutes });
  } catch (error) {
    throw toApiError(error);
  }
}

export async function listNotes(query: string): Promise<NoteListItem[]> {
  try {
    return await invoke<NoteListItem[]>("notes_list", { query });
  } catch (error) {
    throw toApiError(error);
  }
}

export async function getNote(id: string): Promise<NoteView> {
  try {
    return await invoke<NoteView>("notes_get_note", { id });
  } catch (error) {
    throw toApiError(error);
  }
}

export async function createNote(
  title: string,
  body: string,
): Promise<NoteView> {
  try {
    return await invoke<NoteView>("notes_create_note", { title, body });
  } catch (error) {
    throw toApiError(error);
  }
}

export async function updateNote(
  id: string,
  title: string,
  body: string,
): Promise<void> {
  try {
    return await invoke<void>("notes_update_note", { id, title, body });
  } catch (error) {
    throw toApiError(error);
  }
}

export async function deleteNote(id: string): Promise<void> {
  try {
    return await invoke<void>("notes_delete_note", { id });
  } catch (error) {
    throw toApiError(error);
  }
}

export async function changePassphrase(
  currentTyped: string,
): Promise<ChangePassphraseResult> {
  try {
    return await invoke<ChangePassphraseResult>("notes_change_passphrase", {
      currentTyped,
    });
  } catch (error) {
    throw toApiError(error);
  }
}

export async function exportNote(
  id: string,
  format: "txt" | "md" | "erxnote",
): Promise<string | null> {
  try {
    return await invoke<string | null>("notes_export_note", { id, format });
  } catch (error) {
    throw toApiError(error);
  }
}

export async function exportAll(): Promise<string | null> {
  try {
    return await invoke<string | null>("notes_export_all");
  } catch (error) {
    throw toApiError(error);
  }
}

export async function importNotes(): Promise<number | null> {
  try {
    return await invoke<number | null>("notes_import");
  } catch (error) {
    throw toApiError(error);
  }
}

export async function copyText(text: string): Promise<void> {
  try {
    return await invoke<void>("notes_copy_text", { text });
  } catch (error) {
    throw toApiError(error);
  }
}

export async function copyWords(): Promise<void> {
  try {
    return await invoke<void>("notes_copy_words");
  } catch (error) {
    throw toApiError(error);
  }
}

export async function closeWindow(): Promise<void> {
  try {
    return await invoke<void>("notes_close_window");
  } catch (error) {
    throw toApiError(error);
  }
}
