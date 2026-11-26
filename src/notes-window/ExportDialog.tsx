/** Export protection dialog: asks how to protect an .erxnote file, collects
 * what is needed, runs the export and reports back.
 *
 * The typed notebook words live in React state only. They are cleared before
 * the export call and are never kept afterwards, even when they were wrong:
 * the user retypes them.
 *
 * Generated words live only in WordsPanel; this dialog never sees them.
 *
 * After any export call Rust has wiped the export words, so the "new" flow
 * restarts the words panel when the export fails. A locked notebook ends the
 * dialog instead.
 *
 * Known limit: there is no focus trap, so Tab can reach the page behind the
 * dialog. To be checked in the manual tests (R7).
 */
import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import WordsPanel from "./WordsPanel";
import { isCode, toApiError } from "./errors";
import type { ExportPassphraseMode } from "./types";

interface ExportDialogProps {
  title: string;
  run: (
    mode: ExportPassphraseMode,
    currentWords?: string,
  ) => Promise<string | null>;
  /** The file that was written, or null when the save dialog was cancelled. */
  onDone: (path: string | null) => void;
  /** The user cancelled, or the notebook locked. */
  onCancel: () => void;
}

type Stage = "choose" | "current" | "new" | "busy";

export default function ExportDialog({
  title,
  run,
  onDone,
  onCancel,
}: ExportDialogProps) {
  const [stage, setStage] = useState<Stage>("choose");
  // The typed notebook words: state only, cleared before the export call.
  const [typed, setTyped] = useState("");
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Remounts WordsPanel so a failed export starts the words flow again.
  const [panelKey, setPanelKey] = useState(0);
  const firstButtonRef = useRef<HTMLButtonElement | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);

  // Move focus to the first choice, and to the textarea when the user types
  // the notebook words.
  useEffect(() => {
    if (stage === "choose") {
      firstButtonRef.current?.focus();
    } else if (stage === "current") {
      textareaRef.current?.focus();
    }
  }, [stage]);

  function handleProblem(problem: unknown) {
    if (isCode(problem, "not_unlocked")) {
      onCancel();
    } else {
      setError(toApiError(problem).message);
    }
  }

  async function handleExport() {
    const words = typed;
    setTyped("");
    setWorking(true);
    setError(null);
    try {
      const path = await run("current", words);
      onDone(path);
    } catch (problem) {
      handleProblem(problem);
    } finally {
      setWorking(false);
    }
  }

  function handleBack() {
    setTyped("");
    setError(null);
    setStage("choose");
  }

  async function runNew() {
    setError(null);
    try {
      const path = await run("new");
      onDone(path);
    } catch (problem) {
      if (isCode(problem, "not_unlocked")) {
        onCancel();
        return;
      }
      setError(toApiError(problem).message);
      // Rust has wiped the export words, so the user must generate and
      // retype new ones.
      setPanelKey((key) => key + 1);
      setStage("new");
    }
  }

  function handleReady() {
    setStage("busy");
    void runNew();
  }

  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    // In the "new" stage WordsPanel handles its own Escape; acting here too
    // would call onCancel twice.
    if (
      event.key === "Escape" &&
      !working &&
      (stage === "choose" || stage === "current")
    ) {
      event.preventDefault();
      onCancel();
    }
  }

  return (
    <div className="nw-dialog-backdrop">
      <div
        className="nw-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="nw-export-dialog-title"
        onKeyDown={handleKeyDown}
      >
        <h2 id="nw-export-dialog-title">{title}</h2>

        {stage === "choose" && (
          <>
            <p>Choose which words protect this file.</p>
            <button
              ref={firstButtonRef}
              type="button"
              className="nw-button-primary"
              onClick={() => setStage("current")}
            >
              Use my notebook words
            </button>
            <p className="nw-muted">
              You type your notebook words now. The file is only as strong as
              your notebook words. If you change your notebook words later,
              this file still needs the old ones.
            </p>
            <button
              type="button"
              className="nw-button-primary"
              onClick={() => setStage("new")}
            >
              Create new words for this file
            </button>
            <p className="nw-muted">
              The file gets its own set of words. Nobody can recover the file
              if you lose them.
            </p>
            <div className="nw-actions">
              <button
                type="button"
                className="nw-button-secondary"
                onClick={onCancel}
              >
                Cancel
              </button>
            </div>
          </>
        )}

        {stage === "current" && (
          <>
            <div className="nw-field">
              <label htmlFor="nw-export-current-words">
                Your notebook words
              </label>
              <textarea
                id="nw-export-current-words"
                ref={textareaRef}
                className="nw-textarea"
                rows={3}
                value={typed}
                onChange={(event) => setTyped(event.target.value)}
                spellCheck={false}
                autoComplete="off"
                autoCorrect="off"
                autoCapitalize="off"
                dir="ltr"
                disabled={working}
              />
            </div>
            {working && (
              <p className="nw-muted">
                Checking the words... then choose where to save. This takes a
                few seconds.
              </p>
            )}
            <div className="nw-actions">
              <button
                type="button"
                className="nw-button-primary"
                onClick={() => void handleExport()}
                disabled={working || typed.trim() === ""}
              >
                Export
              </button>
              <button
                type="button"
                className="nw-button-secondary"
                onClick={handleBack}
                disabled={working}
              >
                Back
              </button>
              <button
                type="button"
                className="nw-button-secondary"
                onClick={onCancel}
                disabled={working}
              >
                Cancel
              </button>
            </div>
          </>
        )}

        {stage === "new" && (
          <WordsPanel key={panelKey} onReady={handleReady} onCancel={onCancel} />
        )}

        {stage === "busy" && (
          <p className="nw-muted">
            Exporting... choose where to save the file. This takes a few
            seconds.
          </p>
        )}

        {error !== null && (
          <p className="nw-error" role="alert">
            {error}
          </p>
        )}
      </div>
    </div>
  );
}
