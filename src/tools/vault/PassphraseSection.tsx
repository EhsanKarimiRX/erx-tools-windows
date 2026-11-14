import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Copy, RefreshCw } from "lucide-react";
import ProgressBar from "../../shared/ProgressBar";
import type { PassphraseInfo, RefreshResult } from "./types";
import { CLIPBOARD_CLEAR_MS, DEFAULT_WORDS, MAX_WORDS, MIN_WORDS, errorText } from "./types";

interface PassphraseSectionProps {
  /** True while an encryption is running: nothing may change. */
  locked: boolean;
  /** Tells the parent whether the user confirmed "I saved the passphrase". */
  onConfirmedChange: (confirmed: boolean) => void;
}

const WORD_COUNTS: number[] = Array.from(
  { length: MAX_WORDS - MIN_WORDS + 1 },
  (_unused, index) => MIN_WORDS + index,
);

/** Seconds left until the clipboard is wiped; counts down once a second. */
function ClipboardCountdown() {
  const [left, setLeft] = useState(CLIPBOARD_CLEAR_MS / 1000);
  useEffect(() => {
    const id = window.setInterval(() => {
      setLeft((value) => Math.max(0, value - 1));
    }, 1000);
    return () => window.clearInterval(id);
  }, []);
  return (
    <>
      {left} {left === 1 ? "second" : "seconds"}
    </>
  );
}

/** Best effort: the browser cannot tell whether the text is still ours. */
async function wipeClipboard(): Promise<void> {
  try {
    await navigator.clipboard.writeText("");
  } catch {
    // Nothing more can be done.
  }
}

/** Passphrase generation: random words from Rust, replace single words (at
 * most 3 times each), copy, and the mandatory "I saved it" confirmation.
 * The words are only for display; encryption reads them from Rust. */
export default function PassphraseSection({
  locked,
  onConfirmedChange,
}: PassphraseSectionProps) {
  const [count, setCount] = useState(DEFAULT_WORDS);
  const [info, setInfo] = useState<PassphraseInfo | null>(null);
  const [confirmed, setConfirmed] = useState(false);
  const [working, setWorking] = useState(false);
  const [copied, setCopied] = useState(false);
  const [copyRound, setCopyRound] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const clearTimer = useRef<number | null>(null);

  // Leaving the screen: wipe a pending clipboard copy right away.
  useEffect(() => {
    return () => {
      if (clearTimer.current !== null) {
        window.clearTimeout(clearTimer.current);
        clearTimer.current = null;
        void wipeClipboard();
      }
    };
  }, []);

  const generate = async () => {
    setWorking(true);
    setError(null);
    try {
      const next = await invoke<PassphraseInfo>("vault_generate_passphrase", { count });
      setInfo(next);
      setConfirmed(false);
      setCopied(false);
      onConfirmedChange(false);
    } catch (problem) {
      setError(errorText(problem));
    } finally {
      setWorking(false);
    }
  };

  const refresh = async (slot: number) => {
    setWorking(true);
    setError(null);
    try {
      const result = await invoke<RefreshResult>("vault_refresh_word", { slot });
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
      setError(errorText(problem));
    } finally {
      setWorking(false);
    }
  };

  const confirm = async () => {
    setWorking(true);
    setError(null);
    try {
      await invoke("vault_confirm_saved");
      setConfirmed(true);
      onConfirmedChange(true);
    } catch (problem) {
      setError(errorText(problem));
    } finally {
      setWorking(false);
    }
  };

  const copy = async () => {
    if (info === null) return;
    try {
      await navigator.clipboard.writeText(info.words.join(" "));
      if (clearTimer.current !== null) window.clearTimeout(clearTimer.current);
      setCopied(true);
      setCopyRound((round) => round + 1);
      clearTimer.current = window.setTimeout(() => {
        clearTimer.current = null;
        setCopied(false);
        void wipeClipboard();
      }, CLIPBOARD_CLEAR_MS);
    } catch {
      setError("Could not copy to the clipboard. Write the words down instead.");
    }
  };

  const frozen = locked || working;

  return (
    <div className="vt-pass">
      <div className="vt-actions">
        <label className="vt-inline" htmlFor="vt-word-count">
          Words
        </label>
        <select
          id="vt-word-count"
          className="vt-select"
          value={count}
          onChange={(event) => setCount(Number(event.target.value))}
          disabled={frozen}
        >
          {WORD_COUNTS.map((value) => (
            <option key={value} value={value}>
              {value}
              {value === DEFAULT_WORDS ? " (recommended)" : ""}
            </option>
          ))}
        </select>
        <button
          type="button"
          className={info === null ? "button-primary" : "button-secondary"}
          onClick={() => void generate()}
          disabled={frozen}
        >
          {info === null ? "Generate passphrase" : "Generate a new passphrase"}
        </button>
      </div>

      {error !== null && (
        <p className="vt-error" role="alert">
          {error}
        </p>
      )}

      {info !== null && (
        <>
          <ol className="vt-words">
            {info.words.map((word, slot) => {
              const left = info.replacementsLeft[slot] ?? 0;
              return (
                <li key={slot} className="vt-word">
                  <span className="vt-word-number">{slot + 1}</span>
                  <span className="vt-word-text">{word}</span>
                  <button
                    type="button"
                    className="vt-word-refresh"
                    onClick={() => void refresh(slot)}
                    disabled={confirmed || frozen || left === 0}
                    title={
                      left === 0
                        ? "No replacements left for this word"
                        : `Replace this word (${left} left)`
                    }
                    aria-label={`Replace word ${slot + 1}, ${left} replacements left`}
                  >
                    <RefreshCw size={14} aria-hidden="true" />
                    <span className="vt-word-left">{left}</span>
                  </button>
                </li>
              );
            })}
          </ol>

          <div className="vt-strength">
            <ProgressBar
              value={Math.min(info.strength.bits, 128)}
              max={128}
              label="Passphrase strength"
            />
            <p className="vt-note">
              {info.words.length} words are about {Math.round(info.strength.bits)} bits of
              strength. Against a quantum search (Grover) about{" "}
              {Math.round(info.strength.groverBits)} bits remain; the slow key
              derivation (Argon2id) makes guessing far harder in practice.
            </p>
            {info.strength.isWeak && (
              <p className="vt-warning">
                Fewer than 8 words is weak. Use at least 8 words for anything important.
              </p>
            )}
          </div>

          <p className="vt-note">
            Type the words exactly as shown, with the hyphen where a word has one (for
            example yo-yo). Capital letters and extra spaces do not matter.
          </p>

          <div className="vt-warning" role="note">
            <p>
              <strong>Write this passphrase down now.</strong> If you lose it, your
              encrypted file can never be opened again. It is shown only on this screen,
              never saved on disk, and nobody can reset it.
            </p>
          </div>

          <div className="vt-actions">
            <button
              type="button"
              className="button-secondary"
              onClick={() => void copy()}
              disabled={frozen}
            >
              <Copy size={14} aria-hidden="true" /> Copy words
            </button>
            {copied && (
              <div className="vt-copied" role="note">
                <p>
                  <strong>Copied.</strong>{" "}
                  <span className="vt-blink">
                    The clipboard will be cleared in <ClipboardCountdown key={copyRound} />.
                  </span>{" "}
                  After that the words can no longer be pasted, so paste them somewhere
                  safe first (a note or a document) or write them down.
                </p>
              </div>
            )}
          </div>

          {confirmed ? (
            <p className="vt-ok">Passphrase confirmed. It can no longer be changed.</p>
          ) : (
            <div className="vt-actions">
              <button
                type="button"
                className="button-primary"
                onClick={() => void confirm()}
                disabled={frozen}
              >
                I saved the passphrase
              </button>
            </div>
          )}
        </>
      )}
    </div>
  );
}
