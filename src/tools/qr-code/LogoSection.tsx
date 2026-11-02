import { invoke } from "@tauri-apps/api/core";
import DropZone from "../../shared/DropZone";
import type { PreparedLogo } from "./types";
import {
  DEFAULT_LOGO_PERCENT,
  MAX_LOGO_PERCENT,
  MIN_LOGO_PERCENT,
} from "./types";

interface LogoSectionProps {
  logo: PreparedLogo | null;
  onLogo: (logo: PreparedLogo | null) => void;
  /** Percent of the QR width (10..20); fraction is computed on top. */
  percent: number;
  onPercent: (value: number) => void;
  /** Shown when Rust reports a near-maximum-size warning. */
  nearMaxWarning: string | null;
  /** Double-guarded: the level is locked to H while a logo is present. */
  onLogoForcesLevelH: () => void;
  /** Set when loading a logo fails; shown by the parent. */
  onError: (message: string) => void;
}

function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

/** The optional centered logo: pick or drop a local image, choose its
 * size, remove it again. The file is decoded and prepared once by Rust;
 * the UI only ever holds the downscaled PNG. */
export default function LogoSection(props: LogoSectionProps) {
  async function loadPath(path: string) {
    try {
      const prepared = await invoke<{ edge: number; pngBase64: string }>(
        "load_logo",
        { path },
      );
      props.onLogo({ ...prepared, fileName: fileNameOf(path) });
      props.onLogoForcesLevelH();
    } catch (error) {
      props.onError(errorText(error));
    }
  }

  if (props.logo === null) {
    return (
      <div className="qr-fieldset">
        <span className="qr-label">Logo (optional)</span>
        <DropZone
          className="qr-logo-dropzone"
          label="Drop a logo image here"
          hint="PNG, JPEG or WebP. Saved SVG and PNG files will contain it."
          browseLabel="Choose logo file"
          onBrowse={async () => {
            const path = await invoke<string | null>("pick_logo_file");
            if (path) await loadPath(path);
          }}
          onFiles={(paths) => {
            if (paths.length > 0) void loadPath(paths[0]);
          }}
        />
      </div>
    );
  }

  return (
    <div className="qr-fieldset">
      <div className="qr-logo-header">
        <span className="qr-label">Logo</span>
        <button
          type="button"
          className="button-secondary qr-preset"
          onClick={() => props.onLogo(null)}
        >
          Remove logo
        </button>
      </div>
      <div className="qr-logo-card">
        <img
          className="qr-logo-thumbnail"
          src={`data:image/png;base64,${props.logo.pngBase64}`}
          alt="Logo to embed in the QR code"
        />
        <div className="qr-logo-info">
          <p className="qr-logo-name">{props.logo.fileName}</p>
          <p className="qr-hint">
            Level H is used while a logo is present (locked).
          </p>
        </div>
      </div>
      <div className="qr-color-row">
        <label className="qr-label" htmlFor="qr-logo-size">
          Logo size
        </label>
        <span className="qr-value">{props.percent}% of the code</span>
      </div>
      <input
        id="qr-logo-size"
        type="range"
        className="qr-range"
        min={MIN_LOGO_PERCENT}
        max={MAX_LOGO_PERCENT}
        step={1}
        value={props.percent}
        onChange={(event) => props.onPercent(Number(event.target.value))}
      />
      <p className="qr-hint">
        The modules under the logo are removed. Bigger logos make scanning
        harder; {DEFAULT_LOGO_PERCENT}% works well.
      </p>
      {props.nearMaxWarning && (
        <p className="qr-error" role="alert">
          {props.nearMaxWarning}
        </p>
      )}
    </div>
  );
}

function fileNameOf(path: string): string {
  const index = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  return index >= 0 ? path.slice(index + 1) : path;
}
