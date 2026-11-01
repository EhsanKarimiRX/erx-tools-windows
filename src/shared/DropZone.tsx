import { useEffect, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";

interface DropZoneProps {
  /** Main call to action, e.g. "Drag images here". */
  label: string;
  /** Extra line under the label, e.g. the supported formats. */
  hint?: string;
  /** Text of the optional browse button ("Add files"). */
  browseLabel?: string;
  /** Called when the browse button is clicked. */
  onBrowse?: () => void;
  /** Called with absolute paths when files are dropped on the window. */
  onFiles: (paths: string[]) => void;
  /** Dim and ignore drops/clicks (e.g. while a batch is running). */
  disabled?: boolean;
  /** Extra class so tools can add their own variants/spacing. */
  className?: string;
}

/**
 * Generic file drop zone shared by all tools.
 *
 * Uses Tauri's own drag-and-drop event (`onDragDropEvent`), so real absolute
 * file paths reach the app. The event is window-wide: the highlight shows
 * wherever the files are dragged over the window, and drops anywhere are
 * forwarded. Paths are passed through unfiltered — Rust decides what each
 * file really is (content sniffing), never the extension.
 */
export default function DropZone({
  label,
  hint,
  browseLabel = "Add files",
  onBrowse,
  onFiles,
  disabled = false,
  className,
}: DropZoneProps) {
  const [dragActive, setDragActive] = useState(false);

  // Keep the latest callbacks available to the long-lived event listener
  // without resubscribing on every render.
  const onFilesRef = useRef(onFiles);
  const disabledRef = useRef(disabled);
  useEffect(() => {
    onFilesRef.current = onFiles;
    disabledRef.current = disabled;
  });

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        if (disabledRef.current) return;
        const payload = event.payload;
        switch (payload.type) {
          case "enter":
            setDragActive(true);
            break;
          case "over":
            break; // keep the highlight while moving over the window
          case "leave":
            setDragActive(false);
            break;
          case "drop":
            setDragActive(false);
            onFilesRef.current(payload.paths);
            break;
        }
      })
      .then((unlistenFn) => {
        if (disposed) {
          unlistenFn();
        } else {
          unlisten = unlistenFn;
        }
      })
      .catch(() => {
        // Not running inside a Tauri webview (plain browser preview):
        // drops simply stay unavailable.
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const classes = [
    "dropzone",
    dragActive ? "dropzone-active" : "",
    disabled ? "dropzone-disabled" : "",
    className ?? "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <div className={classes} aria-disabled={disabled}>
      <p className="dropzone-label">{label}</p>
      {hint && <p className="dropzone-hint">{hint}</p>}
      {onBrowse && (
        <button
          type="button"
          className="button-primary dropzone-button"
          onClick={onBrowse}
          disabled={disabled}
        >
          {browseLabel}
        </button>
      )}
    </div>
  );
}
