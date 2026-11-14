import ProgressBar from "../../shared/ProgressBar";
import type { ProgressView } from "./progress";
import { formatBytes, formatDuration } from "./types";

interface OperationStatusProps {
  view: ProgressView;
  /** Title while data is being processed, e.g. "Encrypting...". */
  workingLabel: string;
  cancelling: boolean;
  onCancel: () => void;
}

function stageTitle(view: ProgressView, workingLabel: string): string {
  switch (view.stage) {
    case "tuning":
      return "Preparing the security settings for this computer (first time only)...";
    case "deriving":
      return "Deriving the key from your passphrase...";
    case "verifying":
      return "Checking the new file...";
    default:
      return workingLabel;
  }
}

/** Progress of a running operation: indeterminate while the key is being
 * derived, then a percentage with speed and time left, plus Cancel. */
export default function OperationStatus({
  view,
  workingLabel,
  cancelling,
  onCancel,
}: OperationStatusProps) {
  const indeterminate = view.stage === "tuning" || view.stage === "deriving";
  const percent =
    view.total > 0 ? Math.min(100, Math.floor((view.done / view.total) * 100)) : 0;
  const title = stageTitle(view, workingLabel);
  const details = [
    `${percent}%`,
    view.bytesPerSecond !== null ? `${formatBytes(view.bytesPerSecond)}/s` : "",
    view.etaSeconds !== null ? `about ${formatDuration(view.etaSeconds)} left` : "",
  ]
    .filter((part) => part !== "")
    .join(" - ");

  return (
    <div className="vt-status" aria-live="polite">
      <p className="vt-status-title">{title}</p>
      <div className="vt-status-row">
        {indeterminate ? (
          <div className="vt-indeterminate" role="progressbar" aria-label={title}>
            <div className="vt-indeterminate-fill" />
          </div>
        ) : (
          <ProgressBar value={view.done} max={view.total} label={title} />
        )}
        <button
          type="button"
          className="button-secondary"
          onClick={onCancel}
          disabled={cancelling}
        >
          {cancelling ? "Cancelling..." : "Cancel"}
        </button>
      </div>
      {!indeterminate && <p className="vt-note">{details}</p>}
      {indeterminate && cancelling && (
        <p className="vt-note">
          Cancelling takes effect when the key step is finished (about a second).
        </p>
      )}
    </div>
  );
}
