import { useEffect, useRef, useState } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import ColorControl from "./ColorControl";
import FolderList from "./FolderList";
import PatternControls from "./PatternControls";
import type {
  BatchEvent,
  BatchSummary,
  EligibilityDto,
  FolderEntry,
  IconSettingsInput,
  InspectResult,
  PatternKind,
} from "./types";
import { MAX_FOLDERS } from "./types";
import DropZone from "../../shared/DropZone";
import ProgressBar from "../../shared/ProgressBar";
import "./folder-icon.css";

/** Preview refresh delay: rapid slider/typing changes don't spam Rust. */
const DEBOUNCE_MS = 50;

/** Turns anything thrown by invoke into a short display string. */
function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

type PreviewState =
  | { status: "empty" }
  | { status: "ok"; svg: string; warnings: string[]; frames: FramePreview[] }
  | { status: "error"; message: string };

/** One small preview frame, rendered on the exact .ico raster path. */
interface FramePreview {
  size: number;
  dataUrl: string;
}

type BusyAction = "pick" | "apply" | "restore" | null;

/** Folder Icon Changer: folders on the left (drag & drop or the native
 * picker), the icon designer on the right. Apply/Restore run over all
 * eligible folders sequentially; Rust validates and performs every file
 * operation. */
export default function FolderIconChanger() {
  // --- designer state ---
  const [baseColor, setBaseColor] = useState("#0078D4");
  const [pattern, setPattern] = useState<PatternKind>("none");
  const [patternColor, setPatternColor] = useState("#FFFFFF");
  const [patternSize, setPatternSize] = useState(3);
  const [patternOpacity, setPatternOpacity] = useState(70);

  // --- preview state ---
  const [preview, setPreview] = useState<PreviewState>({ status: "empty" });
  const requestRef = useRef(0);

  // --- folder list state ---
  const [folders, setFolders] = useState<FolderEntry[]>([]);
  const [busy, setBusy] = useState<BusyAction>(null);
  const [note, setNote] = useState<string | null>(null);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(
    null,
  );

  // Live preview: regenerate (debounced) whenever the design changes. A
  // request counter drops responses that arrive out of order.
  useEffect(() => {
    const settings: IconSettingsInput = {
      baseColor,
      pattern,
      patternColor,
      patternSize,
      patternOpacity,
    };
    const request = ++requestRef.current;
    const timer = window.setTimeout(() => {
      invoke<{ svg: string; warnings: string[]; frames: FramePreview[] }>(
        "folder_icon_preview_svg",
        { settings },
      )
        .then((result) => {
          if (requestRef.current === request) {
            setPreview({
              status: "ok",
              svg: result.svg,
              warnings: result.warnings,
              frames: result.frames,
            });
          }
        })
        .catch((error) => {
          if (requestRef.current === request) {
            setPreview({ status: "error", message: errorText(error) });
          }
        });
    }, DEBOUNCE_MS);
    return () => window.clearTimeout(timer);
  }, [baseColor, pattern, patternColor, patternSize, patternOpacity]);

  /** Adds paths (from the dialog or a drop), skipping duplicates and
   * enforcing the cap. Each path is inspected by Rust: files, protected
   * and otherwise blocked locations land in the list as Blocked with the
   * reason. */
  function addPaths(paths: string[]) {
    setNote(null);
    setFolders((current) => {
      const known = new Set(current.map((entry) => entry.path.toLowerCase()));
      const added: FolderEntry[] = [];
      let duplicates = 0;
      let overflow = 0;
      for (const path of paths) {
        if (current.length + added.length >= MAX_FOLDERS) {
          overflow += 1;
          continue;
        }
        const key = path.toLowerCase();
        if (known.has(key)) {
          duplicates += 1;
          continue;
        }
        known.add(key);
        added.push({
          path,
          status: "checking",
          detail: null,
          inspectState: null,
        });
      }
      if (duplicates > 0 || overflow > 0) {
        const parts: string[] = [];
        if (duplicates > 0) {
          parts.push(
            `Skipped ${duplicates} duplicate ${duplicates === 1 ? "folder" : "folders"} already in the list.`,
          );
        }
        if (overflow > 0) {
          parts.push(`Only ${MAX_FOLDERS} folders per batch — ${overflow} not added.`);
        }
        setNote(parts.join(" "));
      }
      if (added.length === 0) return current;
      // Inspect each new path; keep the response matching by path.
      for (const entry of added) {
        invoke<InspectResult>("folder_icon_inspect", { path: entry.path })
          .then((info) => {
            setFolders((list) =>
              list.map((item) =>
                item.path === entry.path
                  ? {
                      ...item,
                      status: statusOf(info.eligibility),
                      detail: detailOf(info.eligibility),
                      inspectState: info.state,
                    }
                  : item,
              ),
            );
          })
          .catch((error) => {
            setFolders((list) =>
              list.map((item) =>
                item.path === entry.path
                  ? { ...item, status: "failed", detail: errorText(error) }
                  : item,
              ),
            );
          });
      }
      return [...current, ...added];
    });
  }

  async function pickFolders() {
    setBusy("pick");
    try {
      const picked = await invoke<string[] | null>("folder_icon_pick_folders");
      if (picked && picked.length > 0) addPaths(picked);
    } catch (error) {
      setNote(errorText(error));
    } finally {
      setBusy(null);
    }
  }

  function removeFolder(index: number) {
    setNote(null);
    setFolders((list) => list.filter((_, i) => i !== index));
  }

  function clearList() {
    setNote(null);
    setFolders([]);
  }

  /** Applies the current design to every Ready/Warning folder through the
   * Rust batch command: strictly sequential, per-folder events update the
   * rows live, cancellable, capped at 100 in Rust. */
  async function applyAll() {
    const targets = folders.filter(
      (entry) => entry.status === "ready" || entry.status === "warning",
    );
    if (targets.length === 0) return;
    const settings: IconSettingsInput = {
      baseColor,
      pattern,
      patternColor,
      patternSize,
      patternOpacity,
    };
    await runBatch("folder_icon_apply_batch", targets, { settings });
  }

  /** Restores the default icon on every eligible folder. */
  async function restoreAll() {
    const targets = folders.filter(
      (entry) =>
        entry.status === "ready" ||
        entry.status === "warning" ||
        entry.status === "applied",
    );
    if (targets.length === 0) return;
    await runBatch("folder_icon_restore_batch", targets, {});
  }

  /** Shared batch runner: streams per-folder results through a Channel,
   * shows the "3 of 12" progress and a Cancel button, and reports the
   * summary. If the command rejects (validation), nothing ran and the
   * rows go back to their previous state. */
  async function runBatch(
    command: "folder_icon_apply_batch" | "folder_icon_restore_batch",
    targets: FolderEntry[],
    extraArgs: Record<string, unknown>,
  ) {
    setBusy(command === "folder_icon_apply_batch" ? "apply" : "restore");
    setNote(null);
    setProgress({ done: 0, total: targets.length });
    setFolders((list) =>
      list.map((item) =>
        targets.some((target) => target.path === item.path)
          ? { ...item, status: "applying", detail: null }
          : item,
      ),
    );

    const channel = new Channel<BatchEvent>();
    channel.onmessage = (event) => {
      setProgress((current) =>
        current ? { ...current, done: current.done + 1 } : current,
      );
      setFolders((list) =>
        list.map((item) => {
          if (item.path !== event.path) return item;
          switch (event.status) {
            case "applied":
              return {
                ...item,
                status: "applied",
                detail: null,
                inspectState: "custom_by_erx",
              };
            case "restored":
              return {
                ...item,
                status: "ready",
                detail: null,
                inspectState: "default",
              };
            case "skipped":
              return { ...item, status: "skipped", detail: event.detail };
            default:
              return { ...item, status: "failed", detail: event.detail };
          }
        }),
      );
    };

    try {
      const summary = await invoke<BatchSummary>(command, {
        paths: targets.map((entry) => entry.path),
        onProgress: channel,
        ...extraArgs,
      });
      const verb = command === "folder_icon_apply_batch" ? "Applied" : "Restored";
      setNote(
        `${verb} ${summary.done} of ${targets.length}` +
          (summary.skipped > 0 ? `, ${summary.skipped} skipped` : "") +
          (summary.failed > 0 ? `, ${summary.failed} failed (see the list).` : "."),
      );
    } catch (error) {
      // Validation ran before any folder was touched — reset the rows.
      setNote(errorText(error));
      setFolders((list) =>
        list.map((item) =>
          item.status === "applying"
            ? { ...item, status: "ready", detail: null }
            : item,
        ),
      );
    } finally {
      setBusy(null);
      setProgress(null);
    }
  }

  async function cancelBatch() {
    try {
      await invoke("folder_icon_cancel_batch");
    } catch {
      // Nothing running — nothing to cancel.
    }
  }

  const previewArea = (() => {
    if (preview.status === "error") {
      return (
        <div className="fi-error" role="alert">
          {preview.message}
        </div>
      );
    }
    if (preview.status === "empty") {
      return <p className="fi-preview-hint">The preview appears here.</p>;
    }
    return (
      <>
        <div
          className="fi-svg-holder"
          // The SVG string is fully generated by our own Rust code from
          // validated colors; it contains no scripts and no user text.
          dangerouslySetInnerHTML={{ __html: preview.svg }}
        />
        <div className="fi-small-row" aria-label="Small size previews">
          {preview.frames.map((frame) => (
            <div key={frame.size} className="fi-small-item">
              <img
                className="fi-small-frame"
                src={frame.dataUrl}
                width={frame.size}
                height={frame.size}
                // Rust-rendered PNG of the exact .ico frame; no remote
                // content, no user strings.
                alt={`Preview at ${frame.size} pixels`}
              />
              <span className="fi-small-size">{frame.size} px</span>
            </div>
          ))}
        </div>
      </>
    );
  })();

  const readyCount = folders.filter(
    (entry) => entry.status === "ready" || entry.status === "warning",
  ).length;
  const restorableCount = folders.filter(
    (entry) =>
      entry.status === "ready" ||
      entry.status === "warning" ||
      entry.status === "applied",
  ).length;
  const working = busy !== null;

  return (
    <div className="fi-layout">
      {/* ----- left: drop zone + folder list ----- */}
      <section className="fi-main" aria-label="Folders">
        <DropZone
          label="Drag & drop folders here"
          hint="Files and protected locations are checked and marked Blocked."
          browseLabel="Add folder"
          onBrowse={pickFolders}
          onFiles={addPaths}
          disabled={working}
        />
        {folders.length > 0 ? (
          <>
            <FolderList
              folders={folders}
              onRemove={removeFolder}
              disabled={working}
            />
            {progress !== null && (
              <div className="fi-progress-area">
                <p className="fi-progress-label" aria-live="polite">
                  {busy === "apply" ? "Applying" : "Restoring"} {progress.done}{" "}
                  of {progress.total}…
                </p>
                <ProgressBar
                  value={progress.done}
                  max={progress.total}
                  label="Batch progress"
                />
                <button
                  type="button"
                  className="button-secondary"
                  onClick={cancelBatch}
                >
                  Cancel
                </button>
              </div>
            )}
            <div className="fi-folder-actions">
              <button
                type="button"
                className="button-primary"
                onClick={applyAll}
                disabled={working || readyCount === 0}
                title={
                  readyCount === 0 ? "No ready folders in the list" : undefined
                }
              >
                {busy === "apply" ? "Applying…" : "Apply icon"}
              </button>
              <button
                type="button"
                className="button-secondary"
                onClick={restoreAll}
                disabled={working || restorableCount === 0}
                title={
                  restorableCount === 0
                    ? "No restorable folders in the list"
                    : undefined
                }
              >
                {busy === "restore" ? "Restoring…" : "Restore default icon"}
              </button>
              <button
                type="button"
                className="button-secondary"
                onClick={clearList}
                disabled={working}
              >
                Clear list
              </button>
            </div>
          </>
        ) : (
          <p className="fi-placeholder-note">
            Add one or more folders to change their icons. Nothing is written
            until you press Apply icon.
          </p>
        )}
        {note !== null && (
          <p className="fi-note" role="status">
            {note}
          </p>
        )}
      </section>

      {/* ----- right: preview + designer ----- */}
      <section className="fi-side" aria-label="Icon design">
        <div className="fi-preview" aria-live="polite">
          {previewArea}
        </div>

        {preview.status === "ok" && preview.warnings.length > 0 && (
          <ul className="fi-warnings">
            {preview.warnings.map((warning) => (
              <li key={warning}>{warning}</li>
            ))}
          </ul>
        )}

        <div className="fi-fieldset">
          <ColorControl
            id="fi-base"
            label="Folder color"
            value={baseColor}
            onColor={setBaseColor}
          />
        </div>

        <PatternControls
          pattern={pattern}
          onPattern={setPattern}
          size={patternSize}
          onSize={setPatternSize}
          opacity={patternOpacity}
          onOpacity={setPatternOpacity}
        />

        <div className="fi-fieldset">
          <ColorControl
            id="fi-pattern-color"
            label="Pattern color"
            value={patternColor}
            onColor={setPatternColor}
            disabled={pattern === "none"}
          />
        </div>
      </section>
    </div>
  );
}

/** Maps the eligibility verdict to a list status. */
function statusOf(eligibility: EligibilityDto): FolderEntry["status"] {
  if (eligibility.status === "blocked") return "blocked";
  if (eligibility.status === "warning") return "warning";
  return "ready";
}

function detailOf(eligibility: EligibilityDto): string | null {
  if (eligibility.status === "blocked") return eligibility.reason;
  if (eligibility.status === "warning") return eligibility.warning;
  return null;
}
