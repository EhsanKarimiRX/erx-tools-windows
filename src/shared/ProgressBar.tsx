interface ProgressBarProps {
  /** Current progress (e.g. files finished). */
  value: number;
  /** Total to reach; kept at 0..max. */
  max: number;
  /** Accessible label, e.g. "Overall conversion progress". */
  label: string;
}

/** Generic determinate progress bar shared by all tools. */
export default function ProgressBar({ value, max, label }: ProgressBarProps) {
  const safeMax = Math.max(0, max);
  const safeValue = Math.min(Math.max(0, value), safeMax);
  const percent = safeMax > 0 ? Math.round((safeValue / safeMax) * 100) : 0;

  return (
    <div
      className="progress"
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={safeMax}
      aria-valuenow={safeValue}
      aria-label={label}
    >
      <div className="progress-fill" style={{ width: `${percent}%` }} />
    </div>
  );
}
