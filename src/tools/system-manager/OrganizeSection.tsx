import { useCallback, useEffect, useState } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import InfoNote from "./InfoNote";
import SectionCard from "./SectionCard";
import type {
  DateSourceId,
  GranularityId,
  OrganizeModeInput,
  OrganizeTab,
  RunReport,
  ScanPlan,
  SortMode,
} from "./types";

/** Turns anything thrown by invoke into a short display string. */
function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

type Phase = "idle" | "scanning" | "preview" | "running" | "report";

const TAB_LABELS: Record<OrganizeTab, string> = {
  desktop: "Desktop",
  folder: "Folder",
};

const SORT_LABELS: Record<SortMode, string> = {
  format: "Format",
  date: "Date",
  size: "Size",
};

const DATE_SOURCE_LABELS: Record<DateSourceId, string> = {
  photo_taken: "Photo date taken",
  created: "Date created",
  modified: "Date modified",
};

const GRANULARITY_LABELS: Record<GranularityId, string> = {
  year: "Year",
  year_month: "Year and month",
};

/** One plain sentence per mode (spec 5h-4). */
const MODE_SENTENCE: Record<SortMode, string> = {
  format: "Groups files into folders such as Documents, Images, Videos, Music and Archives.",
  date: "Groups files into folders by year (or by year and month). Files without a date go into a folder named “Date not found”.",
  size: "Groups files into folders by size: Under 1 MB, 1-10 MB, 10-100 MB, 100 MB-1 GB and Over 1 GB.",
};

const DESKTOP_TAB_TEXT =
  "Moves the loose files on your Desktop into three folders: Apps (programs and shortcuts), Documents (Word, PDF, Excel and similar) and More (everything else). Windows icons such as This PC and Recycle Bin are not touched.";

/** The preview lists at most this many names per category, then "and N more". */
const PREVIEW_NAME_CAP = 200;

function plural(count: number, one: string, many: string): string {
  return `${count} ${count === 1 ? one : many}`;
}

function folderName(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  return trimmed.split(/[\\/]/).pop() ?? trimmed;
}

function modeInput(
  mode: SortMode,
  dateSource: DateSourceId,
  granularity: GranularityId,
): OrganizeModeInput {
  return mode === "date" ? { mode, dateSource, granularity } : { mode };
}

/** Organize, restructured (Prompt 5h): a Desktop tab with the proven
 * three-folder behavior and a Folder tab with a "Sort by" selector
 * (Format | Date | Size). Scan → plain-words preview → run with honest
 * progress → report → Undo. Nothing is deleted or overwritten, ever. */
export default function OrganizeSection() {
  const [tab, setTab] = useState<OrganizeTab>("desktop");
  const [customPath, setCustomPath] = useState<string | null>(null);
  const [cameFromDownloads, setCameFromDownloads] = useState(false);
  const [mode, setMode] = useState<SortMode>("format");
  const [dateSource, setDateSource] = useState<DateSourceId>("photo_taken");
  const [granularity, setGranularity] = useState<GranularityId>("year");

  const [phase, setPhase] = useState<Phase>("idle");
  const [scanCount, setScanCount] = useState<number | null>(null);
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

  const refreshUndoInfo = useCallback(async (target: string) => {
    try {
      const info = await invoke<{ undoable: boolean; root: string | null }>(
        "organize_undo_info",
        { target },
      );
      setUndoInfo(info);
    } catch {
      setUndoInfo({ undoable: false, root: null });
    }
  }, []);

  useEffect(() => {
    void refreshUndoInfo(tab === "folder" ? "custom" : tab);
  }, [tab, refreshUndoInfo]);

  /** Changing any option clears the old preview: it must always match the
   * visible options. */
  const clearPreview = useCallback(() => {
    setPlan(null);
    setReport(null);
    setUndoSummary(null);
    setPhase("idle");
  }, []);

  const scan = useCallback(async () => {
    setError(null);
    setReport(null);
    setUndoSummary(null);
    setScanCount(0);
    setPhase("scanning");
    try {
      const channel = new Channel<{ checked: number }>();
      channel.onmessage = (event) => setScanCount(event.checked);
      const scanned = await invoke<ScanPlan>("organize_scan", {
        tab,
        customPath: tab === "folder" ? customPath : undefined,
        mode: modeInput(mode, dateSource, granularity),
        onProgress: channel,
      });
      setPlan(scanned);
      setPhase("preview");
      void refreshUndoInfo(scanned.target);
    } catch (caught) {
      setError(errorText(caught));
      setPhase("idle");
    } finally {
      setScanCount(null);
    }
  }, [tab, customPath, mode, dateSource, granularity, refreshUndoInfo]);

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
        target: plan.target,
        customPath: tab === "folder" ? customPath : undefined,
        mode: modeInput(mode, dateSource, granularity),
        files,
        onProgress: channel,
      });
      setReport(result);
      setPlan(null);
      setPhase("report");
      void refreshUndoInfo(plan.target);
    } catch (caught) {
      setError(errorText(caught));
      setPhase("preview");
    } finally {
      setProgress(null);
    }
  }, [plan, tab, customPath, mode, dateSource, granularity, refreshUndoInfo]);

  const undo = useCallback(async () => {
    setError(null);
    try {
      const target = tab === "folder" ? "custom" : tab;
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
  }, [tab, refreshUndoInfo]);

  const chooseFolder = useCallback(async () => {
    setError(null);
    try {
      const picked = await invoke<string | null>("organize_pick_folder");
      if (picked !== null) {
        setCustomPath(picked);
        setCameFromDownloads(false);
        clearPreview();
      }
    } catch (caught) {
      setError(errorText(caught));
    }
  }, [clearPreview]);

  const useDownloads = useCallback(async () => {
    setError(null);
    try {
      const path = await invoke<string>("organize_downloads_path");
      setCustomPath(path);
      setCameFromDownloads(true);
      clearPreview();
    } catch (caught) {
      setError(errorText(caught));
    }
  }, [clearPreview]);

  const totalToMove = plan?.categories.reduce((sum, category) => sum + category.files.length, 0) ?? 0;
  const totalSkipped = plan?.skipped.length ?? 0;
  const folderCount = plan?.categories.length ?? 0;
  const undoLabel =
    tab === "folder" && undoInfo.root !== null
      ? `Undo last organize in ${folderName(undoInfo.root)}`
      : "Undo last organize";
  // In the Folder tab, Date/Size leave program files in place everywhere
  // except the Downloads folder (the scan corrects any guess).
  const showProtectedNote =
    tab === "folder" &&
    mode !== "format" &&
    customPath !== null &&
    !cameFromDownloads &&
    plan?.target !== "downloads";

  return (
    <SectionCard title="Organize" subtitle="Tidy loose files into folders. Nothing is deleted.">
      <div className="sm-org-segments" role="tablist" aria-label="Which folder to organize">
        {(Object.keys(TAB_LABELS) as OrganizeTab[]).map((id) => (
          <button
            key={id}
            type="button"
            role="tab"
            aria-selected={tab === id}
            className={`sm-org-segment ${tab === id ? "sm-org-segment-active" : ""}`}
            disabled={busy}
            onClick={() => {
              setTab(id);
              clearPreview();
              setError(null);
            }}
          >
            {TAB_LABELS[id]}
          </button>
        ))}
      </div>

      {tab === "desktop" ? (
        <InfoNote>{DESKTOP_TAB_TEXT}</InfoNote>
      ) : (
        <>
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
            <button
              type="button"
              className="sm-dialog-button"
              disabled={busy}
              onClick={() => void useDownloads()}
            >
              Use Downloads folder
            </button>
          </div>

          <div className="sm-org-sortby">
            <span className="sm-org-sortby-label">Sort by</span>
            <div className="sm-org-segments" role="tablist" aria-label="Sort by">
              {(Object.keys(SORT_LABELS) as SortMode[]).map((id) => (
                <button
                  key={id}
                  type="button"
                  role="tab"
                  aria-selected={mode === id}
                  className={`sm-org-segment ${mode === id ? "sm-org-segment-active" : ""}`}
                  disabled={busy}
                  onClick={() => {
                    setMode(id);
                    clearPreview();
                  }}
                >
                  {SORT_LABELS[id]}
                </button>
              ))}
            </div>
          </div>
          <p className="sm-note-info">{MODE_SENTENCE[mode]}</p>

          {mode === "date" && (
            <div className="sm-org-mode-options">
              <label className="sm-awake-select">
                <span>Which date?</span>
                <select
                  value={dateSource}
                  disabled={busy}
                  onChange={(event) => {
                    setDateSource(event.target.value as DateSourceId);
                    clearPreview();
                  }}
                >
                  {(Object.keys(DATE_SOURCE_LABELS) as DateSourceId[]).map((id) => (
                    <option key={id} value={id}>
                      {DATE_SOURCE_LABELS[id]}
                    </option>
                  ))}
                </select>
              </label>
              {dateSource === "photo_taken" ? (
                <p className="sm-note-info">
                  Read from the photo itself (camera data). Files that are not photos, or photos
                  without this data, go to “Date not found”.
                </p>
              ) : (
                <p className="sm-note-info">
                  These dates come from Windows and can change when files are copied or edited.
                </p>
              )}
              <label className="sm-awake-select">
                <span>Folders by</span>
                <select
                  value={granularity}
                  disabled={busy}
                  onChange={(event) => {
                    setGranularity(event.target.value as GranularityId);
                    clearPreview();
                  }}
                >
                  {(Object.keys(GRANULARITY_LABELS) as GranularityId[]).map((id) => (
                    <option key={id} value={id}>
                      {GRANULARITY_LABELS[id]}
                    </option>
                  ))}
                </select>
              </label>
            </div>
          )}

          <InfoNote>
            Only files directly inside this folder are organized. Existing sub-folders are left
            untouched.
          </InfoNote>
          {showProtectedNote && (
            <InfoNote>
              Program files (such as .exe and .dll) and shortcuts are left in place in this
              folder.
            </InfoNote>
          )}
        </>
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
          disabled={busy || (tab === "folder" && customPath === null)}
          onClick={() => void scan()}
        >
          Scan
        </button>
      )}

      {phase === "scanning" && scanCount !== null && (
        <p className="sm-org-progress">Scanning… {scanCount} files checked</p>
      )}

      {plan !== null && phase === "preview" && (
        <div className="sm-org-preview">
          <p className="sm-org-preview-title">
            Ready to organize:{" "}
            {tab === "desktop" ? "Desktop" : folderName(plan.root)}
          </p>
          {totalToMove === 0 ? (
            <p className="sm-note-info">
              There is nothing to move — every loose file here is skipped or left in place.
            </p>
          ) : (
            <p className="sm-org-preview-text">
              ERX Tools will move {plural(totalToMove, "loose file", "loose files")} into{" "}
              {plural(folderCount, "folder", "folders")}
              {plan.mode === "date"
                ? plan.categories.some((category) => /^-?\d{4}-\d{2}$/.test(category.name))
                  ? " named by year and month:"
                  : " named by year:"
                : plan.mode === "size"
                  ? " by size:"
                  : ":"}
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
            exists, the file is renamed like "
            {plan.mode === "date" ? "photo (2).jpg" : "report (2).pdf"}".
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
              not moved here).
            </p>
          )}
          {plan.onOnedrive && (
            <InfoNote>
              This folder is stored in OneDrive. ERX Tools can organize it and OneDrive will
              sync the changes. Online-only files are skipped. ERX Tools never changes any
              OneDrive settings.
            </InfoNote>
          )}
          {tab === "desktop" && (
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
            <button type="button" className="sm-dialog-button" onClick={clearPreview}>
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

      {undoSummary !== null && <p className="sm-note-info">Undo finished: {undoSummary}</p>}

      {undoInfo.undoable && phase !== "preview" && (
        <div className="sm-org-undo">
          <button
            type="button"
            className="sm-dialog-button"
            disabled={busy}
            onClick={() => void undo()}
          >
            {undoLabel}
          </button>
          <InfoNote>Only the most recent run for this target can be undone.</InfoNote>
        </div>
      )}
    </SectionCard>
  );
}
