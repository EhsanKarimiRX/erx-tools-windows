import { Fragment } from "react";
import { ChevronDown, ChevronRight, X } from "lucide-react";
import type { FileEntry, Finding } from "./types";
import { fileBaseName, formatBytes } from "./types";

interface FileListProps {
  entries: FileEntry[];
  /** Paths of the rows whose details are open. */
  expanded: string[];
  onToggle: (path: string) => void;
  onRemove: (path: string) => void;
  onClear: () => void;
  disabled: boolean;
}

/** The css class suffix of the status chip. */
function chipKind(entry: FileEntry): string {
  return entry.status === "ready" ? entry.kind : entry.status;
}

function statusText(entry: FileEntry): string {
  if (entry.status === "waiting") return "Waiting";
  if (entry.status === "working") {
    return entry.total > 0
      ? `Cleaning ${Math.floor((entry.done / entry.total) * 100)}%`
      : "Cleaning\u2026";
  }
  if (entry.status === "done") return "Cleaned";
  if (entry.status === "nothing") return "No metadata";
  if (entry.status === "failed") return "Failed";
  if (entry.status === "skipped") return "Skipped";
  if (entry.kind === "found") {
    const count = entry.findings.length;
    return `${count} item${count === 1 ? "" : "s"} found`;
  }
  if (entry.kind === "clean") return "No metadata";
  return "Not supported";
}

function FindingList({ title, items }: { title: string; items: Finding[] }) {
  return (
    <div>
      <p className="mr-details-title">{title}</p>
      <ul className="mr-list">
        {items.map((item, index) => (
          <li key={`${item.label}-${index}`}>
            <span className="mr-item-label">{item.label}</span>
            {item.detail && <span className="mr-item-detail">{item.detail}</span>}
          </li>
        ))}
      </ul>
    </div>
  );
}

function KeptList({ items }: { items: string[] }) {
  return (
    <div>
      <p className="mr-details-title">Kept</p>
      <ul className="mr-list">
        {items.map((item) => (
          <li key={item}>
            <span className="mr-item-label">{item}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

function Details({ entry }: { entry: FileEntry }) {
  const done = entry.status === "done";
  const items = done ? entry.removed : entry.findings;
  const empty = items.length === 0 && entry.kept.length === 0;
  return (
    <div className="mr-details">
      {items.length > 0 && (
        <FindingList title={done ? "Removed" : "Found (will be removed)"} items={items} />
      )}
      {entry.kept.length > 0 && <KeptList items={entry.kept} />}
      {empty && (
        <p className="mr-muted">{entry.note ?? entry.reason ?? "Nothing to show."}</p>
      )}
      {done && entry.outputPath && (
        <p className="mr-muted">
          Saved as {fileBaseName(entry.outputPath)} ({formatBytes(entry.sourceSize ?? 0)} to{" "}
          {formatBytes(entry.outputSize ?? 0)})
        </p>
      )}
      {done && (
        <p className="mr-muted">
          {entry.pixelsVerified
            ? "The picture was compared with the original: identical."
            : "The picture or video data was copied unchanged."}
        </p>
      )}
    </div>
  );
}

/** The list of files: one row each, with details that open on click. */
export default function FileList({
  entries,
  expanded,
  onToggle,
  onRemove,
  onClear,
  disabled,
}: FileListProps) {
  if (entries.length === 0) return null;

  const totalBytes = entries.reduce((sum, entry) => sum + entry.sizeBytes, 0);

  return (
    <section aria-label="Files to clean">
      <div className="mr-files-header">
        <p className="mr-files-count">
          {entries.length} file{entries.length === 1 ? "" : "s"} {"\u00b7"}{" "}
          {formatBytes(totalBytes)} total
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

      <div className="mr-files-scroll">
        <table className="mr-table">
          <thead>
            <tr>
              <th scope="col">Name</th>
              <th scope="col">Format</th>
              <th scope="col">Size</th>
              <th scope="col">Status</th>
              <th scope="col">
                <span className="visually-hidden">Remove</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {entries.map((entry) => {
              const isOpen = expanded.includes(entry.path);
              const sub =
                entry.note ??
                (entry.status === "ready" && entry.kind !== "found" ? entry.reason : null);
              return (
                <Fragment key={entry.path}>
                  <tr>
                    <td className="mr-name">
                      <div className="mr-name-row">
                        <button
                          type="button"
                          className="mr-toggle"
                          onClick={() => onToggle(entry.path)}
                          aria-expanded={isOpen}
                          aria-label={`${isOpen ? "Hide" : "Show"} details for ${entry.fileName}`}
                        >
                          {isOpen ? (
                            <ChevronDown size={14} aria-hidden="true" />
                          ) : (
                            <ChevronRight size={14} aria-hidden="true" />
                          )}
                        </button>
                        <span className="mr-name-text">{entry.fileName}</span>
                      </div>
                      {sub && <span className="mr-file-note">{sub}</span>}
                    </td>
                    <td className="mr-format">{entry.format ?? "?"}</td>
                    <td className="mr-size">{formatBytes(entry.sizeBytes)}</td>
                    <td className="mr-status">
                      <span className={`mr-chip mr-chip-${chipKind(entry)}`}>
                        {statusText(entry)}
                      </span>
                    </td>
                    <td>
                      <button
                        type="button"
                        className="mr-remove"
                        onClick={() => onRemove(entry.path)}
                        disabled={disabled}
                        aria-label={`Remove ${entry.fileName}`}
                      >
                        <X size={14} aria-hidden="true" />
                      </button>
                    </td>
                  </tr>
                  {isOpen && (
                    <tr className="mr-details-row">
                      <td colSpan={5}>
                        <Details entry={entry} />
                      </td>
                    </tr>
                  )}
                </Fragment>
              );
            })}
          </tbody>
        </table>
      </div>
    </section>
  );
}
