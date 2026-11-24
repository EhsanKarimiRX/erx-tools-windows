import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface OpenWindowResult {
  alreadyOpen: boolean;
}

/** Shows a message only when it is a real user-facing string. */
function errorMessage(error: unknown): string {
  if (typeof error === "object" && error !== null) {
    const candidate = error as { message?: unknown };
    if (typeof candidate.message === "string") {
      return candidate.message;
    }
  }
  return "Something went wrong.";
}

/** Secure Notes: the tool screen only opens the hardened window. Notes are
 * read, written and encrypted there, never here. */
export default function SecureNotes() {
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [opening, setOpening] = useState(false);

  async function openWindow() {
    setError(null);
    setMessage(null);
    setOpening(true);
    try {
      const result = await invoke<OpenWindowResult>("notes_open_window");
      setMessage(
        result.alreadyOpen ? "The window was already open." : "Window opened.",
      );
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setOpening(false);
    }
  }

  return (
    <div>
      <h2>Secure Notes</h2>
      <p>Encrypted notes on this PC, protected by a set of words.</p>
      <button
        type="button"
        className="button-primary"
        onClick={() => void openWindow()}
        disabled={opening}
      >
        Open Secure Notes
      </button>
      {message !== null && <p role="status">{message}</p>}
      {error !== null && (
        <p className="tool-error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
