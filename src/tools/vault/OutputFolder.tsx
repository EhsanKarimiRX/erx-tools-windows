interface OutputFolderProps {
  /** The folder the result will be written to. */
  folder: string;
  /** True when the user picked a folder (false: the file's own folder). */
  custom: boolean;
  disabled: boolean;
  onPick: () => void;
  onReset: () => void;
}

/** Shows the output folder with a button to choose another one. */
export default function OutputFolder({
  folder,
  custom,
  disabled,
  onPick,
  onReset,
}: OutputFolderProps) {
  return (
    <div className="vt-field">
      <span className="vt-label">Save to</span>
      <p className="vt-path">{folder}</p>
      <div className="vt-actions">
        <button
          type="button"
          className="button-secondary"
          onClick={onPick}
          disabled={disabled}
        >
          Choose another folder
        </button>
        {custom && (
          <button
            type="button"
            className="button-secondary"
            onClick={onReset}
            disabled={disabled}
          >
            Use the file&apos;s own folder
          </button>
        )}
      </div>
    </div>
  );
}
