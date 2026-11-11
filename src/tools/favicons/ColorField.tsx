import { isValidHex, toPickerColor } from "./types";

interface ColorFieldProps {
  id: string;
  label: string;
  hint: string;
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
}

/** Color picker + hex text field. The text may be temporarily invalid while
 * the user types; the parent blocks the preview until it is valid again. */
export default function ColorField({ id, label, hint, value, onChange, disabled }: ColorFieldProps) {
  const valid = isValidHex(value);
  return (
    <div className="fav-field">
      <label className="fav-label" htmlFor={`${id}-text`}>
        {label}
      </label>
      <div className="fav-color-row">
        <input
          type="color"
          className="fav-color-swatch"
          aria-label={`${label}: pick a color`}
          value={toPickerColor(value)}
          onChange={(event) => onChange(event.target.value)}
          disabled={disabled}
        />
        <input
          id={`${id}-text`}
          type="text"
          className="fav-input fav-hex"
          value={value}
          maxLength={7}
          spellCheck={false}
          autoComplete="off"
          aria-invalid={!valid}
          aria-describedby={`${id}-hint`}
          onChange={(event) => onChange(event.target.value)}
          disabled={disabled}
        />
      </div>
      <p id={`${id}-hint`} className={valid ? "fav-hint" : "fav-hint fav-hint-error"}>
        {valid ? hint : "Use a color like #ff8800 or #f80."}
      </p>
    </div>
  );
}
