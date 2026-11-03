import type { PatternKind } from "./types";
import {
  PATTERN_OPACITY_MAX,
  PATTERN_OPACITY_MIN,
  PATTERN_SIZE_MAX,
  PATTERN_SIZE_MIN,
} from "./types";

const KINDS: { value: PatternKind; label: string }[] = [
  { value: "none", label: "None" },
  { value: "vertical", label: "Vertical" },
  { value: "horizontal", label: "Horizontal" },
  { value: "checker", label: "Checker" },
];

const SIZE_HINTS: Record<number, string> = {
  1: "thin",
  2: "narrow",
  3: "medium",
  4: "wide",
  5: "thick",
};

interface PatternControlsProps {
  pattern: PatternKind;
  onPattern: (value: PatternKind) => void;
  size: number;
  onSize: (value: number) => void;
  opacity: number;
  onOpacity: (value: number) => void;
}

/** Pattern kind, size and opacity controls. Size and opacity are disabled
 * (dimmed) while the pattern is off. */
export default function PatternControls({
  pattern,
  onPattern,
  size,
  onSize,
  opacity,
  onOpacity,
}: PatternControlsProps) {
  const off = pattern === "none";
  return (
    <div className="fi-fieldset">
      <span className="fi-label" id="fi-pattern-kind-label">
        Pattern
      </span>
      <div
        className="fi-segmented"
        role="radiogroup"
        aria-labelledby="fi-pattern-kind-label"
      >
        {KINDS.map(({ value, label }) => (
          <button
            key={value}
            type="button"
            role="radio"
            aria-checked={pattern === value}
            className={pattern === value ? "fi-seg fi-seg-active" : "fi-seg"}
            onClick={() => onPattern(value)}
          >
            {label}
          </button>
        ))}
      </div>

      <div className={off ? "fi-pattern-extra fi-pattern-off" : "fi-pattern-extra"}>
        <div className="fi-color-row">
          <label className="fi-label" htmlFor="fi-pattern-size">
            Pattern size
          </label>
          <span className="fi-value">{SIZE_HINTS[size] ?? ""}</span>
        </div>
        <input
          id="fi-pattern-size"
          type="range"
          className="fi-range"
          min={PATTERN_SIZE_MIN}
          max={PATTERN_SIZE_MAX}
          step={1}
          value={size}
          onChange={(event) => onSize(Number(event.target.value))}
          disabled={off}
        />

        <div className="fi-color-row">
          <label className="fi-label" htmlFor="fi-pattern-opacity">
            Pattern opacity
          </label>
          <span className="fi-value">{opacity}%</span>
        </div>
        <input
          id="fi-pattern-opacity"
          type="range"
          className="fi-range"
          min={PATTERN_OPACITY_MIN}
          max={PATTERN_OPACITY_MAX}
          step={5}
          value={opacity}
          onChange={(event) => onOpacity(Number(event.target.value))}
          disabled={off}
        />
      </div>
    </div>
  );
}
