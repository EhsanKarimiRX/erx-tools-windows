// TEMPORARY (N6-5): a test button for the hardened window. N8 replaces this file with the real tool.
import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface OpenWindowResult {
  alreadyOpen: boolean;
}

function describe(error: unknown): string {
  if (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    "message" in error
  ) {
    const code = (error as { code: unknown }).code;
    const message = (error as { message: unknown }).message;
    if (typeof code === "string" && typeof message === "string") {
      return `${code}: ${message}`;
    }
  }
  return String(error);
}

export default function EncryptedNotes() {
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function openWindow() {
    setError(null);
    try {
      const result = await invoke<OpenWindowResult>("notes_open_window");
      setMessage(
        result.alreadyOpen ? "The window was already open." : "Window opened.",
      );
    } catch (caught) {
      setMessage(null);
      setError(describe(caught));
    }
  }

  return (
    <>
      <p className="tool-placeholder">This tool is not built yet.</p>
      <button type="button" onClick={openWindow}>
        Open test window (temporary)
      </button>
      {message !== null && <p>{message}</p>}
      {error !== null && <p>{error}</p>}
    </>
  );
}
