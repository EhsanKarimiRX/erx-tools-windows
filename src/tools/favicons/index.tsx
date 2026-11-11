import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import DropZone from "../../shared/DropZone";
import OptionsPanel from "./OptionsPanel";
import PreviewGrid from "./PreviewGrid";
import SavedPanel from "./SavedPanel";
import SnippetPanel from "./SnippetPanel";
import type { FaviconsOptions, PreviewResult, SaveResult, SourceInfo } from "./types";
import { DEFAULT_OPTIONS, SUPPORTED_FORMATS, errorText, isValidHex } from "./types";
import "./favicons.css";

/** Favicons: drop one logo, preview the icon set, save the favicon package. */
export default function Favicons() {
  const [source, setSource] = useState<SourceInfo | null>(null);
  const [options, setOptions] = useState<FaviconsOptions>(DEFAULT_OPTIONS);
  const [preview, setPreview] = useState<PreviewResult | null>(null);
  const [previewing, setPreviewing] = useState(false);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [loadingSource, setLoadingSource] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saveResult, setSaveResult] = useState<SaveResult | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  const optionsValid = isValidHex(options.backgroundColor) && isValidHex(options.themeColor);
  const busy = loadingSource || saving;

  const loadSource = useCallback(async (path: string, note?: string) => {
    setLoadingSource(true);
    try {
      const info = await invoke<SourceInfo>("favicons_inspect_source", { path });
      setSource(info);
      setPreview(null);
      setPreviewError(null);
      setSaveResult(null);
      setMessage(note ?? null);
    } catch (error) {
      setMessage(errorText(error));
    } finally {
      setLoadingSource(false);
    }
  }, []);

  const handleFiles = useCallback(
    (paths: string[]) => {
      if (paths.length === 0 || busy) return;
      const note = paths.length > 1 ? "Only the first file was used. Favicons works with one image at a time." : undefined;
      void loadSource(paths[0], note);
    },
    [busy, loadSource],
  );

  const browse = useCallback(async () => {
    if (busy) return;
    try {
      const picked = await invoke<string | null>("favicons_pick_source");
      if (picked) await loadSource(picked);
    } catch (error) {
      setMessage(`Could not open the file dialog: ${errorText(error)}`);
    }
  }, [busy, loadSource]);

  const updateOptions = useCallback((patch: Partial<FaviconsOptions>) => {
    setOptions((previous) => ({ ...previous, ...patch }));
    setSaveResult(null);
  }, []);

  // Debounced preview. The cleanup marks older requests as stale so a slow
  // answer can never overwrite a newer one.
  useEffect(() => {
    if (source === null || !optionsValid) {
      setPreviewing(false);
      return;
    }
    let cancelled = false;
    const timer = window.setTimeout(async () => {
      setPreviewing(true);
      try {
        const result = await invoke<PreviewResult>("favicons_preview", { path: source.path, options });
        if (!cancelled) {
          setPreview(result);
          setPreviewError(null);
        }
      } catch (error) {
        if (!cancelled) setPreviewError(errorText(error));
      } finally {
        if (!cancelled) setPreviewing(false);
      }
    }, 250);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [source, options, optionsValid]);

  const save = async () => {
    if (source === null || !optionsValid || busy) return;
    setSaving(true);
    setMessage(null);
    try {
      const result = await invoke<SaveResult | null>("favicons_save", { path: source.path, options });
      if (result) setSaveResult(result); // null = the user cancelled the folder dialog
    } catch (error) {
      setMessage(errorText(error));
    } finally {
      setSaving(false);
    }
  };

  const reveal = async () => {
    if (saveResult === null) return;
    try {
      await invoke("favicons_reveal_folder", { folder: saveResult.folder });
    } catch (error) {
      setMessage(errorText(error));
    }
  };

  const warnings = preview?.warnings ?? source?.warnings ?? [];

  if (source === null) {
    return (
      <div className="fav">
        <div className="fav-empty">
          <DropZone
            label="Drag your logo here"
            hint={`Formats: ${SUPPORTED_FORMATS}`}
            browseLabel="Choose image"
            onFiles={handleFiles}
            onBrowse={browse}
            disabled={busy}
          />
          <p className="fav-note fav-center">
            A favicon is the small icon shown in browser tabs and bookmarks. Drop one logo (best: a square image of at
            least 512×512 pixels, or an SVG) and get a complete favicon package: favicon.ico, PNG icons, an Apple touch
            icon, Android icons, a web manifest and an HTML snippet.
          </p>
          <p className="fav-note fav-center">Everything is created on your computer. Nothing is uploaded.</p>
          {loadingSource && (
            <p className="fav-note fav-center" role="status">
              Reading the image…
            </p>
          )}
          {message && (
            <div className="fav-error" role="alert">
              <p>{message}</p>
            </div>
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="fav">
      <div className="fav-layout">
        <div className="fav-main">
          <DropZone
            className="fav-dropzone-compact"
            label="Drop another image here to replace this one"
            hint={`Formats: ${SUPPORTED_FORMATS}`}
            browseLabel="Choose image"
            onFiles={handleFiles}
            onBrowse={browse}
            disabled={busy}
          />

          {message && (
            <div className="fav-error" role="alert">
              <p>{message}</p>
            </div>
          )}

          <section className="fav-card fav-source" aria-label="Selected image">
            <div className="fav-thumb fav-checker">
              <img src={source.previewDataUrl} alt="Preview of the selected logo" />
            </div>
            <div className="fav-source-info">
              <p className="fav-source-name" title={source.path}>
                {source.fileName}
              </p>
              <p className="fav-note">
                {source.formatLabel} · {source.width}×{source.height} px
              </p>
              {warnings.length > 0 && (
                <div className="fav-notice" role="status">
                  {warnings.map((warning) => (
                    <p key={warning}>{warning}</p>
                  ))}
                </div>
              )}
            </div>
          </section>

          {saveResult && <SavedPanel result={saveResult} onReveal={reveal} />}

          {previewError && (
            <div className="fav-error" role="alert">
              <p>Preview failed: {previewError}</p>
            </div>
          )}

          {preview ? (
            <PreviewGrid icons={preview.icons} busy={previewing} />
          ) : (
            previewing && (
              <p className="fav-note" role="status">
                Generating the preview…
              </p>
            )
          )}

          {preview && <SnippetPanel snippet={preview.htmlSnippet} />}
        </div>

        <div className="fav-side">
          <OptionsPanel options={options} onChange={updateOptions} disabled={saving} />
          <div className="fav-card">
            <button
              type="button"
              className="button-primary fav-save"
              onClick={save}
              disabled={busy || !optionsValid}
            >
              {saving ? "Saving…" : "Create favicon package"}
            </button>
            <p className={optionsValid ? "fav-hint" : "fav-hint fav-hint-error"}>
              {optionsValid
                ? "You will choose the folder where the package is saved."
                : "Fix the highlighted colors first."}
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}
