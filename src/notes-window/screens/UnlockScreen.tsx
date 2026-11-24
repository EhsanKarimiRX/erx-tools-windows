import { useState } from "react";
import { Lock } from "lucide-react";
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
      <div className="nw-unlock-card">
        <Lock
          className="nw-unlock-icon"
          size={28}
          strokeWidth={1.5}
          aria-hidden="true"
        />
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
          <p className="nw-unlock-hint">
            Type the words you saved when you created your notes.
          </p>
        </div>

        <div className="nw-actions">
          <button
            type="button"
            className="nw-button-primary nw-unlock-submit"
            onClick={() => void handleUnlock()}
            disabled={running || typed.trim() === ""}
            aria-busy={running}
          >
            {running && (
              <span className="nw-spinner">
                <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" width="20" height="20" aria-hidden="true"><rect width="7.33" height="7.33" x="1" y="1" fill="currentColor"><animate id="SVGzjrPLenI" attributeName="x" begin="0;SVGXAURnSRI.end+0.2s" dur="0.6s" values="1;4;1"></animate><animate attributeName="y" begin="0;SVGXAURnSRI.end+0.2s" dur="0.6s" values="1;4;1"></animate><animate attributeName="width" begin="0;SVGXAURnSRI.end+0.2s" dur="0.6s" values="7.33;1.33;7.33"></animate><animate attributeName="height" begin="0;SVGXAURnSRI.end+0.2s" dur="0.6s" values="7.33;1.33;7.33"></animate></rect><rect width="7.33" height="7.33" x="8.33" y="1" fill="currentColor"><animate attributeName="x" begin="SVGzjrPLenI.begin+0.1s" dur="0.6s" values="8.33;11.33;8.33"></animate><animate attributeName="y" begin="SVGzjrPLenI.begin+0.1s" dur="0.6s" values="1;4;1"></animate><animate attributeName="width" begin="SVGzjrPLenI.begin+0.1s" dur="0.6s" values="7.33;1.33;7.33"></animate><animate attributeName="height" begin="SVGzjrPLenI.begin+0.1s" dur="0.6s" values="7.33;1.33;7.33"></animate></rect><rect width="7.33" height="7.33" x="1" y="8.33" fill="currentColor"><animate attributeName="x" begin="SVGzjrPLenI.begin+0.1s" dur="0.6s" values="1;4;1"></animate><animate attributeName="y" begin="SVGzjrPLenI.begin+0.1s" dur="0.6s" values="8.33;11.33;8.33"></animate><animate attributeName="width" begin="SVGzjrPLenI.begin+0.1s" dur="0.6s" values="7.33;1.33;7.33"></animate><animate attributeName="height" begin="SVGzjrPLenI.begin+0.1s" dur="0.6s" values="7.33;1.33;7.33"></animate></rect><rect width="7.33" height="7.33" x="15.66" y="1" fill="currentColor"><animate attributeName="x" begin="SVGzjrPLenI.begin+0.2s" dur="0.6s" values="15.66;18.66;15.66"></animate><animate attributeName="y" begin="SVGzjrPLenI.begin+0.2s" dur="0.6s" values="1;4;1"></animate><animate attributeName="width" begin="SVGzjrPLenI.begin+0.2s" dur="0.6s" values="7.33;1.33;7.33"></animate><animate attributeName="height" begin="SVGzjrPLenI.begin+0.2s" dur="0.6s" values="7.33;1.33;7.33"></animate></rect><rect width="7.33" height="7.33" x="8.33" y="8.33" fill="currentColor"><animate attributeName="x" begin="SVGzjrPLenI.begin+0.2s" dur="0.6s" values="8.33;11.33;8.33"></animate><animate attributeName="y" begin="SVGzjrPLenI.begin+0.2s" dur="0.6s" values="8.33;11.33;8.33"></animate><animate attributeName="width" begin="SVGzjrPLenI.begin+0.2s" dur="0.6s" values="7.33;1.33;7.33"></animate><animate attributeName="height" begin="SVGzjrPLenI.begin+0.2s" dur="0.6s" values="7.33;1.33;7.33"></animate></rect><rect width="7.33" height="7.33" x="1" y="15.66" fill="currentColor"><animate attributeName="x" begin="SVGzjrPLenI.begin+0.2s" dur="0.6s" values="1;4;1"></animate><animate attributeName="y" begin="SVGzjrPLenI.begin+0.2s" dur="0.6s" values="15.66;18.66;15.66"></animate><animate attributeName="width" begin="SVGzjrPLenI.begin+0.2s" dur="0.6s" values="7.33;1.33;7.33"></animate><animate attributeName="height" begin="SVGzjrPLenI.begin+0.2s" dur="0.6s" values="7.33;1.33;7.33"></animate></rect><rect width="7.33" height="7.33" x="15.66" y="8.33" fill="currentColor"><animate attributeName="x" begin="SVGzjrPLenI.begin+0.3s" dur="0.6s" values="15.66;18.66;15.66"></animate><animate attributeName="y" begin="SVGzjrPLenI.begin+0.3s" dur="0.6s" values="8.33;11.33;8.33"></animate><animate attributeName="width" begin="SVGzjrPLenI.begin+0.3s" dur="0.6s" values="7.33;1.33;7.33"></animate><animate attributeName="height" begin="SVGzjrPLenI.begin+0.3s" dur="0.6s" values="7.33;1.33;7.33"></animate></rect><rect width="7.33" height="7.33" x="8.33" y="15.66" fill="currentColor"><animate attributeName="x" begin="SVGzjrPLenI.begin+0.3s" dur="0.6s" values="8.33;11.33;8.33"></animate><animate attributeName="y" begin="SVGzjrPLenI.begin+0.3s" dur="0.6s" values="15.66;18.66;15.66"></animate><animate attributeName="width" begin="SVGzjrPLenI.begin+0.3s" dur="0.6s" values="7.33;1.33;7.33"></animate><animate attributeName="height" begin="SVGzjrPLenI.begin+0.3s" dur="0.6s" values="7.33;1.33;7.33"></animate></rect><rect width="7.33" height="7.33" x="15.66" y="15.66" fill="currentColor"><animate id="SVGXAURnSRI" attributeName="x" begin="SVGzjrPLenI.begin+0.4s" dur="0.6s" values="15.66;18.66;15.66"></animate><animate attributeName="y" begin="SVGzjrPLenI.begin+0.4s" dur="0.6s" values="15.66;18.66;15.66"></animate><animate attributeName="width" begin="SVGzjrPLenI.begin+0.4s" dur="0.6s" values="7.33;1.33;7.33"></animate><animate attributeName="height" begin="SVGzjrPLenI.begin+0.4s" dur="0.6s" values="7.33;1.33;7.33"></animate></rect></svg>
              </span>
            )}
            Unlock
          </button>
        </div>

        {error !== null && (
          <p className="nw-error" role="alert">
            {error}
          </p>
        )}
      </div>
    </div>
  );
}
