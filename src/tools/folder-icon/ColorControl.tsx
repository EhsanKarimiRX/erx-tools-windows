import { useState } from "react";
import { PRESET_COLORS, expandHex } from "./types";

interface ColorControlProps {
  /** Stable id prefix for the inputs. */
  id: string;
  /** Field label, e.g. "Base color". */
  label: string;
  /** The current valid color, always `#rrggbb`. */
  value: string;
  /** Called with a newly valid color; the last valid value stays otherwise. */
  onColor: (color: string) => void;
  /** Disabled while the pattern is off (used for the pattern color). */
  disabled?: boolean;
}

/** Preset swatches + free color picker + HEX field for one color.
 *
 * The text field accepts `#RGB` and `#RRGGBB`. Invalid input shows an
 * inline error and keeps the last valid color; Rust re-validates whatever
 * we send. */
export default function ColorControl({
  id,
  label,
  value,
  onColor,
  disabled = false,
}: ColorControlProps) {
  // Raw text as typed; stays independent of the last valid value so the
  // user can clear or half-type without the field snapping back.
  const [text, setText] = useState(value);
  const [error, setError] = useState<string | null>(null);

  function handleText(next: string) {
    setText(next);
    const hex = expandHex(next);
    if (hex !== null) {
      setError(null);
      onColor(hex);
    } else if (next.trim() === "") {
      // Clearing to type a fresh value: not an error until blur.
      setError(null);
    } else {
      setError("Use a color in the #RRGGBB form (or short #RGB).");
    }
  }

  function handleBlur() {
    // Common paste: hex without the #. Add it automatically.
    if (/^[0-9a-fA-F]{6}$/.test(text.trim()) || /^[0-9a-fA-F]{3}$/.test(text.trim())) {
      handleText(`#${text.trim()}`);
      return;
    }
    if (expandHex(text) === null) {
      setError("Use a color in the #RRGGBB form (or short #RGB).");
      setText(value); // snap the visible text back to the last valid color
    }
  }

  return (
    <div className="fi-color-control">
      <div className="fi-color-row">
        <label className="fi-label" htmlFor={`${id}-picker`}>
          {label}
        </label>
        <div className="fi-color-inputs">
          <input
            id={`${id}-picker`}
            type="color"
            className="fi-color-picker"
            value={value}
            onChange={(event) => {
              setText(event.target.value);
              setError(null);
              onColor(event.target.value);
            }}
            disabled={disabled}
            aria-label={`${label} picker`}
          />
          <input
            type="text"
            className={error === null ? "fi-hex-input" : "fi-hex-input fi-hex-invalid"}
            value={text}
            onChange={(event) => handleText(event.target.value)}
            onBlur={handleBlur}
            maxLength={7}
            spellCheck={false}
            disabled={disabled}
            aria-label={`${label} as hex`}
            aria-invalid={error !== null}
          />
        </div>
      </div>
      {error !== null && !disabled && (
        <p className="fi-hex-error" role="alert">
          {error}
        </p>
      )}
      <div className="fi-presets" role="group" aria-label={`${label} presets`}>
        {PRESET_COLORS.map((preset) => (
          <button
            key={preset.hex}
            type="button"
            className={
              value.toLowerCase() === preset.hex.toLowerCase()
                ? "fi-swatch fi-swatch-active"
                : "fi-swatch"
            }
            style={{ backgroundColor: preset.hex }}
            onClick={() => {
              setText(preset.hex);
              setError(null);
              onColor(preset.hex);
            }}
            disabled={disabled}
            aria-pressed={value.toLowerCase() === preset.hex.toLowerCase()}
            aria-label={`${label}: ${preset.name}`}
            title={preset.name}
          />
        ))}
      </div>
    </div>
  );
}
