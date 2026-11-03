import type { FolderEntry } from "./types";

interface FolderListProps {
  folders: FolderEntry[];
  onRemove: (index: number) => void;
  disabled: boolean;
}

/** The folder rows: name, truncated path (full path in the tooltip),
 * status chip and a remove button. Blocked rows stay visible with their
 * reason so it is clear why they are skipped. */
export default function FolderList({
  folders,
  onRemove,
  disabled,
}: FolderListProps) {
  return (
    <ul className="fi-list" aria-label="Selected folders">
      {folders.map((entry, index) => {
        const name = entry.path.replace(/[\\/]+$/, "").split(/[\\/]/).pop() ?? entry.path;
        return (
          <li key={entry.path} className="fi-row">
            <span className={`fi-chip fi-chip-${entry.status}`}>
              {chipLabel(entry.status)}
            </span>
            <span className="fi-row-text">
              <span className="fi-row-name">{name}</span>
              <span className="fi-row-path" title={entry.path}>
                {entry.path}
              </span>
              {entry.detail !== null && (
                <span
                  className={
                    entry.status === "blocked" || entry.status === "failed"
                      ? "fi-row-detail fi-row-detail-error"
                      : "fi-row-detail"
                  }
                >
                  {entry.detail}
                </span>
              )}
            </span>
            <button
              type="button"
              className="fi-remove"
              onClick={() => onRemove(index)}
              disabled={disabled}
              aria-label={`Remove ${name} from the list`}
              title={`Remove ${name}`}
            >
              ×
            </button>
          </li>
        );
      })}
    </ul>
  );
}

function chipLabel(status: FolderEntry["status"]): string {
  switch (status) {
    case "checking":
      return "…";
    case "ready":
      return "Ready";
    case "warning":
      return "Warning";
    case "blocked":
      return "Blocked";
    case "applying":
      return "Applying";
    case "applied":
      return "Applied";
    case "skipped":
      return "Skipped";
    case "failed":
      return "Failed";
  }
}
