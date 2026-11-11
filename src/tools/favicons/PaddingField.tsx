import { PADDING_MAX, PADDING_MIN, clampPadding } from "./types";

interface PaddingFieldProps {
  id: string;
  label: string;
  hint: string;
  value: number;
  onChange: (value: number) => void;
  disabled?: boolean;
}

/** A 0-45 % padding control: slider + number box, kept in sync. */
export default function PaddingField({ id, label, hint, value, onChange, disabled }: PaddingFieldProps) {
  return (
    <div className="fav-field">
      <label className="fav-label" htmlFor={id}>
        {label}: {value}%
      </label>
      <div className="fav-padding-row">
        <input
          id={id}
          type="range"
          min={PADDING_MIN}
          max={PADDING_MAX}
          step={1}
          value={value}
          aria-describedby={`${id}-hint`}
          onChange={(event) => onChange(clampPadding(Number(event.target.value)))}
          disabled={disabled}
        />
        <input
          type="number"
          className="fav-input fav-number"
          aria-label={`${label} percent`}
          min={PADDING_MIN}
          max={PADDING_MAX}
          step={1}
          value={value}
          onChange={(event) => onChange(clampPadding(Number(event.target.value)))}
          disabled={disabled}
        />
      </div>
      <p id={`${id}-hint`} className="fav-hint">
        {hint}
      </p>
    </div>
  );
}
