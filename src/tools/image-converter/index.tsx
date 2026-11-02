import { useCallback, useState } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import DropZone from "../../shared/DropZone";
import ProgressBar from "../../shared/ProgressBar";
import FileList from "./FileList";
import OptionsPanel from "./OptionsPanel";
import type { BatchSummary, ConvertOptions, FileEntry, InspectedFile, ProgressEvent } from "./types";
import { MAX_FILES, SUPPORTED_INPUT, formatBytes } from "./types";
import "./image-converter.css";

/** Phase of the tool screen. */
type Phase = "empty" | "ready" | "converting" | "done";

/** Turns anything thrown by invoke into a short display string. */
function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

/** "a — reason; b — reason — and 3 more" */
function summarize(items: string[]): string {
  const shown = items.slice(0, 3).join("; ");
  return items.length > 3 ? `${shown} — and ${items.length - 3} more` : shown;
}

/** The folder part of an absolute path (Windows or POSIX separators). */
function parentFolder(path: string): string | null {
  const index = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  return index > 0 ? path.slice(0, index) : null;
}

/** Image Converter: drop or pick files, review the list, choose options,
 * convert with live progress, cancel, open the results. */
export default function ImageConverter() {
  const [phase, setPhase] = useState<Phase>("empty");
  const [entries, setEntries] = useState<FileEntry[]>([]);
  const [notes, setNotes] = useState<string[]>([]);
  const [options, setOptions] = useState<ConvertOptions>({
    format: "jpeg",
    jpegQuality: 85,
    background: "#ffffff",
    outputFolder: null,
  });
  const [busy, setBusy] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [summary, setSummary] = useState<BatchSummary | null>(null);

  const converting = phase === "converting";
  const locked = busy; // list edits + options are locked while a batch runs

  /** Classifies dropped/picked paths in Rust and merges them into the list. */
  const addFiles = useCallback(
    async (paths: string[]) => {
      if (paths.length === 0 || locked) return;
      setBusy(true);
      try {
        const inspected = await invoke<InspectedFile[]>("inspect_files", { paths });

        const seen = new Set(entries.map((entry) => entry.path.toLowerCase()));
        const next = [...entries];
        const rejected: string[] = [];
        let duplicates = 0;
        let overflow = 0;

        for (const item of inspected) {
          const key = item.path.toLowerCase();
          if (seen.has(key)) {
            duplicates += 1;
            continue;
          }
          if (next.length >= MAX_FILES) {
            overflow += 1;
            continue;
          }
          if (item.kind !== "image") {
            rejected.push(`${item.fileName} — ${item.reason ?? "not supported"}`);
            continue;
          }
          seen.add(key);
          next.push({
            path: item.path,
            fileName: item.fileName,
            sizeBytes: item.sizeBytes,
            format: item.format ?? "?",
            animated: item.animated,
            status: "ready",
            note: item.animated ? "animated: first frame only" : null,
          });
        }

        const newNotes: string[] = [];
        if (duplicates > 0) {
          newNotes.push(
            `Skipped ${duplicates} duplicate ${duplicates === 1 ? "file" : "files"} already in the list.`,
          );
        }
        if (overflow > 0) {
          newNotes.push(
            `File limit is ${MAX_FILES}; ${overflow} more ${overflow === 1 ? "file was" : "files were"} ignored.`,
          );
        }
        if (rejected.length > 0) {
          newNotes.push(`Not added: ${summarize(rejected)}`);
        }

        setEntries(next);
        setNotes(newNotes);
        setPhase(next.length > 0 ? "ready" : "empty");
      } catch (error) {
        setNotes([`Could not read the files: ${errorText(error)}`]);
      } finally {
        setBusy(false);
      }
    },
    [entries, locked],
  );

  const browse = useCallback(async () => {
    if (locked) return;
    try {
      const picked = await invoke<string[] | null>("pick_image_files");
      if (picked) await addFiles(picked);
    } catch (error) {
      setNotes([`Could not open the file dialog: ${errorText(error)}`]);
    }
  }, [addFiles, locked]);

  const removeFile = (path: string) => {
    const next = entries.filter((entry) => entry.path !== path);
    setEntries(next);
    if (next.length === 0) {
      setPhase("empty");
      setNotes([]);
    }
  };

  const clearAll = () => {
    setEntries([]);
    setNotes([]);
    setSummary(null);
    setPhase("empty");
  };

  /** Runs the batch: every status change arrives in list order through the
   * channel; the invoke resolves with the final summary. */
  const convert = async () => {
    if (entries.length === 0 || busy) return;
    setBusy(true);
    setCancelling(false);
    setSummary(null);
    setPhase("converting");
    setEntries((prev) =>
      prev.map((entry) => ({ ...entry, status: "waiting", outputPath: undefined, outputSize: undefined })),
    );
    try {
      const channel = new Channel<ProgressEvent>();
      channel.onmessage = (event) => {
        setEntries((prev) =>
          prev.map((entry, index) => {
            if (index !== event.index) return entry;
            return {
              ...entry,
              status: event.status,
              note: event.reason ?? entry.note,
              outputPath: event.outputPath ?? undefined,
              outputSize: event.outputSize ?? undefined,
            };
          }),
        );
      };
      const result = await invoke<BatchSummary>("convert_files", {
        paths: entries.map((entry) => entry.path),
        options: {
          format: options.format,
          jpegQuality: options.jpegQuality,
          background: options.background,
          outputFolder: options.outputFolder,
        },
        onProgress: channel,
      });
      setSummary(result);
      setPhase("done");
    } catch (error) {
      setNotes([`Conversion could not start: ${errorText(error)}`]);
      setEntries((prev) => prev.map((entry) => ({ ...entry, status: "ready" })));
      setPhase("ready");
    } finally {
      setBusy(false);
      setCancelling(false);
    }
  };

  const cancel = async () => {
    if (!converting) return;
    setCancelling(true);
    try {
      await invoke("cancel_conversion");
    } catch (error) {
      setNotes([`Could not cancel: ${errorText(error)}`]);
    }
  };

  /** Back to the ready state, keeping the file list for another round. */
  const convertMore = () => {
    setSummary(null);
    setEntries((prev) => prev.map((entry) => ({ ...entry, status: "ready" })));
    setPhase("ready");
  };

  const outputPaths = entries
    .map((entry) => entry.outputPath)
    .filter((path): path is string => Boolean(path));

  const allInOneFolder =
    outputPaths.length > 0 &&
    outputPaths.every((path) => parentFolder(path) === parentFolder(outputPaths[0]));

  const openOutputFolder = async () => {
    try {
      await invoke("reveal_outputs", { paths: outputPaths });
    } catch (error) {
      setNotes([`Could not open the folder: ${errorText(error)}`]);
    }
  };

  const total = entries.length;
  const finished = entries.filter((entry) =>
    ["done", "failed", "skipped"].includes(entry.status),
  ).length;
  const inputBytes = entries.reduce((sum, entry) => sum + entry.sizeBytes, 0);

  return (
    <div className="ic">
      {phase === "empty" ? (
        <div className="ic-empty">
          <DropZone
            label="Drag images here"
            hint={`Formats: ${SUPPORTED_INPUT} — up to ${MAX_FILES} files`}
            onFiles={addFiles}
            onBrowse={browse}
            disabled={busy}
          />
          <p className="ic-note ic-center">
            Mixed formats are fine — all files are converted to the one format
            you choose.
          </p>
        </div>
      ) : (
        <div className="ic-layout">
          <div className="ic-main">
            <DropZone
              className="ic-dropzone-compact"
              label="Drop more files here"
              hint={`Formats: ${SUPPORTED_INPUT}`}
              onFiles={addFiles}
              onBrowse={browse}
              disabled={locked}
            />

            {notes.length > 0 && (
              <ul className="ic-notes" aria-live="polite">
                {notes.map((note) => (
                  <li key={note}>{note}</li>
                ))}
              </ul>
            )}

            <FileList
              entries={entries}
              onRemove={removeFile}
              onClear={clearAll}
              disabled={locked}
            />

            {converting && (
              <div className="ic-progress-row">
                <ProgressBar
                  value={finished}
                  max={total}
                  label="Overall conversion progress"
                />
                <button
                  type="button"
                  className="button-secondary"
                  onClick={cancel}
                  disabled={cancelling}
                >
                  {cancelling ? "Cancelling…" : "Cancel"}
                </button>
              </div>
            )}

            {phase === "ready" && (
              <div className="ic-actions">
                <button
                  type="button"
                  className="button-primary"
                  onClick={convert}
                  disabled={busy || entries.length === 0}
                >
                  Convert {entries.length} file{entries.length === 1 ? "" : "s"}
                </button>
              </div>
            )}

            {phase === "done" && summary && (
              <div className="ic-done">
                <p className="ic-summary">
                  Converted {summary.converted} · Failed {summary.failed} ·
                  Skipped {summary.skipped} · {formatBytes(inputBytes)} →{" "}
                  {formatBytes(summary.outputBytes)}
                </p>
                <div className="ic-actions">
                  <button
                    type="button"
                    className="button-primary"
                    onClick={convertMore}
                  >
                    Convert more
                  </button>
                  {allInOneFolder && (
                    <button
                      type="button"
                      className="button-secondary"
                      onClick={openOutputFolder}
                    >
                      Open output folder
                    </button>
                  )}
                </div>
              </div>
            )}
          </div>

          <OptionsPanel options={options} onChange={setOptions} disabled={locked} />
        </div>
      )}
    </div>
  );
}
