import { invoke } from "@tauri-apps/api/core";
import type { Options } from "./types";

interface OptionsPanelProps {
  options: Options;
  onChange: (options: Options) => void;
  disabled: boolean;
}

/** What to keep and where the new files go. Locked while cleaning. */
export default function OptionsPanel({ options, onChange, disabled }: OptionsPanelProps) {
  /** Opens the native folder picker; canceling keeps the previous location. */
  const pickFolder = async () => {
    const folder = await invoke<string | null>("mr_pick_output_folder");
    if (folder) {
      onChange({ ...options, outputFolder: folder });
    }
  };

  return (
    <div className="mr-options">
      <fieldset className="mr-fieldset" disabled={disabled}>
        <legend className="mr-legend">What to keep</legend>
        <label className="mr-choice">
          <input
            type="checkbox"
            checked={options.keepOrientation}
            onChange={(event) =>
              onChange({ ...options, keepOrientation: event.target.checked })
            }
          />
          Keep photo rotation (recommended)
        </label>
        <p className="mr-note">
          Phone photos stay upright. HEIC and TIFF photos always keep their rotation.
        </p>
        <label className="mr-choice">
          <input
            type="checkbox"
            checked={options.keepColorProfile}
            onChange={(event) =>
              onChange({ ...options, keepColorProfile: event.target.checked })
            }
          />
          Keep color profile (recommended)
        </label>
        <p className="mr-note">
          Colors stay the same. If you turn this off the profile is removed too and colors
          may look different. HEIC photos cannot lose their profile.
        </p>
      </fieldset>

      <fieldset className="mr-fieldset" disabled={disabled}>
        <legend className="mr-legend">Output location</legend>
        <label className="mr-choice">
          <input
            type="radio"
            name="mr-location"
            checked={options.outputFolder === null}
            onChange={() => onChange({ ...options, outputFolder: null })}
          />
          Same folder as each original
        </label>
        <label className="mr-choice">
          <input
            type="radio"
            name="mr-location"
            checked={options.outputFolder !== null}
            onChange={pickFolder}
          />
          One chosen folder
        </label>
        {options.outputFolder !== null && (
          <div className="mr-inline">
            <button type="button" className="button-secondary" onClick={pickFolder}>
              Browse...
            </button>
            <span className="mr-path">{options.outputFolder}</span>
          </div>
        )}
      </fieldset>

      <p className="mr-note">
        Originals are never changed or deleted. Each new file gets ERX before its extension
        (photo.jpg becomes photoERX.jpg). Delete the originals yourself if you do not need
        them.
      </p>
      <p className="mr-note">
        Only metadata fields are removed. This tool cannot remove what is visible in the
        picture or video, watermarks, the file name, the dates of the new file, traces
        inside the video itself (for example encoder settings) or other copies of the
        file. It is not a guarantee of anonymity.
      </p>
    </div>
  );
}
