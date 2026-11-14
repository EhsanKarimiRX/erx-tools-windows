import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import DropZone from "../../shared/DropZone";
import OperationStatus from "./OperationStatus";
import OutputFolder from "./OutputFolder";
import { useVaultProgress } from "./progress";
import type { DecryptPlan, DecryptResult } from "./types";
import { errorText, formatBytes } from "./types";

interface DecryptPanelProps {
  /** False while the other tab is shown (the panel stays mounted). */
  active: boolean;
  onBusyChange: (busy: boolean) => void;
}

/** Decrypt tab: choose the first part, type the words, run with progress. */
export default function DecryptPanel({ active, onBusyChange }: DecryptPanelProps) {
  const [firstPart, setFirstPart] = useState<string | null>(null);
  const [outputDir, setOutputDir] = useState<string | null>(null);
  const [overwrite, setOverwrite] = useState(false);
  const [plan, setPlan] = useState<DecryptPlan | null>(null);
  const [planError, setPlanError] = useState<string | null>(null);
  const [words, setWords] = useState("");
  const [running, setRunning] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [result, setResult] = useState<DecryptResult | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const { view, start, reset } = useVaultProgress();

  const chooseFile = useCallback((path: string) => {
    setFirstPart(path);
    setOverwrite(false);
    setResult(null);
    setMessage(null);
    setNotice(null);
  }, []);

  const browse = useCallback(async () => {
    try {
      const picked = await invoke<string | null>("vault_pick_vault_file");
      if (picked) chooseFile(picked);
    } catch (problem) {
      setMessage(`Could not open the file dialog: ${errorText(problem)}`);
    }
  }, [chooseFile]);

  const onDrop = useCallback(
    (paths: string[]) => {
      const first = paths[0];
      if (first) chooseFile(first);
    },
    [chooseFile],
  );

  // Facts about the chosen file (parts found, name of the restored file).
  useEffect(() => {
    if (firstPart === null) return;
    let stale = false;
    invoke<DecryptPlan>("vault_prepare_decrypt", { firstPart, outputDir })
      .then((next) => {
        if (stale) return;
        setPlan(next);
        setPlanError(null);
      })
      .catch((problem) => {
        if (stale) return;
        setPlan(null);
        setPlanError(errorText(problem));
      });
    return () => {
      stale = true;
    };
  }, [firstPart, outputDir]);

  const pickFolder = async () => {
    try {
      const folder = await invoke<string | null>("vault_pick_folder");
      if (folder) {
        setOutputDir(folder);
        setOverwrite(false);
      }
    } catch (problem) {
      setMessage(`Could not open the folder dialog: ${errorText(problem)}`);
    }
  };

  const run = async () => {
    if (plan === null || firstPart === null || running) return;
    setRunning(true);
    onBusyChange(true);
    setCancelling(false);
    setMessage(null);
    setNotice(null);
    const channel = start();
    try {
      const outcome = await invoke<DecryptResult>("vault_decrypt_file", {
        onProgress: channel,
        request: { firstPart, outputDir, words, overwrite },
      });
      if (outcome.cancelled) {
        setNotice("Cancelled. No files were created.");
      } else {
        setResult(outcome);
        // The words are no longer needed on screen.
        setWords("");
      }
    } catch (problem) {
      // The words stay, so a typing mistake can be fixed.
      setMessage(errorText(problem));
    } finally {
      setRunning(false);
      setCancelling(false);
      onBusyChange(false);
      reset();
    }
  };

  const cancel = async () => {
    if (!running) return;
    setCancelling(true);
    try {
      await invoke("vault_cancel");
    } catch (problem) {
      setMessage(`Could not cancel: ${errorText(problem)}`);
      setCancelling(false);
    }
  };

  const reveal = async (path: string) => {
    try {
      await invoke("reveal_outputs", { paths: [path] });
    } catch (problem) {
      setMessage(`Could not open the folder: ${errorText(problem)}`);
    }
  };

  const startOver = () => {
    setFirstPart(null);
    setPlan(null);
    setPlanError(null);
    setOutputDir(null);
    setOverwrite(false);
    setWords("");
    setResult(null);
    setMessage(null);
    setNotice(null);
  };

  const hasFile = firstPart !== null;
  const shownPlan = hasFile ? plan : null;
  const blockedByExisting = false;
  const canDecrypt =
    shownPlan !== null && words.trim() !== "" && !running && !blockedByExisting;

  if (result !== null) {
    return (
      <div className="vt-panel" hidden={!active}>
        <div className="vt-card vt-done">
          <h2 className="vt-card-title">Decrypted successfully</h2>
          <p>The original file ({formatBytes(result.outputBytes)}) was restored:</p>
          <p className="vt-path">{result.outputPath}</p>
          <p className="vt-note">
            Its integrity was checked: it matches the file that was encrypted.
          </p>
          <div className="vt-actions">
            <button
              type="button"
              className="button-secondary"
              onClick={() => void reveal(result.outputPath)}
            >
              Show in folder
            </button>
            <button type="button" className="button-primary" onClick={startOver}>
              Decrypt another file
            </button>
          </div>
          {message !== null && (
            <p className="vt-error" role="alert">
              {message}
            </p>
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="vt-panel" hidden={!active}>
      <section className="vt-card">
        <h2 className="vt-card-title">1. Choose the encrypted file</h2>
        {!hasFile ? (
          <DropZone
            label="Drag the .erx file here"
            hint="For a split file choose the first part (the one ending in .001)"
            browseLabel="Choose file"
            onBrowse={() => void browse()}
            onFiles={onDrop}
            disabled={!active || running}
          />
        ) : (
          <div className="vt-file">
            <div>
              <p className="vt-file-name">{shownPlan?.fileName ?? firstPart}</p>
              {shownPlan !== null && (
                <p className="vt-note">
                  {shownPlan.partCount === 1
                    ? "1 file"
                    : `${shownPlan.partCount} parts found`}
                  , {formatBytes(shownPlan.totalBytes)}
                </p>
              )}
            </div>
            <div className="vt-actions">
              <button
                type="button"
                className="button-secondary"
                onClick={() => void browse()}
                disabled={running}
              >
                Choose another file
              </button>
              <button
                type="button"
                className="button-secondary"
                onClick={startOver}
                disabled={running}
              >
                Remove file
              </button>
            </div>
          </div>
        )}
        {planError !== null && (
          <p className="vt-error" role="alert">
            {planError}
          </p>
        )}
      </section>

      {hasFile && shownPlan !== null && (
        <>
          <section className="vt-card">
            <h2 className="vt-card-title">2. Enter the passphrase</h2>
            <label className="vt-label" htmlFor="vt-words">
              The words of your passphrase
            </label>
            <textarea
              id="vt-words"
              className="vt-textarea"
              rows={3}
              value={words}
              onChange={(event) => setWords(event.target.value)}
              placeholder="Type or paste the words, separated by spaces"
              spellCheck={false}
              autoComplete="off"
              autoCapitalize="off"
              disabled={running}
            />
            <p className="vt-note">
              Capital letters and extra spaces do not matter. Words with a hyphen (like
              yo-yo) must keep it.
            </p>
          </section>

          <section className="vt-card">
            <h2 className="vt-card-title">3. Decrypt</h2>
            <fieldset className="vt-fieldset" disabled={running}>
              <OutputFolder
                folder={shownPlan.outputDir}
                custom={outputDir !== null}
                disabled={running}
                onPick={() => void pickFolder()}
                onReset={() => {
                  setOutputDir(null);
                  setOverwrite(false);
                }}
              />
              <p className="vt-note">
                The restored file gets its original name and type (shown when it is done). If
                that name is already used in the folder, a number is added, so nothing is
                overwritten.
              </p>
            </fieldset>

            {running ? (
              <OperationStatus
                view={view}
                workingLabel="Decrypting..."
                cancelling={cancelling}
                onCancel={() => void cancel()}
              />
            ) : (
              <div className="vt-actions">
                <button
                  type="button"
                  className="button-primary"
                  onClick={() => void run()}
                  disabled={!canDecrypt}
                >
                  Decrypt file
                </button>
              </div>
            )}
            {notice !== null && <p className="vt-note">{notice}</p>}
            {message !== null && (
              <p className="vt-error" role="alert">
                {message}
              </p>
            )}
          </section>
        </>
      )}
      {!hasFile && message !== null && (
        <p className="vt-error" role="alert">
          {message}
        </p>
      )}
    </div>
  );
}
