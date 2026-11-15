import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import DropZone from "../../shared/DropZone";
import OperationStatus from "./OperationStatus";
import OutputFolder from "./OutputFolder";
import PassphraseSection from "./PassphraseSection";
import { useVaultProgress } from "./progress";
import type { EncryptPlan, EncryptResult, PartSizeId } from "./types";
import { PART_SIZE_OPTIONS, errorText, formatBytes } from "./types";

interface EncryptPanelProps {
  /** False while the other tab is shown (the panel stays mounted). */
  active: boolean;
  onBusyChange: (busy: boolean) => void;
}

/** Both drop boxes show the highlight while something is dragged over the
 * window; the drop itself is handled once, by the first box. */
const ignoreDrop = () => undefined;

/** "a, b, c and 2 more" */
function listFiles(paths: string[]): string {
  const shown = paths.slice(0, 3).join(", ");
  return paths.length > 3 ? `${shown} and ${paths.length - 3} more` : shown;
}

/** Encrypt tab: choose a file, options, passphrase, run with progress. */
export default function EncryptPanel({ active, onBusyChange }: EncryptPanelProps) {
  const [sourcePath, setSourcePath] = useState<string | null>(null);
  const [outputDir, setOutputDir] = useState<string | null>(null);
  const [partSize, setPartSize] = useState<PartSizeId>("off");
  const [compress, setCompress] = useState(true);
  const [verify, setVerify] = useState(false);
  const [overwrite, setOverwrite] = useState(false);
  const [plan, setPlan] = useState<EncryptPlan | null>(null);
  const [planError, setPlanError] = useState<string | null>(null);
  const [confirmed, setConfirmed] = useState(false);
  const [passphraseKey, setPassphraseKey] = useState(0);
  const [running, setRunning] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [result, setResult] = useState<EncryptResult | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const compressTouched = useRef(false);
  const { view, start, reset } = useVaultProgress();

  const chooseFile = useCallback((path: string) => {
    compressTouched.current = false;
    setSourcePath(path);
    setOverwrite(false);
    setResult(null);
    setMessage(null);
    setNotice(null);
  }, []);

  const browse = useCallback(async () => {
    try {
      const picked = await invoke<string | null>("vault_pick_source_file");
      if (picked) chooseFile(picked);
    } catch (problem) {
      setMessage(`Could not open the file dialog: ${errorText(problem)}`);
    }
  }, [chooseFile]);

  const browseFolder = useCallback(async () => {
    try {
      const picked = await invoke<string | null>("vault_pick_folder");
      if (picked) chooseFile(picked);
    } catch (problem) {
      setMessage(`Could not open the folder dialog: ${errorText(problem)}`);
    }
  }, [chooseFile]);

  const onDrop = useCallback(
    (paths: string[]) => {
      const first = paths[0];
      if (first) chooseFile(first);
    },
    [chooseFile],
  );

  // Facts about the chosen file (size, output name, conflicts, free space).
  useEffect(() => {
    if (sourcePath === null) return;
    let stale = false;
    invoke<EncryptPlan>("vault_prepare_encrypt", {
      path: sourcePath,
      outputDir,
      partSize,
    })
      .then((next) => {
        if (stale) return;
        setPlan(next);
        setPlanError(null);
        setCompress((previous) =>
          compressTouched.current ? previous : next.compressByDefault,
        );
      })
      .catch((problem) => {
        if (stale) return;
        setPlan(null);
        setPlanError(errorText(problem));
      });
    return () => {
      stale = true;
    };
  }, [sourcePath, outputDir, partSize]);

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
    if (plan === null || sourcePath === null || !confirmed || running) return;
    setRunning(true);
    onBusyChange(true);
    setCancelling(false);
    setMessage(null);
    setNotice(null);
    const channel = start();
    try {
      const outcome = await invoke<EncryptResult>("vault_encrypt_file", {
        onProgress: channel,
        request: {
          path: sourcePath,
          outputDir,
          partSize,
          compress,
          overwrite,
          verify,
        },
      });
      if (outcome.cancelled) {
        setNotice(
          "Cancelled. No files were created, and your passphrase is still available.",
        );
      } else {
        setResult(outcome);
        // The passphrase was wiped in Rust; show a fresh section next time.
        setConfirmed(false);
        setPassphraseKey((key) => key + 1);
      }
    } catch (problem) {
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
    setSourcePath(null);
    setPlan(null);
    setPlanError(null);
    setOutputDir(null);
    setOverwrite(false);
    setResult(null);
    setMessage(null);
    setNotice(null);
  };

  const hasFile = sourcePath !== null;
  const shownPlan = hasFile ? plan : null;
  const lowSpace =
    shownPlan !== null &&
    shownPlan.freeBytes !== null &&
    shownPlan.freeBytes < shownPlan.requiredBytes;
  const hasConflicts = shownPlan !== null && shownPlan.conflicts.length > 0;
  const canEncrypt =
    shownPlan !== null &&
    confirmed &&
    !running &&
    !lowSpace &&
    (!hasConflicts || overwrite);

  if (result !== null) {
    const first = result.parts[0];
    return (
      <div className="vt-panel" hidden={!active}>
        <div className="vt-card vt-done">
          <h2 className="vt-card-title">Encrypted successfully</h2>
          <p>
            {result.parts.length === 1
              ? "1 file was created"
              : `${result.parts.length} part files were created`}
            : {formatBytes(result.inputBytes)} became {formatBytes(result.outputBytes)}.
          </p>
          {first !== undefined && <p className="vt-path">{first}</p>}
          <p className="vt-note">
            {result.verified
              ? "Checked after writing: the new file decrypts correctly."
              : "The new file was not checked after writing."}
          </p>
          <p className="vt-note">
            Your original file was not changed or deleted. Keep the passphrase safe:
            without it the file cannot be opened.
          </p>
          <div className="vt-actions">
            {first !== undefined && (
              <button
                type="button"
                className="button-secondary"
                onClick={() => void reveal(first)}
              >
                Show in folder
              </button>
            )}
            <button type="button" className="button-primary" onClick={startOver}>
              Encrypt another file
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
        <h2 className="vt-card-title">1. Choose a file or a folder</h2>
        {!hasFile ? (
          <div className="vt-pick">
            <DropZone
              label="Drag a file here"
              hint="Any file type - tested up to 8 GB"
              browseLabel="Choose file"
              onBrowse={() => void browse()}
              onFiles={onDrop}
              disabled={!active || running}
            />
            <DropZone
              label="Drag a folder here"
              hint="A whole folder with everything in it"
              browseLabel="Choose folder"
              onBrowse={() => void browseFolder()}
              onFiles={ignoreDrop}
              disabled={!active || running}
            />
          </div>
        ) : (
          <div className="vt-file">
            <div>
              <p className="vt-file-name">{shownPlan?.fileName ?? sourcePath}</p>
              {shownPlan !== null && (
                <p className="vt-note">
                  {shownPlan.isFolder
                    ? `Folder: ${shownPlan.itemCount} files, ${formatBytes(shownPlan.sizeBytes)}`
                    : formatBytes(shownPlan.sizeBytes)}
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
                  onClick={() => void browseFolder()}
                  disabled={running}
                >
                  Choose another folder
              </button>
              <button
                type="button"
                className="button-secondary"
                onClick={startOver}
                disabled={running}
              >
                {shownPlan?.isFolder ? "Remove folder" : "Remove file"}
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
            <h2 className="vt-card-title">2. Options</h2>
            <fieldset className="vt-fieldset" disabled={running}>
              <div className="vt-field">
                <label className="vt-label" htmlFor="vt-part-size">
                  Split the encrypted file
                </label>
                <select
                  id="vt-part-size"
                  className="vt-select"
                  value={partSize}
                  onChange={(event) => setPartSize(event.target.value as PartSizeId)}
                >
                  {PART_SIZE_OPTIONS.map((option) => (
                    <option key={option.id} value={option.id}>
                      {option.label}
                    </option>
                  ))}
                </select>
                <p className="vt-note">
                  Parts are named name.erx.001, .002 and so on. A drive formatted as
                  FAT32 cannot hold a single file of 4 GB or more.
                </p>
                {shownPlan.suggestSplit && partSize === "off" && (
                  <p className="vt-warning">
                    This file is larger than 2 GB. Splitting it into parts is
                    recommended.
                  </p>
                )}
              </div>

              <label className="vt-check">
                <input
                  type="checkbox"
                  checked={compress}
                  onChange={(event) => {
                    compressTouched.current = true;
                    setCompress(event.target.checked);
                  }}
                />
                <span>
                  Compress before encrypting
                  <span className="vt-note vt-block">
                    {shownPlan.compressByDefault
                      ? "Recommended for this file type."
                      : "This file type is usually compressed already, so compressing again would only waste time."}
                  </span>
                </span>
              </label>

              <label className="vt-check">
                <input
                  type="checkbox"
                  checked={verify}
                  onChange={(event) => setVerify(event.target.checked)}
                />
                <span>
                  Check the new file after writing
                  <span className="vt-note vt-block">
                    Decrypts the result once to prove it is intact. Takes roughly as
                    long as encrypting again.
                  </span>
                </span>
              </label>

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
              {shownPlan.isFolder && (
                <p className="vt-note">
                  Folder: every file and subfolder is stored, including empty ones. File dates
                  and attributes (hidden, read-only) are not kept, and links or shortcuts inside
                  the folder are not supported. Do not change the folder while it is encrypted.
                </p>
              )}
              <p className="vt-note">Output: {shownPlan.outputBasePath}</p>
              <p className="vt-note">
                The file type is not part of the output name. The original name (with its
                type) is stored inside the encrypted data and restored when you decrypt.
              </p>

              {hasConflicts && (
                <div className="vt-warning" role="alert">
                  <p>
                    These files already exist and would be replaced:{" "}
                    {listFiles(shownPlan.conflicts)}
                  </p>
                  <label className="vt-check">
                    <input
                      type="checkbox"
                      checked={overwrite}
                      onChange={(event) => setOverwrite(event.target.checked)}
                    />
                    <span>Replace the existing files</span>
                  </label>
                </div>
              )}

              {lowSpace && (
                <p className="vt-error" role="alert">
                  Not enough free space on the destination drive: about{" "}
                  {formatBytes(shownPlan.requiredBytes)} are needed,{" "}
                  {formatBytes(shownPlan.freeBytes ?? 0)} are free.
                </p>
              )}
            </fieldset>
          </section>

          <section className="vt-card">
            <h2 className="vt-card-title">3. Passphrase</h2>
            <PassphraseSection
              key={passphraseKey}
              locked={running}
              onConfirmedChange={setConfirmed}
            />
          </section>

          <section className="vt-card">
            <h2 className="vt-card-title">4. Encrypt</h2>
            {running ? (
              <OperationStatus
                view={view}
                workingLabel="Encrypting..."
                cancelling={cancelling}
                onCancel={() => void cancel()}
              />
            ) : (
              <div className="vt-actions">
                <button
                  type="button"
                  className="button-primary"
                  onClick={() => void run()}
                  disabled={!canEncrypt}
                >
                  Encrypt file
                </button>
                {!confirmed && (
                  <span className="vt-note">
                    Generate a passphrase and confirm that you saved it to continue.
                  </span>
                )}
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
