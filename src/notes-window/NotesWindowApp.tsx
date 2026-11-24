import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { lockNotes, notesActivity, notesStatus } from "./api";
import { isCode, toApiError } from "./errors";
import CreateScreen from "./screens/CreateScreen";
import NotesScreen from "./screens/NotesScreen";
import UnlockScreen from "./screens/UnlockScreen";
import type { AutoLockedEvent, AutosaveEvent, SaveState } from "./types";

type Screen =
  | { kind: "loading" }
  | { kind: "unavailable" }
  | { kind: "failed"; message: string }
  | { kind: "create" }
  | { kind: "unlock"; notice: string | null }
  | { kind: "notes" };

const ACTIVITY_THROTTLE_MS = 10_000;

function saveLabel(state: SaveState): string {
  if (state === "saved") return "Saved";
  if (state === "pending") return "Saving...";
  return "Not saved - will retry";
}

export default function NotesWindowApp() {
  const [screen, setScreen] = useState<Screen>({ kind: "loading" });
  const [saveIndicator, setSaveIndicator] = useState<SaveState>("saved");
  const [usedBackup, setUsedBackup] = useState(false);
  const [lockError, setLockError] = useState<string | null>(null);
  const [locking, setLocking] = useState(false);
  const [statusAttempt, setStatusAttempt] = useState(0);
  const lastActivity = useRef(0);

  useEffect(() => {
    let cancelled = false;
    notesStatus().then(
      (status) => {
        if (cancelled) return;
        if (!status.hasFile) {
          setScreen({ kind: "create" });
        } else if (!status.unlocked) {
          setScreen({ kind: "unlock", notice: null });
        } else {
          setScreen({ kind: "notes" });
        }
      },
      (error: unknown) => {
        if (cancelled) return;
        const apiError = toApiError(error);
        if (apiError.code === "unavailable") {
          setScreen({ kind: "unavailable" });
        } else {
          setScreen({ kind: "failed", message: apiError.message });
        }
      },
    );
    return () => {
      cancelled = true;
    };
  }, [statusAttempt]);

  // Rust events. The cancelled flag makes this safe under StrictMode: a
  // listener that resolves after cleanup is stopped immediately.
  useEffect(() => {
    let cancelled = false;
    const stops: Array<() => void> = [];

    async function register() {
      const stopAutoLocked = await listen<AutoLockedEvent>(
        "notes://auto-locked",
        (event) => {
          setUsedBackup(false);
          setLockError(null);
          setScreen({
            kind: "unlock",
            notice: event.payload.finalSaveFailed
              ? "Secure Notes locked itself after inactivity. The last changes could not be saved."
              : "Secure Notes locked itself after inactivity.",
          });
        },
      );
      if (cancelled) {
        stopAutoLocked();
        return;
      }
      stops.push(stopAutoLocked);

      const stopAutosave = await listen<AutosaveEvent>(
        "notes://autosave",
        (event) => {
          setSaveIndicator(event.payload.ok ? "saved" : "failed");
        },
      );
      if (cancelled) {
        stopAutosave();
        return;
      }
      stops.push(stopAutosave);
    }

    void register().catch(() => {
      // Without the events the window still works; only auto-lock notices and
      // the save indicator stay quiet.
    });

    return () => {
      cancelled = true;
      for (const stop of stops) stop();
    };
  }, []);

  // Only the notes screen reports activity, at most once per 10 seconds.
  useEffect(() => {
    if (screen.kind !== "notes") return;
    let cancelled = false;
    lastActivity.current = 0;

    const ping = () => {
      const now = Date.now();
      if (now - lastActivity.current < ACTIVITY_THROTTLE_MS) return;
      lastActivity.current = now;
      notesActivity().catch((error: unknown) => {
        if (cancelled) return;
        if (isCode(error, "not_unlocked")) {
          setUsedBackup(false);
          setLockError(null);
          setScreen({ kind: "unlock", notice: "Secure Notes is locked." });
        }
        // Every other error is ignored on purpose: activity is best-effort.
      });
    };

    window.addEventListener("keydown", ping);
    window.addEventListener("pointerdown", ping);
    window.addEventListener("pointermove", ping);
    return () => {
      cancelled = true;
      window.removeEventListener("keydown", ping);
      window.removeEventListener("pointerdown", ping);
      window.removeEventListener("pointermove", ping);
    };
  }, [screen.kind]);

  async function handleLock(force: boolean) {
    setLockError(null);
    setLocking(true);
    try {
      const report = await lockNotes(force);
      setUsedBackup(false);
      setScreen({
        kind: "unlock",
        notice: report.finalSaveFailed
          ? "Secure Notes is locked. The last changes could not be saved."
          : "Secure Notes is locked.",
      });
    } catch (error) {
      setLockError(toApiError(error).message);
    } finally {
      setLocking(false);
    }
  }

  const onNotes = screen.kind === "notes";

  return (
    <div className="nw-root">
      <header className="nw-header">
        <h1 className="nw-title">Secure Notes</h1>
        {onNotes && (
          <div className="nw-header-actions">
            <span className={`nw-save nw-save-${saveIndicator}`}>
              {saveLabel(saveIndicator)}
            </span>
            <button
              type="button"
              className="nw-button-secondary"
              onClick={() => void handleLock(false)}
              disabled={locking}
            >
              Lock
            </button>
          </div>
        )}
      </header>

      {lockError !== null && (
        <div className="nw-lock-error" role="alert">
          <p>{lockError}</p>
          <p>Your latest changes could not be saved. Lock anyway and lose them?</p>
          <div className="nw-actions">
            <button
              type="button"
              className="nw-button-danger"
              onClick={() => void handleLock(true)}
              disabled={locking}
            >
              Lock anyway
            </button>
            <button
              type="button"
              className="nw-button-secondary"
              onClick={() => setLockError(null)}
              disabled={locking}
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      <div className="nw-body">
        {screen.kind === "loading" && (
          <p className="nw-muted nw-centered">Loading Secure Notes...</p>
        )}

        {screen.kind === "unavailable" && (
          <p className="nw-muted nw-centered">Secure Notes is not available.</p>
        )}

        {screen.kind === "failed" && (
          <div className="nw-message nw-centered">
            <p role="alert">{screen.message}</p>
            <button
              type="button"
              className="nw-button-primary"
              onClick={() => {
                setScreen({ kind: "loading" });
                setStatusAttempt((attempt) => attempt + 1);
              }}
            >
              Try again
            </button>
          </div>
        )}

        {screen.kind === "create" && (
          <CreateScreen
            onCreated={() => {
              setUsedBackup(false);
              setSaveIndicator("saved");
              setScreen({ kind: "notes" });
            }}
          />
        )}

        {screen.kind === "unlock" && (
          <UnlockScreen
            notice={screen.notice}
            onUnlocked={(openedBackup) => {
              setUsedBackup(openedBackup);
              setSaveIndicator("saved");
              setScreen({ kind: "notes" });
            }}
          />
        )}

        {screen.kind === "notes" && (
          <>
            {usedBackup && (
              <div className="nw-banner" role="status">
                <p>
                  The main notes file was damaged, so the previous saved version
                  was opened. Your latest changes may be missing.
                </p>
                <button
                  type="button"
                  className="nw-button-secondary"
                  onClick={() => setUsedBackup(false)}
                >
                  Dismiss
                </button>
              </div>
            )}
            <NotesScreen />
          </>
        )}
      </div>
    </div>
  );
}
