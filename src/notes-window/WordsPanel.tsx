/** Words panel of the export flow.
 *
 * The words live only in this component's React state. Rust keeps the copy it
 * uses to encrypt the exported file, and the generated words are never sent
 * back to Rust. This panel uses the export-words commands only
 * (exportGenerateWords, exportRefreshWord, exportCopyWords,
 * exportConfirmWordsSaved, exportRetypeWords, exportClearWords) and never the
 * notebook word commands: the notebook words are separate, and changing them
 * later does not change a file exported with these words.
 *
 * Known limit: if the panel unmounts because the notebook locked or
 * auto-locked, the unmount cleanup calls exportClearWords, which then fails
 * with not_unlocked. That error is ignored on purpose: there is no session
 * left to clear.
 */
import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { Copy, RefreshCw } from "lucide-react";
import {
  exportClearWords,
  exportConfirmWordsSaved,
  exportCopyWords,
  exportGenerateWords,
  exportRefreshWord,
  exportRetypeWords,
} from "./api";
import { isCode, toApiError } from "./errors";
import {
  CLIPBOARD_CLEAR_SECONDS,
  DEFAULT_WORDS,
  MAX_WORDS,
  MIN_WORDS,
  type WordsView,
} from "./types";

interface WordsPanelProps {
  onReady: () => void;
  onCancel: () => void;
}

type Phase = "choose" | "words" | "retype" | "matched";

const WORD_COUNTS: number[] = Array.from(
  { length: MAX_WORDS - MIN_WORDS + 1 },
  (_unused, index) => MIN_WORDS + index,
);

/** Seconds left until Rust clears the clipboard; counts down once a second.
 * Display only: Rust does the clearing. Remounted for every new copy. */
function CopyCountdown() {
  const [left, setLeft] = useState(CLIPBOARD_CLEAR_SECONDS);

  useEffect(() => {
    const id = window.setInterval(() => {
      setLeft((value) => Math.max(0, value - 1));
    }, 1000);
    return () => window.clearInterval(id);
  }, []);

  if (left === 0) {
    return <>The clipboard was cleared (unless you copied something else).</>;
  }

  return (
    <>
      <strong>Copied.</strong>{" "}
      <span className="nw-blink">
        The clipboard will be cleared in {left} seconds.
      </span>{" "}
      After that the words can no longer be pasted, so paste them somewhere safe
      first (a note or a document) or write them down.
    </>
  );
}

/** Export words flow: choose a count, save the words, type them again.
 * After a correct retype the parent decides what happens next. */
export default function WordsPanel({ onReady, onCancel }: WordsPanelProps) {
  const [phase, setPhase] = useState<Phase>("choose");
  const [count, setCount] = useState(DEFAULT_WORDS);
  // The words live only in this component's React state. JavaScript cannot
  // wipe memory: dropping the state removes our reference, but old copies may
  // remain until the engine reuses them. Rust keeps the copy used for the
  // export encryption.
  const [info, setInfo] = useState<WordsView | null>(null);
  const [savedTick, setSavedTick] = useState(false);
  // The typed words are as sensitive as the generated ones and follow the same
  // rule: state only, dropped as soon as they are no longer needed.
  const [typed, setTyped] = useState("");
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copyRound, setCopyRound] = useState<number | null>(null);
  // Set right before onReady(), so a normal hand-off does not clear the words
  // Rust just confirmed. The parent exports next; Rust wipes them itself.
  const readyRef = useRef(false);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);

  // Leaving without a hand-off: ask Rust to forget its words too. Best effort;
  // the result is ignored because there is nothing to show on a closed panel.
  useEffect(() => {
    return () => {
      if (!readyRef.current) {
        void exportClearWords().catch(() => {
          // Nothing more can be done. On lock/auto-lock this fails with
          // not_unlocked and that is ignored on purpose (see the top comment).
        });
      }
    };
  }, []);

  // Move keyboard focus to the textarea when the retype phase begins.
  useEffect(() => {
    if (phase === "retype") {
      textareaRef.current?.focus();
    }
  }, [phase]);

  function clearLocal() {
    setInfo(null);
    setTyped("");
    setSavedTick(false);
    setCopyRound(null);
  }

  /** Shared catch handling: show the message, and leave the panel when the
   * notebook was locked underneath us. */
  function handleProblem(problem: unknown) {
    setError(toApiError(problem).message);
    if (isCode(problem, "not_unlocked")) {
      clearLocal();
      onCancel();
    }
  }

  async function handleGenerate() {
    setWorking(true);
    setError(null);
    try {
      const view = await exportGenerateWords(count);
      setInfo(view);
      setSavedTick(false);
      setCopyRound(null);
      setPhase("words");
    } catch (problem) {
      handleProblem(problem);
    } finally {
      setWorking(false);
    }
  }

  async function handleRefresh(slot: number) {
    setWorking(true);
    setError(null);
    try {
      const result = await exportRefreshWord(slot);
      setInfo((previous) =>
        previous === null
          ? previous
          : {
              ...previous,
              words: previous.words.map((word, index) =>
                index === slot ? result.word : word,
              ),
              replacementsLeft: previous.replacementsLeft.map((left, index) =>
                index === slot ? result.replacementsLeft : left,
              ),
            },
      );
    } catch (problem) {
      handleProblem(problem);
    } finally {
      setWorking(false);
    }
  }

  async function handleCopy() {
    setWorking(true);
    setError(null);
    try {
      await exportCopyWords();
      setCopyRound((round) => (round ?? 0) + 1);
    } catch (problem) {
      handleProblem(problem);
    } finally {
      setWorking(false);
    }
  }

  async function handleConfirm() {
    setWorking(true);
    setError(null);
    try {
      await exportConfirmWordsSaved();
      setCopyRound(null);
      setPhase("retype");
    } catch (problem) {
      handleProblem(problem);
    } finally {
      setWorking(false);
    }
  }

  async function handleCheck() {
    setWorking(true);
    setError(null);
    try {
      const matches = await exportRetypeWords(typed);
      if (matches) {
        setTyped("");
        readyRef.current = true;
        setPhase("matched");
        onReady();
      } else {
        setError("The words do not match. Check the spelling and the spaces.");
      }
    } catch (problem) {
      handleProblem(problem);
    } finally {
      setWorking(false);
    }
  }

  function handleStartOver() {
    clearLocal();
    setError(null);
    setPhase("choose");
    void exportClearWords().catch(() => {
      // Best effort: the local state is already gone.
    });
  }

  function handleCancel() {
    if (working) {
      return;
    }
    clearLocal();
    setError(null);
    void exportClearWords().catch(() => {
      // Best effort: the local state is already gone.
    });
    onCancel();
  }

  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === "Escape" && !working) {
      event.preventDefault();
      handleCancel();
    }
  }

  return (
    <div className="nw-screen" onKeyDown={handleKeyDown}>
      <h2>Words for this exported file</h2>

      {phase === "choose" && (
        <>
          <p>
            These words protect only this exported file. They are different
            from your notebook words. If you lose them, nobody can recover the
            file, not even us. Changing your notebook words later does not
            change this file.
          </p>
          <div className="nw-field">
            <label htmlFor="nw-export-word-count">Number of words</label>
            <select
              id="nw-export-word-count"
              className="nw-select"
              value={count}
              onChange={(event) => setCount(Number(event.target.value))}
              disabled={working}
            >
              {WORD_COUNTS.map((value) => (
                <option key={value} value={value}>
                  {value}
                </option>
              ))}
            </select>
          </div>
          <div className="nw-actions">
            <button
              type="button"
              className="nw-button-primary"
              onClick={() => void handleGenerate()}
              disabled={working}
            >
              Generate words
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
        </>
      )}

      {phase === "words" && info !== null && (
        <>
          <ol className="nw-words">
            {info.words.map((word, slot) => {
              const left = info.replacementsLeft[slot] ?? 0;
              return (
                <li key={slot} className="nw-word">
                  <span className="nw-word-number">{slot + 1}</span>
                  <span className="nw-word-text">{word}</span>
                  <button
                    type="button"
                    className="nw-word-replace"
                    onClick={() => void handleRefresh(slot)}
                    disabled={working || left === 0}
                    title={
                      left === 0
                        ? "No replacements left for this word"
                        : `Replace this word (${left} left)`
                    }
                    aria-label={`Replace word ${slot + 1}, ${left} replacements left`}
                  >
                    <RefreshCw size={14} aria-hidden="true" />
                    <span>{left}</span>
                  </button>
                </li>
              );
            })}
          </ol>

          <p className="nw-strength">
            About {info.entropyBits.toFixed(1)} bits of strength.
          </p>
          {info.weak && (
            <p className="nw-warning">
              This is a short set of words. More words are safer.
            </p>
          )}

          <div className="nw-copy">
            <div className="nw-actions">
              <button
                type="button"
                className="nw-button-secondary"
                onClick={() => void handleCopy()}
                disabled={working}
              >
                <Copy size={14} aria-hidden="true" />
                Copy words
              </button>
              <button
                type="button"
                className="nw-button-secondary"
                onClick={handleStartOver}
                disabled={working}
              >
                Start over
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
            <p className="nw-muted">
              Other programs can read the clipboard while the words are on it.
            </p>
            {copyRound !== null && (
              <div className="nw-copied" role="note">
                <p>
                  <CopyCountdown key={copyRound} />
                </p>
              </div>
            )}
          </div>

          <label className="nw-check">
            <input
              type="checkbox"
              checked={savedTick}
              onChange={(event) => setSavedTick(event.target.checked)}
              disabled={working}
            />
            I have saved these words
          </label>

          <div className="nw-actions">
            <button
              type="button"
              className="nw-button-primary"
              onClick={() => void handleConfirm()}
              disabled={!savedTick || working}
            >
              Continue
            </button>
          </div>
        </>
      )}

      {phase === "retype" && (
        <>
          <p>Type the words again to confirm that you saved them.</p>
          <div className="nw-field">
            <label htmlFor="nw-export-retype">Your words</label>
            <textarea
              id="nw-export-retype"
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
          <div className="nw-actions">
            <button
              type="button"
              className="nw-button-primary"
              onClick={() => void handleCheck()}
              disabled={working}
            >
              Check words
            </button>
            <button
              type="button"
              className="nw-button-secondary"
              onClick={handleStartOver}
              disabled={working}
            >
              Start over
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
        </>
      )}

      {phase === "matched" && <p>The words match.</p>}

      {error !== null && (
        <p className="nw-error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
