import type { SaveResult } from "./types";
import { formatBytes } from "./types";

interface SavedPanelProps {
  result: SaveResult;
  onReveal: () => void;
}

/** Shown after a successful save: folder, file list, "Show in folder". */
export default function SavedPanel({ result, onReveal }: SavedPanelProps) {
  return (
    <section className="fav-card" aria-labelledby="fav-saved-title">
      <h2 id="fav-saved-title" className="fav-heading">
        Saved {result.files.length} files
      </h2>
      <p className="fav-folder">{result.folder}</p>
      <ul className="fav-saved-list">
        {result.files.map((file) => (
          <li key={file.name}>
            <span>{file.name}</span>
            <span className="fav-icon-meta">{formatBytes(file.sizeBytes)}</span>
          </li>
        ))}
      </ul>
      <button type="button" className="button-primary" onClick={onReveal}>
        Show in folder
      </button>
    </section>
  );
}
