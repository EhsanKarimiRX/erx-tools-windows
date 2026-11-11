import ColorField from "./ColorField";
import PaddingField from "./PaddingField";
import type { FaviconsOptions } from "./types";

interface OptionsPanelProps {
  options: FaviconsOptions;
  onChange: (patch: Partial<FaviconsOptions>) => void;
  disabled?: boolean;
}

export default function OptionsPanel({ options, onChange, disabled }: OptionsPanelProps) {
  return (
    <section className="fav-card" aria-labelledby="fav-options-title">
      <h2 id="fav-options-title" className="fav-heading">
        Options
      </h2>

      <div className="fav-field">
        <label className="fav-label" htmlFor="fav-site-name">
          Website name
        </label>
        <input
          id="fav-site-name"
          type="text"
          className="fav-input"
          value={options.siteName}
          maxLength={60}
          placeholder="My Website"
          autoComplete="off"
          aria-describedby="fav-site-name-hint"
          onChange={(event) => onChange({ siteName: event.target.value })}
          disabled={disabled}
        />
        <p id="fav-site-name-hint" className="fav-hint">
          Used in the web manifest and in the output folder name.
        </p>
      </div>

      <PaddingField
        id="fav-padding"
        label="Favicon padding"
        hint="Empty space around the logo in favicon.ico and the 16×16 and 32×32 PNG files (browser tabs and bookmarks). Tiny icons stay readable with little or no padding."
        value={options.paddingPercent}
        onChange={(value) => onChange({ paddingPercent: value })}
        disabled={disabled}
      />

      <PaddingField
        id="fav-app-padding"
        label="Home screen icon padding"
        hint="Empty space around the logo in the Apple touch icon (Safari: Add to Home Screen) and the Android 192×192 and 512×512 icons (Chrome shortcuts and installed web apps). iPhones round the corners of the Apple icon, so about 10% keeps the logo from touching the edge. The maskable icon always has its own safe margin."
        value={options.appPaddingPercent}
        onChange={(value) => onChange({ appPaddingPercent: value })}
        disabled={disabled}
      />

      <div className="fav-field">
        <label className="fav-check">
          <input
            type="checkbox"
            checked={options.trimTransparent}
            onChange={(event) => onChange({ trimTransparent: event.target.checked })}
            disabled={disabled}
          />
          <span>Trim empty transparent border</span>
        </label>
        <p className="fav-hint">Crops invisible space around the logo so it fills the icon.</p>
      </div>

      <ColorField
        id="fav-background"
        label="Background color"
        hint="Behind the Apple touch icon and the maskable icon, and the manifest background."
        value={options.backgroundColor}
        onChange={(value) => onChange({ backgroundColor: value })}
        disabled={disabled}
      />

      <ColorField
        id="fav-theme"
        label="Theme color"
        hint="Browser toolbar color on mobile (manifest and HTML meta tag)."
        value={options.themeColor}
        onChange={(value) => onChange({ themeColor: value })}
        disabled={disabled}
      />

      <div className="fav-field">
        <label className="fav-check">
          <input
            type="checkbox"
            checked={options.includeMaskable}
            onChange={(event) => onChange({ includeMaskable: event.target.checked })}
            disabled={disabled}
          />
          <span>Include maskable icon (Android)</span>
        </label>
        <p className="fav-hint">
          Adds a 512×512 icon with the logo on a solid background and safe margins, so Android can crop it into any
          shape.
        </p>
      </div>
    </section>
  );
}
