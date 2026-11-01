import { useCallback, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import DropZone from "../../shared/DropZone";
import FileList from "./FileList";
import OptionsPanel from "./OptionsPanel";
import type { ConvertOptions, FileEntry, InspectedFile } from "./types";
import { MAX_FILES, SUPPORTED_INPUT } from "./types";
import "./image-converter.css";

/** Phase of the tool; 2A covers empty + ready, 2C adds converting + done. */
type Phase = "empty" | "ready";

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

/** Image Converter: drop or pick files, review the list, choose options. */
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
  const [probeResult, setProbeResult] = useState<string | null>(null);

  /** Classifies dropped/picked paths in Rust and merges them into the list. */
  const addFiles = useCallback(
    async (paths: string[]) => {
      if (paths.length === 0 || busy) return;
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
    [busy, entries],
  );

  const browse = useCallback(async () => {
    if (busy) return;
    try {
      const picked = await invoke<string[] | null>("pick_image_files");
      if (picked) await addFiles(picked);
    } catch (error) {
      setNotes([`Could not open the file dialog: ${errorText(error)}`]);
    }
  }, [addFiles, busy]);

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
    setProbeResult(null);
    setPhase("empty");
  };

  /** 2A: proves the JS → Rust bridge with the real options payload. The
   * actual conversion pipeline replaces this in checkpoint 2C. */
  const convert = async () => {
    if (entries.length === 0 || busy) return;
    setBusy(true);
    setProbeResult(null);
    try {
      const result = await invoke<string>("probe_conversion", {
        count: entries.length,
        options: {
          format: options.format,
          jpegQuality: options.jpegQuality,
          background: options.background,
          outputFolder: options.outputFolder,
        },
      });
      setProbeResult(result);
    } catch (error) {
      setProbeResult(`Error: ${errorText(error)}`);
    } finally {
      setBusy(false);
    }
  };

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
              disabled={busy}
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
              disabled={busy}
            />

            <div className="ic-actions">
              <button
                type="button"
                className="button-primary"
                onClick={convert}
                disabled={busy || entries.length === 0}
              >
                Convert {entries.length} file{entries.length === 1 ? "" : "s"}
              </button>
              {probeResult && <p className="ic-probe">{probeResult}</p>}
            </div>
          </div>

          <OptionsPanel options={options} onChange={setOptions} disabled={busy} />
        </div>
      )}
    </div>
  );
}
