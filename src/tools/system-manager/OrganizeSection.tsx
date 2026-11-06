import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Channel } from "@tauri-apps/api/core";
import InfoNote from "./InfoNote";
import SectionCard from "./SectionCard";
import type { OrganizeTargetId, RunReport, ScanPlan } from "./types";

/** Turns anything thrown by invoke into a short display string. */
function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

type Phase = "idle" | "scanning" | "preview" | "running" | "report";

const TARGET_LABELS: Record<OrganizeTargetId, string> = {
  desktop: "Desktop",
  downloads: "Downloads",
  custom: "Custom folder…",
};

/** The preview lists at most this many names per category, then "and N more". */
const PREVIEW_NAME_CAP = 200;

function plural(count: number, one: string, many: string): string {
  return `${count} ${count === 1 ? one : many}`;
}

/** Organize: scan (read-only) → plain-words preview → run with honest
 * progress → report → Undo. The backend re-checks every file before and
 * during the run; nothing is deleted or overwritten, ever. */
export default function OrganizeSection() {
  const [target, setTarget] = useState<OrganizeTargetId>("desktop");
  const [customPath, setCustomPath] = useState<string | null>(null);
  const [phase, setPhase] = useState<Phase>("idle");
  const [plan, setPlan] = useState<ScanPlan | null>(null);
  const [report, setReport] = useState<RunReport | null>(null);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [undoInfo, setUndoInfo] = useState<{ undoable: boolean; root: string | null }>({
    undoable: false,
    root: null,
  });
  const [undoSummary, setUndoSummary] = useState<string | null>(null);

  const busy = phase === "scanning" || phase === "running";

  const refreshUndoInfo = useCallback(async (targetId: OrganizeTargetId) => {
    try {
      const info = await invoke<{ undoable: boolean; root: string | null }>(
        "organize_undo_info",
        { target: targetId },
      );
      setUndoInfo(info);
    } catch {
      setUndoInfo({ undoable: false, root: null });
    }
  }, []);

  useEffect(() => {
    if (target === "custom") {
      void refreshUndoInfo("custom");
    } else {
      void refreshUndoInfo(target);
    }
  }, [target, refreshUndoInfo]);

  const scan = useCallback(async () => {
    setError(null);
    setReport(null);
    setUndoSummary(null);
    setPhase("scanning");
    try {
      const scanned = await invoke<ScanPlan>("organize_scan", {
        target,
        customPath: target === "custom" ? customPath : undefined,
      });
      setPlan(scanned);
      setPhase("preview");
    } catch (caught) {
      setError(errorText(caught));
      setPhase("idle");
    }
  }, [target, customPath]);

  const run = useCallback(async () => {
    if (plan === null) return;
    setError(null);
    setPhase("running");
    setProgress({ done: 0, total: 0 });
    try {
      const channel = new Channel<{ done: number; total: number }>();
      channel.onmessage = (event) => setProgress(event);
      const files = plan.categories.flatMap((category) => category.files);
      const result = await invoke<RunReport>("organize_run", {
        target,
        customPath: target === "custom" ? customPath : undefined,
        files,
        onProgress: channel,
      });
      setReport(result);
      setPlan(null);
      setPhase("report");
      void refreshUndoInfo(target);
    } catch (caught) {
      setError(errorText(caught));
      setPhase("preview");
    } finally {
      setProgress(null);
    }
  }, [plan, target, customPath, refreshUndoInfo]);

  const undo = useCallback(async () => {
    setError(null);
    try {
      const result = await invoke<{ summary: string; notes: string[]; failed: string[] }>(
        "organize_undo",
        { target },
      );
      setUndoSummary(
        result.summary +
          (result.notes.length > 0 ? ` ${result.notes.join(" ")}` : "") +
          (result.failed.length > 0 ? ` ${result.failed.join(" ")}` : ""),
      );
      setReport(null);
      void refreshUndoInfo(target);
    } catch (caught) {
      setError(errorText(caught));
    }
  }, [target, refreshUndoInfo]);

  const chooseFolder = useCallback(async () => {
    setError(null);
    try {
      const picked = await invoke<string | null>("organize_pick_folder");
      if (picked !== null) {
        setCustomPath(picked);
        setPlan(null);
        setPhase("idle");
        setReport(null);
      }
    } catch (caught) {
      setError(errorText(caught));
    }
  }, []);

  const cancelPreview = useCallback(() => {
    setPlan(null);
    setPhase("idle");
    setError(null);
  }, []);

  const totalToMove = plan?.categories.reduce((sum, category) => sum + category.files.length, 0) ?? 0;
  const totalSkipped = plan?.skipped.length ?? 0;
  const folderCount = plan?.categories.length ?? 0;
  const undoLabel =
    target === "custom" && undoInfo.root !== null
      ? `Undo last organize in ${undoInfo.root.split("\\").pop() ?? undoInfo.root}`
      : "Undo last organize";

  return (
    <SectionCard title="Organize" subtitle="Tidy loose files into folders. Nothing is deleted.">
      <div className="sm-org-segments" role="tablist" aria-label="Which folder to organize">
        {(Object.keys(TARGET_LABELS) as OrganizeTargetId[]).map((id) => (
          <button
            key={id}
            type="button"
            role="tab"
            aria-selected={target === id}
            className={`sm-org-segment ${target === id ? "sm-org-segment-active" : ""}`}
            disabled={busy}
            onClick={() => {
              setTarget(id);
              setPlan(null);
              setReport(null);
              setError(null);
              setPhase("idle");
              setUndoSummary(null);
            }}
          >
            {TARGET_LABELS[id]}
          </button>
        ))}
      </div>

      {target === "custom" && (
        <div className="sm-org-custom">
          <span className="sm-org-custom-path">
            {customPath ?? "No folder chosen yet."}
          </span>
          <button
            type="button"
            className="sm-dialog-button"
            disabled={busy}
            onClick={() => void chooseFolder()}
          >
            Choose folder…
          </button>
        </div>
      )}

      {error !== null && (
        <p className="sm-icon-row-error" role="alert">
          {error}
        </p>
      )}

      {phase === "idle" && (
        <button
          type="button"
          className="button-primary sm-org-scan"
          disabled={busy || (target === "custom" && customPath === null)}
          onClick={() => void scan()}
        >
          Scan
        </button>
      )}

      {plan !== null && phase === "preview" && (
        <div className="sm-org-preview">
          <p className="sm-org-preview-title">
            Ready to organize: {TARGET_LABELS[target] === "Custom folder…" ? "your chosen folder" : TARGET_LABELS[target]}
          </p>
          {totalToMove === 0 ? (
            <p className="sm-note-info">
              There is nothing to move — every loose file here is skipped or left in place.
            </p>
          ) : (
            <p className="sm-org-preview-text">
              ERX Tools will move {plural(totalToMove, "loose file", "loose files")} into{" "}
              {plural(folderCount, "folder", "folders")}:
            </p>
          )}
          <ul className="sm-org-categories">
            {plan.categories.map((category) => (
              <li key={category.name}>
                <details>
                  <summary className="sm-org-category">
                    {category.name} — {plural(category.files.length, "file", "files")}
                  </summary>
                  <ul className="sm-org-filelist">
                    {category.files.slice(0, PREVIEW_NAME_CAP).map((file) => (
                      <li key={file.name}>{file.name}</li>
                    ))}
                    {category.files.length > PREVIEW_NAME_CAP && (
                      <li>and {category.files.length - PREVIEW_NAME_CAP} more…</li>
                    )}
                  </ul>
                </details>
              </li>
            ))}
          </ul>
          <p className="sm-note-info">
            Nothing is deleted or overwritten. Folders are never touched. If a name already
            exists, the file is renamed like "report (2).pdf".
          </p>
          {totalSkipped > 0 && (
            <details className="sm-org-skipped">
              <summary className="sm-org-category">
                {plural(totalSkipped, "file will be skipped", "files will be skipped")} (see why)
              </summary>
              <ul className="sm-org-filelist">
                {plan.skipped.map((file) => (
                  <li key={file.name}>
                    {file.name} — {file.reason}
                  </li>
                ))}
              </ul>
            </details>
          )}
          {plan.leftInPlace > 0 && (
            <p className="sm-note-info">
              {plural(plan.leftInPlace, "file stays", "files stay")} where{" "}
              {plan.leftInPlace === 1 ? "it is" : "they are"} (programs and unknown types are
              not moved in a chosen folder).
            </p>
          )}
          {plan.onOnedrive && (
            <InfoNote>
              This folder is stored in OneDrive. ERX Tools can organize it and OneDrive will
              sync the changes. Online-only files are skipped. ERX Tools never changes any
              OneDrive settings.
            </InfoNote>
          )}
          {target === "desktop" && (
            <InfoNote>
              Icons from the shared Public Desktop folder are not part of your Desktop folder
              and are left alone.
            </InfoNote>
          )}
          <p className="sm-note-info">You can undo this afterwards.</p>
          <div className="sm-org-actions">
            <button
              type="button"
              className="button-primary"
              disabled={totalToMove === 0}
              onClick={() => void run()}
            >
              Organize now
            </button>
            <button type="button" className="sm-dialog-button" onClick={cancelPreview}>
              Cancel
            </button>
          </div>
        </div>
      )}

      {phase === "running" && progress !== null && (
        <p className="sm-org-progress">
          Organizing… {progress.done} of {progress.total} files
        </p>
      )}

      {report !== null && phase === "report" && (
        <div className="sm-org-report">
          <p className="sm-org-preview-title">
            Done: {plural(report.moved, "file moved", "files moved")},{" "}
            {plural(report.skipped.length, "file skipped", "files skipped")}.
          </p>
          {report.skipped.length > 0 && (
            <ul className="sm-org-filelist">
              {report.skipped.map((file) => (
                <li key={file.name}>
                  {file.name} — {file.reason}
                </li>
              ))}
            </ul>
          )}
          <InfoNote>Nothing was deleted. You can undo this run below.</InfoNote>
        </div>
      )}

      {undoSummary !== null && (
        <p className="sm-note-info">Undo finished: {undoSummary}</p>
      )}

      {undoInfo.undoable && phase !== "preview" && (
        <div className="sm-org-undo">
          <button type="button" className="sm-dialog-button" disabled={busy} onClick={() => void undo()}>
            {undoLabel}
          </button>
          <InfoNote>Only the most recent run for this target can be undone.</InfoNote>
        </div>
      )}
    </SectionCard>
  );
}
