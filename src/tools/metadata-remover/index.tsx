import { useCallback, useState } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import DropZone from "../../shared/DropZone";
import ProgressBar from "../../shared/ProgressBar";
import FileList from "./FileList";
import OptionsPanel from "./OptionsPanel";
import type {
  BatchSummary,
  FileEntry,
  Options,
  ProgressEvent,
  ScannedFile,
} from "./types";
import { MAX_FILES, SUPPORTED_FORMATS } from "./types";
import "./metadata-remover.css";

/** Phase of the tool screen. */
type Phase = "empty" | "ready" | "cleaning" | "done";

const FINAL_STATUSES = ["done", "nothing", "failed", "skipped"];

/** Turns anything thrown by invoke into a short display string. */
function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

/** The folder part of an absolute path (Windows or POSIX separators). */
function parentFolder(path: string): string | null {
  const index = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  return index > 0 ? path.slice(0, index) : null;
}

function toEntry(scanned: ScannedFile): FileEntry {
  return {
    ...scanned,
    status: "ready",
    done: 0,
    total: 0,
    removed: [],
    pixelsVerified: false,
    outputPath: null,
    outputSize: null,
    sourceSize: null,
    note: null,
  };
}

/** A file that can be cleaned (now or again after a failure or a cancel). */
function isCleanable(entry: FileEntry): boolean {
  return (
    entry.kind === "found" &&
    (entry.status === "ready" || entry.status === "failed" || entry.status === "skipped")
  );
}

function applyEvent(entry: FileEntry, event: ProgressEvent): FileEntry {
  if (event.status === "working") {
    return { ...entry, status: "working", done: event.done, total: event.total };
  }
  if (event.status === "done") {
    return {
      ...entry,
      status: "done",
      removed: event.removed,
      kept: event.kept,
      pixelsVerified: event.pixelsVerified,
      outputPath: event.outputPath,
      outputSize: event.outputSize,
      sourceSize: event.sourceSize,
      note: null,
    };
  }
  if (event.status === "nothing") {
    return { ...entry, status: "nothing", kept: event.kept, note: event.reason };
  }
  if (event.status === "failed") {
    return { ...entry, status: "failed", note: event.reason };
  }
  return { ...entry, status: "skipped", note: event.reason };
}

/** Metadata Remover: add photos and videos, see what is hidden in them,
 * clean copies with live progress, cancel, and open the results. */
export default function MetadataRemover() {
  const [phase, setPhase] = useState<Phase>("empty");
  const [entries, setEntries] = useState<FileEntry[]>([]);
  const [notes, setNotes] = useState<string[]>([]);
  const [options, setOptions] = useState<Options>({
    keepOrientation: true,
    keepColorProfile: true,
    outputFolder: null,
  });
  const [busy, setBusy] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [summary, setSummary] = useState<BatchSummary | null>(null);
  const [expanded, setExpanded] = useState<string[]>([]);
  /** The list positions of the files of the running batch. */
  const [batch, setBatch] = useState<number[]>([]);

  const cleaning = phase === "cleaning";

  /** Scans dropped or picked files in Rust and adds them to the list. */
  const addFiles = useCallback(
    async (paths: string[]) => {
      if (paths.length === 0 || busy) return;
      setBusy(true);
      try {
        const seen = new Set(entries.map((entry) => entry.path.toLowerCase()));
        const fresh: string[] = [];
        let duplicates = 0;
        let overflow = 0;
        for (const path of paths) {
          const key = path.toLowerCase();
          if (seen.has(key)) {
            duplicates += 1;
            continue;
          }
          if (entries.length + fresh.length >= MAX_FILES) {
            overflow += 1;
            continue;
          }
          seen.add(key);
          fresh.push(path);
        }

        let added: FileEntry[] = [];
        if (fresh.length > 0) {
          const scanned = await invoke<ScannedFile[]>("mr_scan_files", {
            paths: fresh,
            options,
          });
          added = scanned.map(toEntry);
        }

        const next = [...entries, ...added];
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
        setEntries(next);
        setNotes(newNotes);
        setSummary(null);
        setPhase(next.length > 0 ? "ready" : "empty");
      } catch (error) {
        setNotes([`Could not read the files: ${errorText(error)}`]);
      } finally {
        setBusy(false);
      }
    },
    [entries, options, busy],
  );

  const browse = useCallback(async () => {
    if (busy) return;
    try {
      const picked = await invoke<string[] | null>("mr_pick_files");
      if (picked) await addFiles(picked);
    } catch (error) {
      setNotes([`Could not open the file dialog: ${errorText(error)}`]);
    }
  }, [addFiles, busy]);

  const toggle = (path: string) => {
    setExpanded((prev) =>
      prev.includes(path) ? prev.filter((item) => item !== path) : [...prev, path],
    );
  };

  const removeFile = (path: string) => {
    const next = entries.filter((entry) => entry.path !== path);
    setEntries(next);
    if (next.length === 0) {
      setPhase("empty");
      setNotes([]);
      setSummary(null);
    }
  };

  const clearAll = () => {
    setEntries([]);
    setNotes([]);
    setSummary(null);
    setExpanded([]);
    setPhase("empty");
  };

  /** The two "keep" options change what is found, so the files are scanned
   * again when one of them changes. */
  const changeOptions = async (next: Options) => {
    const rescan =
      next.keepOrientation !== options.keepOrientation ||
      next.keepColorProfile !== options.keepColorProfile;
    setOptions(next);
    if (!rescan || entries.length === 0 || busy) return;
    setBusy(true);
    try {
      const scanned = await invoke<ScannedFile[]>("mr_scan_files", {
        paths: entries.map((entry) => entry.path),
        options: next,
      });
      setEntries((prev) =>
        prev.map((entry, index) => {
          const fresh = scanned[index];
          if (!fresh || fresh.path !== entry.path || entry.status === "done") return entry;
          return { ...entry, ...fresh };
        }),
      );
    } catch (error) {
      setNotes([`Could not scan the files again: ${errorText(error)}`]);
    } finally {
      setBusy(false);
    }
  };

  /** Cleans the files that have something to remove. Every status change
   * arrives through the channel; the invoke resolves with the summary. */
  const clean = async () => {
    const targets = entries
      .map((entry, index) => ({ entry, index }))
      .filter(({ entry }) => isCleanable(entry));
    if (targets.length === 0 || busy) return;
    const indexes = targets.map((target) => target.index);
    setBusy(true);
    setCancelling(false);
    setSummary(null);
    setNotes([]);
    setBatch(indexes);
    setPhase("cleaning");
    setEntries((prev) =>
      prev.map((entry, index) =>
        indexes.includes(index) ? { ...entry, status: "waiting", done: 0, total: 0, note: null } : entry,
      ),
    );
    try {
      const channel = new Channel<ProgressEvent>();
      channel.onmessage = (event) => {
        const target = indexes[event.index];
        if (target === undefined) return;
        setEntries((prev) =>
          prev.map((entry, index) => (index === target ? applyEvent(entry, event) : entry)),
        );
      };
      const result = await invoke<BatchSummary>("mr_clean_files", {
        paths: targets.map((target) => target.entry.path),
        options,
        onProgress: channel,
      });
      setSummary(result);
      setPhase("done");
    } catch (error) {
      setNotes([`Cleaning could not start: ${errorText(error)}`]);
      setEntries((prev) =>
        prev.map((entry) => (entry.status === "waiting" ? { ...entry, status: "ready" } : entry)),
      );
      setPhase("ready");
    } finally {
      setBusy(false);
      setCancelling(false);
    }
  };

  const cancel = async () => {
    if (!cleaning) return;
    setCancelling(true);
    try {
      await invoke("mr_cancel");
    } catch (error) {
      setNotes([`Could not cancel: ${errorText(error)}`]);
    }
  };

  const outputPaths = entries
    .map((entry) => entry.outputPath)
    .filter((path): path is string => Boolean(path));

  const allInOneFolder =
    outputPaths.length > 0 &&
    outputPaths.every((path) => parentFolder(path) === parentFolder(outputPaths[0]));

  const openOutputFolder = async () => {
    try {
      await invoke("mr_reveal_files", { paths: outputPaths });
    } catch (error) {
      setNotes([`Could not open the folder: ${errorText(error)}`]);
    }
  };

  const cleanableCount = entries.filter(isCleanable).length;
  const finished = batch.filter((index) => {
    const entry = entries[index];
    return entry !== undefined && FINAL_STATUSES.includes(entry.status);
  }).length;

  return (
    <div className="mr">
      {phase === "empty" ? (
        <div className="mr-empty">
          <DropZone
            label="Drag photos or videos here"
            hint={`Formats: ${SUPPORTED_FORMATS} \u2014 up to ${MAX_FILES} files`}
            onFiles={addFiles}
            onBrowse={browse}
            disabled={busy}
          />
          <p className="mr-note mr-center">
            You see what is hidden in each file first. Your files are never changed: a new
            copy without metadata is saved next to each original, with ERX before the
            extension.
          </p>
          {notes.length > 0 && (
            <ul className="mr-notes" aria-live="polite">
              {notes.map((note) => (
                <li key={note}>{note}</li>
              ))}
            </ul>
          )}
        </div>
      ) : (
        <div className="mr-layout">
          <div className="mr-main">
            <DropZone
              className="mr-dropzone-compact"
              label="Drop more files here"
              hint={`Formats: ${SUPPORTED_FORMATS}`}
              onFiles={addFiles}
              onBrowse={browse}
              disabled={busy}
            />

            {notes.length > 0 && (
              <ul className="mr-notes" aria-live="polite">
                {notes.map((note) => (
                  <li key={note}>{note}</li>
                ))}
              </ul>
            )}

            <FileList
              entries={entries}
              expanded={expanded}
              onToggle={toggle}
              onRemove={removeFile}
              onClear={clearAll}
              disabled={busy}
            />

            {cleaning && (
              <div className="mr-progress-row">
                <ProgressBar
                  value={finished}
                  max={batch.length}
                  label="Overall cleaning progress"
                />
                <button
                  type="button"
                  className="button-secondary"
                  onClick={cancel}
                  disabled={cancelling}
                >
                  {cancelling ? "Cancelling\u2026" : "Cancel"}
                </button>
              </div>
            )}

            {!cleaning && (
              <div className="mr-actions">
                <button
                  type="button"
                  className="button-primary"
                  onClick={clean}
                  disabled={busy || cleanableCount === 0}
                >
                  Clean {cleanableCount} file{cleanableCount === 1 ? "" : "s"}
                </button>
                {phase === "done" && allInOneFolder && (
                  <button
                    type="button"
                    className="button-secondary"
                    onClick={openOutputFolder}
                  >
                    Open output folder
                  </button>
                )}
              </div>
            )}

            {!cleaning && cleanableCount === 0 && phase === "ready" && (
              <p className="mr-note">
                None of these files has metadata that can be removed.
              </p>
            )}

            {phase === "done" && summary && (
              <p className="mr-summary">
                Cleaned {summary.cleaned} {"\u00b7"} Already clean {summary.alreadyClean}{" "}
                {"\u00b7"} Failed {summary.failed} {"\u00b7"} Cancelled {summary.cancelled}
              </p>
            )}
          </div>

          <OptionsPanel options={options} onChange={changeOptions} disabled={busy} />
        </div>
      )}
    </div>
  );
}
