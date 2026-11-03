import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import ColorControl from "./ColorControl";
import PatternControls from "./PatternControls";
import type { IconSettingsInput, PatternKind } from "./types";
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

/** Eligibility verdict from Rust (flat shape). */
interface EligibilityDto {
  status: "allowed" | "warning" | "blocked";
  reason: string | null;
  warning: string | null;
}

/** Inspect result from Rust. */
interface InspectResult {
  state: "default" | "custom_by_erx" | "custom_by_other";
  eligibility: EligibilityDto;
}

type BusyAction = "pick" | "apply" | "restore" | null;

/** Folder Icon Changer: pick ONE folder (drag & drop + multi-folder batch
 * arrive in 4C), design the icon on the right, then Apply or Restore.
 * Rust does every file operation; the webview only sends paths and
 * validated settings. */
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

  // --- folder state (4B: one folder) ---
  const [folder, setFolder] = useState<string | null>(null);
  const [inspect, setInspect] = useState<InspectResult | null>(null);
  const [busy, setBusy] = useState<BusyAction>(null);
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(
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

  /** Re-reads the folder's icon state and eligibility. */
  function refreshInspect(path: string) {
    invoke<InspectResult>("folder_icon_inspect", { path })
      .then((info) => setInspect(info))
      .catch((error) => {
        setInspect(null);
        setResult({ ok: false, text: errorText(error) });
      });
  }

  async function pickFolder() {
    setBusy("pick");
    setResult(null);
    try {
      const picked = await invoke<string | null>("folder_icon_pick_folder");
      if (picked) {
        setFolder(picked);
        setInspect(null);
        refreshInspect(picked);
      }
    } catch (error) {
      setResult({ ok: false, text: errorText(error) });
    } finally {
      setBusy(null);
    }
  }

  async function apply() {
    if (folder === null) return;
    setBusy("apply");
    setResult(null);
    const settings: IconSettingsInput = {
      baseColor,
      pattern,
      patternColor,
      patternSize,
      patternOpacity,
    };
    try {
      const message = await invoke<string>("folder_icon_apply", {
        path: folder,
        settings,
      });
      setResult({ ok: true, text: message });
    } catch (error) {
      setResult({ ok: false, text: errorText(error) });
    } finally {
      setBusy(null);
      refreshInspect(folder);
    }
  }

  async function restore() {
    if (folder === null) return;
    setBusy("restore");
    setResult(null);
    try {
      const message = await invoke<string>("folder_icon_restore", {
        path: folder,
      });
      setResult({ ok: true, text: message });
    } catch (error) {
      setResult({ ok: false, text: errorText(error) });
    } finally {
      setBusy(null);
      refreshInspect(folder);
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

  // Folder panel: eligibility + current icon state as one status line.
  const eligibility = inspect?.eligibility ?? null;
  const stateNames: Record<InspectResult["state"], string> = {
    default: "Default icon",
    custom_by_erx: "Icon set by this tool",
    custom_by_other: "Custom icon (another program)",
  };
  const statusLine = (() => {
    if (folder === null) return null;
    if (eligibility === null) return "Checking the folder…";
    if (eligibility.status === "blocked") {
      return `Blocked — ${eligibility.reason ?? ""}`;
    }
    if (eligibility.status === "warning") {
      return `Warning — ${eligibility.warning ?? ""}`;
    }
    return inspect ? stateNames[inspect.state] : null;
  })();
  const canApply =
    folder !== null && eligibility?.status !== "blocked" && busy === null;
  const canRestore = folder !== null && busy === null;

  return (
    <div className="fi-layout">
      {/* ----- left: one folder (drop zone + list arrive in 4C) ----- */}
      <section className="fi-main" aria-label="Folder">
        <div className="fi-folder-panel">
          <button
            type="button"
            className="button-primary fi-add-button"
            onClick={pickFolder}
            disabled={busy !== null}
          >
            {busy === "pick" ? "Opening…" : "Add folder"}
          </button>

          {folder !== null && (
            <>
              <p
                className="fi-folder-path"
                title={folder}
                aria-label="Selected folder"
              >
                {folder}
              </p>
              <p
                className={
                  eligibility?.status === "blocked"
                    ? "fi-folder-status fi-status-blocked"
                    : "fi-folder-status"
                }
                aria-live="polite"
              >
                {statusLine}
              </p>
              <div className="fi-folder-actions">
                <button
                  type="button"
                  className="button-primary"
                  onClick={apply}
                  disabled={!canApply}
                >
                  {busy === "apply" ? "Applying…" : "Apply icon"}
                </button>
                <button
                  type="button"
                  className="button-secondary"
                  onClick={restore}
                  disabled={!canRestore}
                >
                  {busy === "restore" ? "Restoring…" : "Restore default icon"}
                </button>
              </div>
            </>
          )}

          {result !== null && (
            <p
              className={result.ok ? "fi-result" : "fi-result fi-result-error"}
              role={result.ok ? "status" : "alert"}
            >
              {result.text}
            </p>
          )}
        </div>
        <p className="fi-placeholder-note">
          Drag &amp; drop and multiple folders arrive with the next update of
          this tool.
        </p>
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
