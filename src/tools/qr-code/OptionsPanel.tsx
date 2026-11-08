import { useEffect, useRef, useState } from "react";
import type { QrOptions } from "./types";
import {
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
  /** Called on every change: the parsed size when the typed value is a
   * valid in-range integer, null while the field is invalid/in progress. */
  onPngSize: (value: number | null) => void;
  /** Canvas side in modules (grid + quiet zone) of the current preview. */
  totalModules: number | null;
  /** With a logo (3C) the level is forced to H and locked. */
  levelLocked: boolean;
}

const LEVELS: { value: QrOptions["errorCorrection"]; label: string }[] = [
  { value: "L", label: "L" },
  { value: "M", label: "M" },
  { value: "Q", label: "Q" },
  { value: "H", label: "H" },
];

/** How long the "not available while a logo is added" note stays up. */
const LOCKED_NOTE_MS = 4000;

/** Style, colors, error correction, margin and export size controls. */
export default function OptionsPanel(props: OptionsPanelProps) {
  // The typed export size: kept as raw text while typing so the user can
  // always type freely (no per-keystroke clamping). Only a valid in-range
  // integer is sent up; everything else reports null until it is fixed.
  const [rawSize, setRawSize] = useState(String(props.pngSize));
  const [sizeError, setSizeError] = useState(false);

  // The "not available" note for locked error-correction levels: shown on
  // activation of a locked level, auto-hidden after a few seconds, never
  // stacked (another click only resets the timer).
  const [lockedNote, setLockedNote] = useState(false);
  const noteTimer = useRef<number | null>(null);

  function clearNoteTimer() {
    if (noteTimer.current !== null) {
      window.clearTimeout(noteTimer.current);
      noteTimer.current = null;
    }
  }

  // Removing the logo hides the note at once; unmount never leaves a
  // timer behind.
  useEffect(() => {
    if (!props.levelLocked) {
      clearNoteTimer();
      setLockedNote(false);
    }
    return clearNoteTimer;
  }, [props.levelLocked]);

  /** A locked level stays focusable and announces why it does nothing;
   * unlocked levels pass straight through. */
  function onLevelClick(value: QrOptions["errorCorrection"]) {
    if (props.levelLocked && value !== "H") {
      setLockedNote(true);
      clearNoteTimer();
      noteTimer.current = window.setTimeout(() => {
        setLockedNote(false);
        noteTimer.current = null;
      }, LOCKED_NOTE_MS);
      return;
    }
    props.onErrorCorrection(value);
  }

  // Pixels per module the export size would give for the current code —
  // used only for the "may be hard to scan" hint.
  const pixelsPerModule =
    props.totalModules !== null ? props.pngSize / props.totalModules : null;

  /** Digits only, no sign or decimal point; returns the size when the
   * text is a whole number inside the allowed range. */
  function parseExportSize(text: string): number | null {
    if (!/^\d{1,4}$/.test(text)) return null;
    const parsed = Number(text);
    return parsed >= MIN_PNG_SIZE && parsed <= MAX_PNG_SIZE ? parsed : null;
  }

  function onSizeChange(text: string) {
    const digits = text.replace(/\D/g, "").slice(0, 4);
    setRawSize(digits);
    setSizeError(false);
    props.onPngSize(parseExportSize(digits));
  }

  /** Blur/Enter: accept and normalize a valid value ("01080" -> "1080"),
   * or show the inline error and revert the text to the last valid size. */
  function commitSize() {
    if (parseExportSize(rawSize) === null) {
      setSizeError(true);
      setRawSize(String(props.pngSize));
      props.onPngSize(props.pngSize); // field is valid again
    } else {
      setRawSize(String(Number(rawSize)));
    }
  }

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
          {LEVELS.map(({ value, label }) => {
            const active = props.errorCorrection === value;
            const locked = props.levelLocked && value !== "H";
            return (
              <button
                key={value}
                type="button"
                role="radio"
                aria-checked={active}
                aria-disabled={locked || undefined}
                className={
                  active
                    ? "qr-seg qr-seg-active"
                    : locked
                      ? "qr-seg qr-seg-locked"
                      : "qr-seg"
                }
                onClick={() => onLevelClick(value)}
              >
                {label}
              </button>
            );
          })}
        </div>
        <p className="qr-hint">
          Higher levels survive damage better but make the code denser.
          {props.levelLocked
            ? " H is required while a logo covers the center."
            : " Q is a good default."}
        </p>
        {props.levelLocked && lockedNote && (
          <p className="qr-hint-warning" role="status" aria-live="polite">
            Not available while a logo is added. Logos need error correction
            level H.
          </p>
        )}
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

      {/* ----- export size ----- */}
      <div className="qr-fieldset">
        <div className="qr-color-row">
          <label className="qr-label" htmlFor="qr-png-size">
            Export size (PNG)
          </label>
          <div className="qr-color-controls">
            <input
              id="qr-png-size"
              type="text"
              inputMode="numeric"
              className={`qr-hex-input${sizeError ? " qr-hex-invalid" : ""}`}
              value={rawSize}
              onChange={(event) => onSizeChange(event.target.value)}
              onBlur={commitSize}
              onKeyDown={(event) => {
                if (event.key === "Enter") commitSize();
              }}
              maxLength={4}
              spellCheck={false}
              aria-label="Export size in pixels"
              aria-invalid={sizeError}
              aria-describedby={sizeError ? "qr-png-size-error" : undefined}
            />
            <span className="qr-value">px</span>
          </div>
        </div>
        {sizeError ? (
          <p id="qr-png-size-error" className="qr-error" role="alert">
            Enter a size between 256 and 4096 px
          </p>
        ) : (
          <p className="qr-hint">
            saves at {props.pngSize} × {props.pngSize} px
          </p>
        )}
        <p className="qr-hint">
          Preview is not to scale. This only changes the saved PNG. SVG is
          always scalable.
        </p>
        {pixelsPerModule !== null && pixelsPerModule < 4 && (
          <p className="qr-hint-warning">Small size: code may be hard to scan.</p>
        )}
      </div>
    </>
  );
}
