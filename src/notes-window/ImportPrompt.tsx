/** Import protection prompt: asks for the words that protect an encrypted
 * .erxnote file while Rust keeps the file waiting.
 *
 * The typed words live in React state only. They are cleared before the import
 * call and are never kept afterwards, even when they were wrong: the user
 * retypes them.
 *
 * While the file waits, Rust holds it. If the prompt closes without a finished
 * import, the unmount cleanup tells Rust to forget the file (best effort).
 */
import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { cancelImport, importWithPassphrase } from "./api";
import { isCode, toApiError } from "./errors";

interface ImportPromptProps {
  token: string;
  onImported: (count: number, legacy: boolean) => void;
  onClose: (message?: string) => void;
}

export default function ImportPrompt({
  token,
  onImported,
  onClose,
}: ImportPromptProps) {
  // The typed words: state only, cleared before the import call.
  const [words, setWords] = useState("");
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  // Set as soon as Rust has ended the pending import (success, stale, or a
  // hard error). While false, leaving the prompt cancels the pending import.
  const endedRef = useRef(false);
  const mountedRef = useRef(true);
  const cancelTimerRef = useRef<number | null>(null);
  const focusAfterErrorRef = useRef(false);

  // Move keyboard focus to the textarea when the prompt opens.
  useEffect(() => {
    textareaRef.current?.focus();
  }, []);

  // Leaving without a finished import: ask Rust to forget the waiting file.
  // The cancel is deferred by one tick so that the StrictMode re-mount in
  // development can cancel it; a real unmount still cancels the pending import
  // (best effort, result ignored).
  useEffect(() => {
    mountedRef.current = true;
    if (cancelTimerRef.current !== null) {
      window.clearTimeout(cancelTimerRef.current);
      cancelTimerRef.current = null;
    }
    return () => {
      mountedRef.current = false;
      if (!endedRef.current) {
        cancelTimerRef.current = window.setTimeout(() => {
          cancelTimerRef.current = null;
          if (!endedRef.current) {
            void cancelImport().catch(() => undefined);
          }
        }, 0);
      }
    };
  }, []);

  useEffect(() => {
    if (!working && focusAfterErrorRef.current) {
      focusAfterErrorRef.current = false;
      textareaRef.current?.focus();
    }
  }, [working]);

  async function handleImport() {
    if (working || words.trim() === "") {
      return;
    }
    const typed = words;
    setWords("");
    setError(null);
    setWorking(true);
    try {
      const result = await importWithPassphrase(token, typed);
      if (!mountedRef.current) {
        return;
      }
      if (result.status === "imported") {
        endedRef.current = true;
        onImported(result.count, result.legacy);
      } else {
        onClose("The import could not be completed.");
      }
    } catch (problem) {
      if (!mountedRef.current) {
        return;
      }
      if (isCode(problem, "wrong_import_passphrase")) {
        setError(
          "These words do not match this file. Check them and try again.",
        );
        focusAfterErrorRef.current = true;
        setWorking(false);
      } else if (isCode(problem, "words")) {
        setError(toApiError(problem).message);
        focusAfterErrorRef.current = true;
        setWorking(false);
      } else if (isCode(problem, "no_pending_import")) {
        endedRef.current = true;
        onClose("This file is no longer waiting. Choose it again to restore.");
      } else if (isCode(problem, "not_unlocked")) {
        endedRef.current = true;
        onClose();
      } else {
        endedRef.current = true;
        onClose(toApiError(problem).message);
      }
    }
  }

  function handleCancel() {
    if (working) {
      return;
    }
    onClose();
  }

  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === "Escape" && !working) {
      event.preventDefault();
      onClose();
    }
  }

  return (
    <div className="nw-dialog-backdrop">
      <div
        className="nw-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="nw-import-dialog-title"
        onKeyDown={handleKeyDown}
      >
        <h2 id="nw-import-dialog-title">This file needs its words</h2>
        <p>
          Type the words that protect this file. They are the words that were
          shown when the file was exported.
        </p>
        <div className="nw-field">
          <label htmlFor="nw-import-words">Words</label>
          <textarea
            id="nw-import-words"
            ref={textareaRef}
            className="nw-textarea"
            rows={3}
            value={words}
            onChange={(event) => setWords(event.target.value)}
            autoComplete="off"
            autoCorrect="off"
            autoCapitalize="off"
            spellCheck={false}
            dir="ltr"
            disabled={working}
          />
        </div>
        {working && (
          <p className="nw-status" role="status">
            Checking the words...
          </p>
        )}
        {error !== null && (
          <p className="nw-error" role="alert">
            {error}
          </p>
        )}
        <div className="nw-actions">
          <button
            type="button"
            className="nw-button-primary"
            onClick={() => void handleImport()}
            disabled={working || words.trim() === ""}
          >
            Import
          </button>
          <button
            type="button"
            className="nw-button-secondary"
            onClick={handleCancel}
            disabled={working}
          >
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}
