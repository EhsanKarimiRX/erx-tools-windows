import { useEffect, useRef, useState } from "react";
import {
  clearWords,
  confirmWordsSaved,
  copyWords,
  finishCreate,
  generateWords,
  refreshWord,
  retypeWords,
} from "../api";
import { toApiError } from "../errors";
import {
  CLIPBOARD_CLEAR_SECONDS,
  DEFAULT_WORDS,
  MAX_WORDS,
  MIN_WORDS,
  type WordsView,
} from "../types";

interface CreateScreenProps {
  onCreated: () => void;
}

type Phase = "choose" | "words" | "retype" | "ready";

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
    return (
      <p className="nw-copied">
        The clipboard was cleared (unless you copied something else).
      </p>
    );
  }

  return (
    <p className="nw-copied">
      Copied.{" "}
      <span className={left <= 10 ? "nw-blink" : undefined}>
        The clipboard will be cleared in {left} seconds.
      </span>
    </p>
  );
}

/** Create flow: choose a word count, save the words, type them again, create.
 * The words are shown for writing down and for nothing else. */
export default function CreateScreen({ onCreated }: CreateScreenProps) {
  const [phase, setPhase] = useState<Phase>("choose");
  const [count, setCount] = useState(DEFAULT_WORDS);
  // The words live only in this component's React state. JavaScript cannot
  // wipe memory: dropping the state removes our reference, but old copies may
  // remain until the engine reuses them. Rust keeps the copy used for
  // encryption, and the words never go back to Rust for copying.
  const [info, setInfo] = useState<WordsView | null>(null);
  const [confirmed, setConfirmed] = useState(false);
  const [savedTick, setSavedTick] = useState(false);
  // The typed words are as sensitive as the generated ones and follow the same
  // rule: state only, dropped as soon as they are no longer needed.
  const [typed, setTyped] = useState("");
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copyRound, setCopyRound] = useState<number | null>(null);
  const created = useRef(false);

  // Leaving without success: ask Rust to forget its words too. Best effort;
  // the result is ignored because there is nothing to show on a closed screen.
  useEffect(() => {
    return () => {
      if (!created.current) {
        void clearWords().catch(() => {
          // Nothing more can be done.
        });
      }
    };
  }, []);

  async function handleGenerate() {
    setWorking(true);
    setError(null);
    try {
      const view = await generateWords(count);
      setInfo(view);
      setConfirmed(false);
      setSavedTick(false);
      setCopyRound(null);
      setPhase("words");
    } catch (problem) {
      setError(toApiError(problem).message);
    } finally {
      setWorking(false);
    }
  }

  async function handleRefresh(slot: number) {
    setWorking(true);
    setError(null);
    try {
      const result = await refreshWord(slot);
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
      setError(toApiError(problem).message);
    } finally {
      setWorking(false);
    }
  }

  async function handleCopy() {
    setWorking(true);
    setError(null);
    try {
      await copyWords();
      setCopyRound((round) => (round ?? 0) + 1);
    } catch (problem) {
      setError(toApiError(problem).message);
    } finally {
      setWorking(false);
    }
  }

  async function handleConfirm() {
    setWorking(true);
    setError(null);
    try {
      await confirmWordsSaved();
      setConfirmed(true);
      setCopyRound(null);
      setPhase("retype");
    } catch (problem) {
      setError(toApiError(problem).message);
    } finally {
      setWorking(false);
    }
  }

  async function handleCheck() {
    setWorking(true);
    setError(null);
    try {
      const matches = await retypeWords(typed);
      if (matches) {
        setTyped("");
        setPhase("ready");
      } else {
        setError("The words do not match. Check the spelling and the spaces.");
      }
    } catch (problem) {
      setError(toApiError(problem).message);
    } finally {
      setWorking(false);
    }
  }

  async function handleCreate() {
    setWorking(true);
    setError(null);
    try {
      await finishCreate();
      created.current = true;
      setInfo(null);
      setTyped("");
      setSavedTick(false);
      setCopyRound(null);
      setConfirmed(false);
      setWorking(false);
      onCreated();
    } catch (problem) {
      setError(toApiError(problem).message);
      setWorking(false);
    }
  }

  function handleStartOver() {
    setPhase("choose");
    setInfo(null);
    setConfirmed(false);
    setSavedTick(false);
    setTyped("");
    setCopyRound(null);
    setError(null);
    void clearWords().catch(() => {
      // Best effort: the local state is already gone.
    });
  }

  return (
    <div className="nw-screen">
      <h2>Create your Secure Notes</h2>

      {phase === "choose" && (
        <>
          <p>
            Your notes are encrypted on this PC and protected by a set of words.
            If you lose the words, nobody can recover your notes, not even us.
            Write the words down on paper or keep them somewhere safe.
          </p>
          <div className="nw-field">
            <label htmlFor="nw-word-count">Number of words</label>
            <select
              id="nw-word-count"
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
                  {!confirmed && (
                    <button
                      type="button"
                      className="nw-word-replace"
                      onClick={() => void handleRefresh(slot)}
                      disabled={working || left === 0}
                    >
                      Replace ({left} left)
                    </button>
                  )}
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
            </div>
            <p className="nw-muted">
              Other programs can read the clipboard while the words are on it.
            </p>
            {copyRound !== null && <CopyCountdown key={copyRound} />}
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
          <textarea
            className="nw-textarea"
            rows={3}
            value={typed}
            onChange={(event) => setTyped(event.target.value)}
            spellCheck={false}
            autoComplete="off"
            autoCorrect="off"
            autoCapitalize="off"
            dir="ltr"
            aria-label="Your words"
            disabled={working}
          />
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
          </div>
        </>
      )}

      {phase === "ready" && (
        <>
          <p>The words match.</p>
          <div className="nw-actions">
            <button
              type="button"
              className="nw-button-primary"
              onClick={() => void handleCreate()}
              disabled={working}
            >
              {working ? "Creating... this takes about a second" : "Create notes"}
            </button>
            <button
              type="button"
              className="nw-button-secondary"
              onClick={handleStartOver}
              disabled={working}
            >
              Start over
            </button>
          </div>
        </>
      )}

      {error !== null && (
        <p className="nw-error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
