import { useEffect, useRef, useState } from "react";
import { Copy, RefreshCw } from "lucide-react";
import {
  cancelImport,
  changePassphrase,
  clearWords,
  confirmWordsSaved,
  copyWords,
  generateWords,
  getSettings,
  importNotes,
  refreshWord,
  retypeWords,
  setSettings,
} from "../api";
import { toApiError } from "../errors";
import {
  CLIPBOARD_CLEAR_SECONDS,
  DEFAULT_WORDS,
  MAX_WORDS,
  MIN_WORDS,
  type PreviewMode,
  type WordsView,
} from "../types";

interface SettingsScreenProps {
  onBack: () => void;
}

type ChangePhase = "choose" | "words" | "verify" | "done";

/** Must match MIN_AUTO_LOCK_MINUTES and MAX_AUTO_LOCK_MINUTES in
 * src-tauri/src/tools/notes/model.rs. */
const MIN_AUTO_LOCK = 1;
const MAX_AUTO_LOCK = 240;
const SAVED_MS = 3_000;

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

/** Settings: preview mode and auto-lock time, plus an inline flow that replaces
 * the passphrase. The new words live only in this component's React state. */
export default function SettingsScreen({ onBack }: SettingsScreenProps) {
  const [preview, setPreview] = useState<PreviewMode>("titles");
  const [minutes, setMinutes] = useState("10");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [backupWorking, setBackupWorking] = useState(false);
  const [backupMessage, setBackupMessage] = useState<string | null>(null);
  const [backupError, setBackupError] = useState<string | null>(null);

  const [changing, setChanging] = useState(false);
  const [phase, setPhase] = useState<ChangePhase>("choose");
  const [count, setCount] = useState(DEFAULT_WORDS);
  const [info, setInfo] = useState<WordsView | null>(null);
  const [confirmed, setConfirmed] = useState(false);
  const [savedTick, setSavedTick] = useState(false);
  // The new and the previous words stay in state only and are dropped as soon
  // as they are no longer needed.
  const [typedNew, setTypedNew] = useState("");
  const [typedOld, setTypedOld] = useState("");
  const [working, setWorking] = useState(false);
  const [changeError, setChangeError] = useState<string | null>(null);
  const [copyRound, setCopyRound] = useState<number | null>(null);
  const [oldBackupGone, setOldBackupGone] = useState(true);

  const changed = useRef(false);
  const savedTimer = useRef<number | null>(null);

  // Load the current settings once.
  useEffect(() => {
    let cancelled = false;
    getSettings().then(
      (view) => {
        if (cancelled) return;
        setPreview(view.preview);
        setMinutes(`${view.autoLockMinutes}`);
        setLoading(false);
      },
      (problem: unknown) => {
        if (cancelled) return;
        setError(toApiError(problem).message);
        setLoading(false);
      },
    );
    return () => {
      cancelled = true;
    };
  }, []);

  // Leaving without a completed change: ask Rust to forget its words too.
  // Best effort; the result is ignored because the screen may be gone.
  useEffect(() => {
    return () => {
      if (savedTimer.current !== null) {
        window.clearTimeout(savedTimer.current);
        savedTimer.current = null;
      }
      if (!changed.current) {
        void clearWords().catch(() => {
          // Nothing more can be done.
        });
      }
    };
  }, []);

  function showSaved() {
    setSaved(true);
    if (savedTimer.current !== null) window.clearTimeout(savedTimer.current);
    savedTimer.current = window.setTimeout(() => {
      savedTimer.current = null;
      setSaved(false);
    }, SAVED_MS);
  }

  async function handleSave() {
    setError(null);
    setSaved(false);
    const value = Number(minutes);
    if (
      !Number.isInteger(value) ||
      value < MIN_AUTO_LOCK ||
      value > MAX_AUTO_LOCK
    ) {
      setError("Enter a whole number of minutes from 1 to 240.");
      return;
    }
    setSaving(true);
    try {
      await setSettings(preview, value);
      showSaved();
    } catch (problem) {
      setError(toApiError(problem).message);
    } finally {
      setSaving(false);
    }
  }

  async function handleExportAll() {
    setBackupMessage(null);
    setBackupError(null);
    // TEMP(R5a): replaced in R5d/R6a
    setBackupError("Export is being updated.");
  }

  async function handleRestore() {
    setBackupMessage(null);
    setBackupError(null);
    setBackupWorking(true);
    try {
      // TEMP(R5a): replaced in R5d/R6a
      const result = await importNotes();
      if (result.status === "cancelled") {
        setBackupMessage("Restore cancelled.");
      } else if (result.status === "imported") {
        setBackupMessage(`Added ${result.count} note(s).`);
      } else {
        await cancelImport();
        setBackupError("This file needs its words. This is not supported yet.");
      }
    } catch (problem) {
      setBackupError(toApiError(problem).message);
    } finally {
      setBackupWorking(false);
    }
  }

  function resetChange() {
    setPhase("choose");
    setInfo(null);
    setConfirmed(false);
    setSavedTick(false);
    setTypedNew("");
    setTypedOld("");
    setCopyRound(null);
    setChangeError(null);
    void clearWords().catch(() => {
      // Best effort: the local state is already gone.
    });
  }

  function handleCancel() {
    if (working) return;
    setChanging(false);
    resetChange();
  }

  async function handleStart() {
    setWorking(true);
    setChangeError(null);
    try {
      const view = await generateWords(count);
      setInfo(view);
      setConfirmed(false);
      setSavedTick(false);
      setCopyRound(null);
      setPhase("words");
    } catch (problem) {
      setChangeError(toApiError(problem).message);
    } finally {
      setWorking(false);
    }
  }

  async function handleRefresh(slot: number) {
    setWorking(true);
    setChangeError(null);
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
      setChangeError(toApiError(problem).message);
    } finally {
      setWorking(false);
    }
  }

  async function handleCopy() {
    setWorking(true);
    setChangeError(null);
    try {
      await copyWords();
      setCopyRound((round) => (round ?? 0) + 1);
    } catch (problem) {
      setChangeError(toApiError(problem).message);
    } finally {
      setWorking(false);
    }
  }

  async function handleConfirm() {
    setWorking(true);
    setChangeError(null);
    try {
      await confirmWordsSaved();
      setConfirmed(true);
      setCopyRound(null);
      setPhase("verify");
    } catch (problem) {
      setChangeError(toApiError(problem).message);
    } finally {
      setWorking(false);
    }
  }

  async function handleChange() {
    if (typedNew.trim() === "" || typedOld.trim() === "") return;
    setWorking(true);
    setChangeError(null);
    try {
      const matches = await retypeWords(typedNew);
      if (!matches) {
        setChangeError(
          "The new words do not match. Check the spelling and the spaces.",
        );
        return;
      }
      const result = await changePassphrase(typedOld);
      changed.current = true;
      setInfo(null);
      setTypedNew("");
      setTypedOld("");
      setConfirmed(false);
      setSavedTick(false);
      setCopyRound(null);
      setOldBackupGone(result.oldBackupGone);
      setPhase("done");
    } catch (problem) {
      setChangeError(toApiError(problem).message);
    } finally {
      setWorking(false);
    }
  }

  function handleDone() {
    setChanging(false);
    resetChange();
  }

  return (
    <div className="nw-screen">
      {!changing && (
        <>
          <div className="nw-settings-bar">
            <h2>Settings</h2>
            <div className="nw-settings-actions">
              {saved && (
                <span className="nw-muted" role="status">
                  Settings saved.
                </span>
              )}
              <button
                type="button"
                className="nw-button-primary"
                onClick={() => void handleSave()}
                disabled={loading || saving}
              >
                {saving ? "Saving..." : "Save settings"}
              </button>
              <button
                type="button"
                className="nw-button-secondary"
                onClick={onBack}
                disabled={saving}
              >
                Back
              </button>
            </div>
          </div>

          {loading ? (
            <p className="nw-muted">Loading settings...</p>
          ) : (
            <div className="nw-settings">
              <div className="nw-settings-field">
                <label htmlFor="nw-preview">Preview in list</label>
                <select
                  id="nw-preview"
                  className="nw-select"
                  value={preview}
                  onChange={(event) =>
                    setPreview(event.target.value as PreviewMode)
                  }
                  disabled={saving}
                >
                  <option value="titles">Titles only</option>
                  <option value="first-line">First line</option>
                </select>
              </div>

              <div className="nw-settings-field">
                <label htmlFor="nw-auto-lock">Auto-lock after</label>
                <input
                  id="nw-auto-lock"
                  type="number"
                  className="nw-input"
                  min={MIN_AUTO_LOCK}
                  max={MAX_AUTO_LOCK}
                  step={1}
                  inputMode="numeric"
                  value={minutes}
                  onChange={(event) => setMinutes(event.target.value)}
                  disabled={saving}
                />
                <p className="nw-hint">minutes (1 to 240)</p>
              </div>
            </div>
          )}

          {error !== null && (
            <p className="nw-error" role="alert">
              {error}
            </p>
          )}

          <div className="nw-settings-actions">
            <button
              type="button"
              className="nw-button-secondary"
              onClick={() => {
                changed.current = false;
                setChangeError(null);
                setPhase("choose");
                setChanging(true);
              }}
              disabled={loading || saving}
            >
              Change passphrase
            </button>
          </div>

          <section className="nw-backup" aria-labelledby="nw-backup-title">
            <h3 id="nw-backup-title" className="nw-pane-title">
              Backup and restore
            </h3>
            <p className="nw-hint">
              Export all notes to one encrypted file, or add the notes of a
              backup or text file. Restored notes are added; existing notes stay
              unchanged.
            </p>
            <div className="nw-settings-actions">
              <button
                type="button"
                className="nw-button-secondary"
                onClick={() => void handleExportAll()}
                disabled={loading || saving || backupWorking}
              >
                {backupWorking ? "Working..." : "Export all notes"}
              </button>
              <button
                type="button"
                className="nw-button-secondary"
                onClick={() => void handleRestore()}
                disabled={loading || saving || backupWorking}
              >
                Restore from file
              </button>
            </div>
            {backupMessage !== null && (
              <p className="nw-status" role="status">
                {backupMessage}
              </p>
            )}
            {backupError !== null && (
              <p className="nw-error" role="alert">
                {backupError}
              </p>
            )}
          </section>
        </>
      )}

      {changing && (
        <div className="nw-change">
          <h2>Change passphrase</h2>

          {phase === "choose" && (
            <>
              <p>
                Changing the passphrase creates a new set of words. Your notes
                stay the same; only the words that open them change.
              </p>
              <div className="nw-field">
                <label htmlFor="nw-change-count">Number of words</label>
                <select
                  id="nw-change-count"
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
              <div className="nw-settings-actions">
                <button
                  type="button"
                  className="nw-button-primary"
                  onClick={() => void handleStart()}
                  disabled={working}
                >
                  Start
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
                      {!confirmed && (
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
                    <Copy size={14} aria-hidden="true" />
                    Copy words
                  </button>
                  <button
                    type="button"
                    className="nw-button-secondary"
                    onClick={resetChange}
                    disabled={working}
                  >
                    Start over
                  </button>
                </div>
                <p className="nw-muted">
                  Other programs can read the clipboard while the words are on
                  it.
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

              <div className="nw-settings-actions">
                <button
                  type="button"
                  className="nw-button-primary"
                  onClick={() => void handleConfirm()}
                  disabled={!savedTick || working}
                >
                  Continue
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

          {phase === "verify" && (
            <>
              <p>
                Type the new words again, and your previous words below, to
                confirm the change.
              </p>
              <div className="nw-field">
                <label htmlFor="nw-new-words">Your new words</label>
                <textarea
                  id="nw-new-words"
                  className="nw-textarea"
                  rows={3}
                  value={typedNew}
                  onChange={(event) => setTypedNew(event.target.value)}
                  spellCheck={false}
                  autoComplete="off"
                  autoCorrect="off"
                  autoCapitalize="off"
                  dir="ltr"
                  aria-label="Your new words"
                  disabled={working}
                />
              </div>
              <div className="nw-field">
                <label htmlFor="nw-previous-words">Your previous words</label>
                <textarea
                  id="nw-previous-words"
                  className="nw-textarea"
                  rows={3}
                  value={typedOld}
                  onChange={(event) => setTypedOld(event.target.value)}
                  spellCheck={false}
                  autoComplete="off"
                  autoCorrect="off"
                  autoCapitalize="off"
                  dir="ltr"
                  aria-label="Your previous words"
                  disabled={working}
                />
              </div>
              <div className="nw-settings-actions">
                <button
                  type="button"
                  className="nw-button-primary"
                  onClick={() => void handleChange()}
                  disabled={
                    working ||
                    typedNew.trim() === "" ||
                    typedOld.trim() === ""
                  }
                >
                  {working
                    ? "Changing... this takes about a second"
                    : "Change passphrase"}
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

          {phase === "done" && (
            <>
              <p className="nw-muted" role="status">
                Passphrase changed.
              </p>
              {!oldBackupGone && (
                <p>The previous backup file was kept.</p>
              )}
              <div className="nw-settings-actions">
                <button
                  type="button"
                  className="nw-button-primary"
                  onClick={handleDone}
                  disabled={working}
                >
                  Done
                </button>
              </div>
            </>
          )}

          {changeError !== null && (
            <p className="nw-error" role="alert">
              {changeError}
            </p>
          )}
        </div>
      )}
    </div>
  );
}
