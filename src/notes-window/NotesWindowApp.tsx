// TEMPORARY (N6-5): test page for the hardened window. N8 replaces it with the real Secure Notes interface.
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

interface NotesStatus {
  hasFile: boolean;
  unlocked: boolean;
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

export default function NotesWindowApp() {
  const label = getCurrentWindow().label;
  const [status, setStatus] = useState<string | null>(null);
  const [statusError, setStatusError] = useState<string | null>(null);
  const [closeError, setCloseError] = useState<string | null>(null);
  const [listenerStatus, setListenerStatus] = useState(
    "event listener: starting",
  );

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    listen("notes://autosave", () => {}).then(
      (stop) => {
        if (cancelled) {
          stop();
          return;
        }
        unlisten = stop;
        setListenerStatus("event listener: ready");
      },
      (error: unknown) => {
        if (!cancelled) {
          setListenerStatus(`event listener: FAILED - ${describe(error)}`);
        }
      },
    );
    return () => {
      cancelled = true;
      if (unlisten !== null) {
        unlisten();
      }
    };
  }, []);

  async function checkStatus() {
    setStatusError(null);
    try {
      const result = await invoke<NotesStatus>("notes_status");
      setStatus(`hasFile: ${result.hasFile}, unlocked: ${result.unlocked}`);
    } catch (error) {
      setStatus(null);
      setStatusError(describe(error));
    }
  }

  async function closeWindow() {
    setCloseError(null);
    try {
      await invoke("notes_close_window");
    } catch (error) {
      setCloseError(describe(error));
    }
  }

  return (
    <main className="notes-window">
      <h1>Secure Notes - test window (temporary)</h1>
      <p>Window label: {label}</p>

      <input
        type="text"
        id="title"
        name="title"
        placeholder="Title"
        spellCheck={false}
        autoComplete="off"
        autoCorrect="off"
        autoCapitalize="off"
        dir="auto"
      />
      <textarea
        id="body"
        name="body"
        placeholder="Note text"
        spellCheck={false}
        autoComplete="off"
        autoCorrect="off"
        autoCapitalize="off"
        dir="auto"
      />
      <input
        type="search"
        id="search"
        name="search"
        placeholder="Search titles"
        spellCheck={false}
        autoComplete="off"
        autoCorrect="off"
        autoCapitalize="off"
        dir="auto"
      />

      <div className="notes-window-actions">
        <button type="button" onClick={checkStatus}>
          Check status
        </button>
        <button type="button" onClick={closeWindow}>
          Close window
        </button>
      </div>

      {status !== null && <p>{status}</p>}
      {statusError !== null && <p>{statusError}</p>}
      {closeError !== null && <p>{closeError}</p>}
      <p>{listenerStatus}</p>
    </main>
  );
}
