import { useState } from "react";
import { unlockNotes } from "../api";
import { toApiError } from "../errors";

interface UnlockScreenProps {
  notice: string | null;
  onUnlocked: (usedBackup: boolean) => void;
}

/** Unlock flow: type the words, Ctrl+Enter or the button unlocks. The typed
 * words stay in state only and are cleared on success. */
export default function UnlockScreen({
  notice,
  onUnlocked,
}: UnlockScreenProps) {
  // The typed words live only in this component's React state. JavaScript
  // cannot wipe memory: clearing the state removes our reference, but old
  // copies may remain until the engine reuses them.
  const [typed, setTyped] = useState("");
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleUnlock() {
    if (running || typed.trim() === "") return;
    setRunning(true);
    setError(null);
    try {
      const outcome = await unlockNotes(typed);
      setTyped("");
      setRunning(false);
      onUnlocked(outcome.usedBackup);
    } catch (problem) {
      setError(toApiError(problem).message);
      setRunning(false);
    }
  }

  return (
    <div className="nw-screen">
      <h2>Unlock Secure Notes</h2>

      {notice !== null && <p className="nw-notice">{notice}</p>}

      <div className="nw-field">
        <label htmlFor="nw-unlock-words">Your words</label>
        <textarea
          id="nw-unlock-words"
          className="nw-textarea"
          rows={3}
          value={typed}
          onChange={(event) => setTyped(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && event.ctrlKey) {
              event.preventDefault();
              void handleUnlock();
            }
          }}
          spellCheck={false}
          autoComplete="off"
          autoCorrect="off"
          autoCapitalize="off"
          dir="ltr"
          autoFocus
          disabled={running}
        />
      </div>

      <div className="nw-actions">
        <button
          type="button"
          className="nw-button-primary"
          onClick={() => void handleUnlock()}
          disabled={running || typed.trim() === ""}
        >
          Unlock
        </button>
      </div>

      {error !== null && (
        <p className="nw-error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
