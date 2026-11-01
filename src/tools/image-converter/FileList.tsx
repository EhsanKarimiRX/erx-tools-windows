import { X } from "lucide-react";
import type { FileEntry, FileStatus } from "./types";
import { formatBytes } from "./types";

interface FileListProps {
  entries: FileEntry[];
  onRemove: (path: string) => void;
  onClear: () => void;
  disabled: boolean;
}

const STATUS_LABEL: Record<FileStatus, string> = {
  ready: "Ready",
  waiting: "Waiting",
  converting: "Converting…",
  done: "Done",
  failed: "Failed",
  skipped: "Skipped",
};

/**
 * The queue of files to convert, always shown in list order (which is also
 * the conversion start order).
 */
export default function FileList({ entries, onRemove, onClear, disabled }: FileListProps) {
  if (entries.length === 0) return null;

  const totalBytes = entries.reduce((sum, entry) => sum + entry.sizeBytes, 0);
  const readyCount = entries.filter((entry) => entry.status === "ready").length;

  return (
    <section aria-label="Files to convert">
      <div className="ic-files-header">
        <p className="ic-files-count">
          {entries.length} file{entries.length === 1 ? "" : "s"} ·{" "}
          {formatBytes(totalBytes)} total
          {readyCount !== entries.length && ` · ${readyCount} ready`}
        </p>
        <button
          type="button"
          className="button-secondary"
          onClick={onClear}
          disabled={disabled}
        >
          Clear list
        </button>
      </div>

      <div className="ic-files-scroll">
        <table className="ic-table">
          <thead>
            <tr>
              <th scope="col">Name</th>
              <th scope="col">Size</th>
              <th scope="col">Format</th>
              <th scope="col">Status</th>
              <th scope="col"><span className="visually-hidden">Remove</span></th>
            </tr>
          </thead>
          <tbody>
            {entries.map((entry) => (
              <tr key={entry.path}>
                <td className="ic-name">
                  {entry.fileName}
                  {entry.note && (
                    <span className="ic-file-note">{entry.note}</span>
                  )}
                </td>
                <td className="ic-size">{formatBytes(entry.sizeBytes)}</td>
                <td className="ic-format">{entry.format}</td>
                <td className="ic-status">
                  <span className={`ic-status-chip ic-status-${entry.status}`}>
                    {STATUS_LABEL[entry.status]}
                  </span>
                </td>
                <td>
                  <button
                    type="button"
                    className="ic-remove"
                    onClick={() => onRemove(entry.path)}
                    disabled={disabled}
                    aria-label={`Remove ${entry.fileName}`}
                  >
                    <X size={14} aria-hidden="true" />
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}
