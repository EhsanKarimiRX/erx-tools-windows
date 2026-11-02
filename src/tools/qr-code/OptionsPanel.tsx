import type { QrOptions } from "./types";
import {
  DEFAULT_PNG_SIZE,
  MAX_MARGIN,
  MAX_PNG_SIZE,
  MIN_MARGIN,
  MIN_PNG_SIZE,
  isHexColor,
} from "./types";

interface OptionsPanelProps {
  style: QrOptions["style"];
  onStyle: (value: QrOptions["style"]) => void;
  foreground: string;
  onForeground: (value: string) => void;
  background: string;
  onBackground: (value: string) => void;
  transparent: boolean;
  onTransparent: (value: boolean) => void;
  errorCorrection: QrOptions["errorCorrection"];
  onErrorCorrection: (value: QrOptions["errorCorrection"]) => void;
  margin: number;
  onMargin: (value: number) => void;
  pngSize: number;
  onPngSize: (value: number) => void;
  /** Shown exactly as Rust reports it (snapped to whole modules). */
  pngPixels: number | null;
  pngPixelsPerModule: number | null;
  /** With a logo (3C) the level is forced to H and locked. */
  levelLocked: boolean;
}

const LEVELS: { value: QrOptions["errorCorrection"]; label: string }[] = [
  { value: "L", label: "L" },
  { value: "M", label: "M" },
  { value: "Q", label: "Q" },
  { value: "H", label: "H" },
];

/** Style, colors, error correction, margin and PNG size controls. */
export default function OptionsPanel(props: OptionsPanelProps) {
  return (
    <>
      {/* ----- style ----- */}
      <div className="qr-fieldset">
        <span className="qr-label">Style</span>
        <div className="qr-segmented" role="radiogroup" aria-label="Style">
          <button
            type="button"
            role="radio"
            aria-checked={props.style === "sharp"}
            className={props.style === "sharp" ? "qr-seg qr-seg-active" : "qr-seg"}
            onClick={() => props.onStyle("sharp")}
          >
            Sharp
          </button>
          <button
            type="button"
            role="radio"
            aria-checked={props.style === "round"}
            className={props.style === "round" ? "qr-seg qr-seg-active" : "qr-seg"}
            onClick={() => props.onStyle("round")}
          >
            Rounded
          </button>
        </div>
      </div>

      {/* ----- colors ----- */}
      <div className="qr-fieldset">
        <div className="qr-color-row">
          <label className="qr-label" htmlFor="qr-fg-color">
            Code color
          </label>
          <div className="qr-color-controls">
            <input
              id="qr-fg-color"
              type="color"
              className="qr-color-picker"
              value={isHexColor(props.foreground) ? props.foreground : "#000000"}
              onChange={(event) => props.onForeground(event.target.value)}
              aria-label="Pick the code color"
            />
            <input
              type="text"
              className={
                isHexColor(props.foreground)
                  ? "qr-hex-input"
                  : "qr-hex-input qr-hex-invalid"
              }
              value={props.foreground}
              onChange={(event) => props.onForeground(event.target.value)}
              onBlur={(event) => {
                // Common paste: hex without the #. Add it automatically.
                if (/^[0-9a-fA-F]{6}$/.test(event.target.value)) {
                  props.onForeground(`#${event.target.value}`);
                }
              }}
              maxLength={7}
              spellCheck={false}
              aria-label="Code color as #RRGGBB"
            />
          </div>
        </div>

        <div className="qr-color-row">
          <label className="qr-label" htmlFor="qr-bg-color">
            Background
          </label>
          <div className="qr-color-controls">
            <input
              id="qr-bg-color"
              type="color"
              className="qr-color-picker"
              value={isHexColor(props.background) ? props.background : "#ffffff"}
              onChange={(event) => props.onBackground(event.target.value)}
              disabled={props.transparent}
              aria-label="Pick the background color"
            />
            <input
              type="text"
              className={
                props.transparent || isHexColor(props.background)
                  ? "qr-hex-input"
                  : "qr-hex-input qr-hex-invalid"
              }
              value={props.background}
              onChange={(event) => props.onBackground(event.target.value)}
              onBlur={(event) => {
                if (/^[0-9a-fA-F]{6}$/.test(event.target.value)) {
                  props.onBackground(`#${event.target.value}`);
                }
              }}
              maxLength={7}
              spellCheck={false}
              disabled={props.transparent}
              aria-label="Background color as #RRGGBB"
            />
          </div>
        </div>

        <div className="qr-presets">
          <button
            type="button"
            className="button-secondary qr-preset"
            onClick={() => {
              props.onForeground("#000000");
              props.onBackground("#ffffff");
              props.onTransparent(false);
            }}
          >
            Black on white
          </button>
          <button
            type="button"
            className="button-secondary qr-preset"
            onClick={() => {
              props.onForeground("#ffffff");
              props.onBackground("#000000");
              props.onTransparent(false);
            }}
          >
            White on black
          </button>
        </div>

        <label className="qr-checkbox">
          <input
            type="checkbox"
            checked={props.transparent}
            onChange={(event) => props.onTransparent(event.target.checked)}
          />
          Transparent background
        </label>
      </div>

      {/* ----- error correction ----- */}
      <div className="qr-fieldset">
        <span className="qr-label">Error correction</span>
        <div
          className="qr-segmented"
          role="radiogroup"
          aria-label="Error correction level"
        >
          {LEVELS.map(({ value, label }) => (
            <button
              key={value}
              type="button"
              role="radio"
              aria-checked={props.errorCorrection === value}
              className={
                props.errorCorrection === value
                  ? "qr-seg qr-seg-active"
                  : "qr-seg"
              }
              disabled={props.levelLocked && value !== "H"}
              onClick={() => props.onErrorCorrection(value)}
            >
              {label}
            </button>
          ))}
        </div>
        <p className="qr-hint">
          Higher levels survive damage better but make the code denser.
          {props.levelLocked
            ? " H is required while a logo covers the center."
            : " Q is a good default."}
        </p>
      </div>

      {/* ----- margin ----- */}
      <div className="qr-fieldset">
        <div className="qr-color-row">
          <label className="qr-label" htmlFor="qr-margin">
            Quiet-zone margin
          </label>
          <span className="qr-value">{props.margin} modules</span>
        </div>
        <input
          id="qr-margin"
          type="range"
          className="qr-range"
          min={MIN_MARGIN}
          max={MAX_MARGIN}
          step={1}
          value={props.margin}
          onChange={(event) => props.onMargin(Number(event.target.value))}
        />
        <p className="qr-hint">
          The empty border around the code. Small margins can hurt scanning.
        </p>
      </div>

      {/* ----- PNG size ----- */}
      <div className="qr-fieldset">
        <div className="qr-color-row">
          <label className="qr-label" htmlFor="qr-png-size">
            PNG size
          </label>
          <span className="qr-value">
            {props.pngPixels !== null && props.pngPixelsPerModule !== null
              ? `saves at ${props.pngPixels} × ${props.pngPixels} px (${props.pngPixelsPerModule} px/module)`
              : `${props.pngSize} px requested`}
          </span>
        </div>
        <input
          id="qr-png-size"
          type="range"
          className="qr-range"
          min={MIN_PNG_SIZE}
          max={MAX_PNG_SIZE}
          step={16}
          value={props.pngSize}
          onChange={(event) => props.onPngSize(Number(event.target.value))}
        />
        <p className="qr-hint">
          Snapped down to a whole number of pixels per module so every edge
          stays perfectly sharp. {DEFAULT_PNG_SIZE} px is plenty for most
          uses.
        </p>
      </div>
    </>
  );
}
