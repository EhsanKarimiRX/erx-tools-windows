import { invoke } from "@tauri-apps/api/core";
import type { ConvertOptions, OutputFormat } from "./types";

interface OptionsPanelProps {
  options: ConvertOptions;
  onChange: (options: ConvertOptions) => void;
  disabled: boolean;
}

const FORMATS: { value: OutputFormat; label: string }[] = [
  { value: "jpeg", label: "JPEG" },
  { value: "png", label: "PNG" },
  { value: "webp", label: "WebP (lossless)" },
  { value: "bmp", label: "BMP" },
  { value: "tiff", label: "TIFF" },
];

/** Locked while converting (disabled props on every control). */
export default function OptionsPanel({ options, onChange, disabled }: OptionsPanelProps) {
  const needsQuality = options.format === "jpeg";
  const needsBackground = options.format === "jpeg" || options.format === "bmp";

  /** Opens the native folder picker; selecting "custom" also goes through
   * here, so canceling the dialog simply keeps the previous location. */
  const pickFolder = async () => {
    const folder = await invoke<string | null>("pick_output_folder");
    if (folder) {
      onChange({ ...options, outputFolder: folder });
    }
  };

  return (
    <div className="ic-options">
      <div className="ic-field">
        <label htmlFor="ic-format">Output format</label>
        <select
          id="ic-format"
          value={options.format}
          disabled={disabled}
          onChange={(event) =>
            onChange({ ...options, format: event.target.value as OutputFormat })
          }
        >
          {FORMATS.map((format) => (
            <option key={format.value} value={format.value}>
              {format.label}
            </option>
          ))}
        </select>
      </div>

      {needsQuality && (
        <div className="ic-field">
          <label htmlFor="ic-quality">JPEG quality: {options.jpegQuality}</label>
          <input
            id="ic-quality"
            type="range"
            min={1}
            max={100}
            value={options.jpegQuality}
            disabled={disabled}
            onChange={(event) =>
              onChange({ ...options, jpegQuality: Number(event.target.value) })
            }
          />
        </div>
      )}

      {options.format === "webp" && (
        <p className="ic-note">WebP output is lossless in this version.</p>
      )}

      {needsBackground && (
        <div className="ic-field">
          <label htmlFor="ic-background">Background for transparency</label>
          <div className="ic-inline">
            <input
              id="ic-background"
              type="color"
              value={options.background}
              disabled={disabled}
              onChange={(event) =>
                onChange({ ...options, background: event.target.value })
              }
            />
            <span className="ic-note">
              Used when the image has transparency (this format cannot store it).
            </span>
          </div>
        </div>
      )}

      <fieldset className="ic-fieldset" disabled={disabled}>
        <legend className="ic-legend">Output location</legend>
        <label className="ic-choice">
          <input
            type="radio"
            name="ic-location"
            checked={options.outputFolder === null}
            onChange={() => onChange({ ...options, outputFolder: null })}
          />
          Same folder as each original
        </label>
        <label className="ic-choice">
          <input
            type="radio"
            name="ic-location"
            checked={options.outputFolder !== null}
            onChange={pickFolder}
          />
          One chosen folder
        </label>
        {options.outputFolder !== null && (
          <div className="ic-inline">
            <button type="button" className="button-secondary" onClick={pickFolder}>
              Browse…
            </button>
            <span className="ic-path">{options.outputFolder}</span>
          </div>
        )}
      </fieldset>

      <p className="ic-note">
        Outputs contain no metadata (EXIF and GPS are removed). Camera rotation
        is applied automatically. Originals are never modified. Wide-color
        (Display P3) photos, such as iPhone photos, are saved without a color
        profile and may look slightly less vivid.
      </p>
      <p className="ic-note">
        HEIC/HEIF (iPhone photos) is read with the codecs installed in
        Windows. If a HEIC photo cannot be opened, install Microsoft’s “HEIF
        Image Extensions” and “HEVC Video Extensions” from the Microsoft
        Store.
      </p>
    </div>
  );
}
