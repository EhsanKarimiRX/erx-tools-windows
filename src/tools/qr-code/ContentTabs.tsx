import { useState } from "react";
import { Eye, EyeOff } from "lucide-react";
import type { ContentTab, WifiSecurity } from "./types";
import { previewUrl } from "./types";

interface ContentTabsProps {
  tab: ContentTab;
  onTab: (tab: ContentTab) => void;
  text: string;
  onText: (value: string) => void;
  url: string;
  onUrl: (value: string) => void;
  ssid: string;
  onSsid: (value: string) => void;
  password: string;
  onPassword: (value: string) => void;
  security: WifiSecurity;
  onSecurity: (value: WifiSecurity) => void;
  hidden: boolean;
  onHidden: (value: boolean) => void;
}

const TABS: { id: ContentTab; label: string }[] = [
  { id: "text", label: "Text" },
  { id: "website", label: "Website" },
  { id: "wifi", label: "Wi-Fi" },
];

/** The three content tabs and their fields. Switching away from Wi-Fi is
 * handled by the parent, which clears the password (it must never linger
 * outside its tab). */
export default function ContentTabs(props: ContentTabsProps) {
  const [showPassword, setShowPassword] = useState(false);

  return (
    <div className="qr-fieldset qr-tabs-block">
      <div className="qr-tabs" role="tablist" aria-label="Content type">
        {TABS.map(({ id, label }) => (
          <button
            key={id}
            type="button"
            role="tab"
            aria-selected={props.tab === id}
            className={props.tab === id ? "qr-tab qr-tab-active" : "qr-tab"}
            onClick={() => props.onTab(id)}
          >
            {label}
          </button>
        ))}
      </div>

      {props.tab === "text" && (
        <>
          <label className="qr-label" htmlFor="qr-text">
            Text
          </label>
          <textarea
            id="qr-text"
            className="qr-textarea"
            dir="auto"
            rows={5}
            placeholder="Any text — links, notes, Persian, anything"
            value={props.text}
            onChange={(event) => props.onText(event.target.value)}
            spellCheck={false}
          />
        </>
      )}

      {props.tab === "website" && (
        <>
          <label className="qr-label" htmlFor="qr-url">
            Website address
          </label>
          <input
            id="qr-url"
            type="text"
            className="qr-input"
            dir="ltr"
            placeholder="example.com/page"
            value={props.url}
            onChange={(event) => props.onUrl(event.target.value)}
            spellCheck={false}
          />
          {previewUrl(props.url) && (
            <p className="qr-hint" dir="ltr">
              Will encode: {previewUrl(props.url)}
            </p>
          )}
          <p className="qr-hint">
            Only http and https links belong here. Other link types can go in
            the Text tab.
          </p>
        </>
      )}

      {props.tab === "wifi" && (
        <>
          <label className="qr-label" htmlFor="qr-ssid">
            Network name (SSID)
          </label>
          <input
            id="qr-ssid"
            type="text"
            className="qr-input"
            dir="auto"
            value={props.ssid}
            onChange={(event) => props.onSsid(event.target.value)}
            spellCheck={false}
          />

          <label className="qr-label" htmlFor="qr-password">
            Password
          </label>
          <div className="qr-password-row">
            <input
              id="qr-password"
              type={showPassword ? "text" : "password"}
              className="qr-input"
              dir="ltr"
              autoComplete="off"
              value={props.password}
              onChange={(event) => props.onPassword(event.target.value)}
              disabled={props.security === "nopass"}
              spellCheck={false}
            />
            <button
              type="button"
              className="button-secondary qr-show-button"
              onClick={() => setShowPassword(!showPassword)}
              aria-pressed={showPassword}
              aria-label={showPassword ? "Hide password" : "Show password"}
              disabled={props.security === "nopass"}
            >
              {showPassword ? (
                <EyeOff size={15} aria-hidden="true" />
              ) : (
                <Eye size={15} aria-hidden="true" />
              )}
            </button>
          </div>

          <label className="qr-label" htmlFor="qr-security">
            Security
          </label>
          <select
            id="qr-security"
            className="qr-input"
            value={props.security}
            onChange={(event) =>
              props.onSecurity(event.target.value as WifiSecurity)
            }
          >
            <option value="wpa">WPA / WPA2 / WPA3</option>
            <option value="wep">WEP</option>
            <option value="nopass">No password</option>
          </select>

          <label className="qr-checkbox">
            <input
              type="checkbox"
              checked={props.hidden}
              onChange={(event) => props.onHidden(event.target.checked)}
            />
            Hidden network
          </label>

          <p className="qr-hint qr-hint-warning">
            Anyone who scans this QR code can read the Wi-Fi password in
            plain form. Keep the printed code safe.
          </p>
        </>
      )}
    </div>
  );
}
