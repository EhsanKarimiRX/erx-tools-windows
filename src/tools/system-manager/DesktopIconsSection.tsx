import { useCallback, useEffect, useState } from "react";
import { Monitor, Network, Settings, Trash2, User } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import InfoNote from "./InfoNote";
import SectionCard from "./SectionCard";
import WarnNote from "./WarnNote";
import type { DesktopIconState } from "./types";

/** Turns anything thrown by invoke into a short display string. */
function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

/** The row icon; purely decorative, keyed by the stable backend id. */
const ROW_ICONS: Record<string, LucideIcon> = {
  this_pc: Monitor,
  recycle_bin: Trash2,
  control_panel: Settings,
  network: Network,
  user_files: User,
};

/** Desktop Icons: one switch per classic system icon. Every switch shows
 * the state READ BACK from the registry — after a failed change it reverts
 * and a red message says why. Manual changes made in Windows Settings are
 * picked up when the window regains focus. */
export default function DesktopIconsSection() {
  const [icons, setIcons] = useState<DesktopIconState[] | null>(null);
  const [pendingId, setPendingId] = useState<string | null>(null);
  const [rowError, setRowError] = useState<{ id: string; message: string } | null>(null);
  const [openingSettings, setOpeningSettings] = useState(false);
  const [settingsError, setSettingsError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const states = await invoke<DesktopIconState[]>("desktop_icons_get");
      setIcons(states);
    } catch (error) {
      setRowError({ id: "", message: errorText(error) });
    }
  }, []);

  // Read state when the section opens AND when the window regains focus,
  // so changes made in Windows Settings are mirrored here.
  useEffect(() => {
    void refresh();
    const onFocus = () => void refresh();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refresh]);

  const toggle = useCallback(
    async (icon: DesktopIconState) => {
      if (pendingId !== null) return;
      setPendingId(icon.id);
      setRowError(null);
      try {
        // The response carries the READ-BACK state — that is what the
        // switch shows, never our intention.
        const updated = await invoke<DesktopIconState>("desktop_icons_set", {
          id: icon.id,
          visible: !icon.visible,
        });
        setIcons(
          (current) =>
            current?.map((entry) => (entry.id === updated.id ? updated : entry)) ?? current,
        );
      } catch (error) {
        // Revert to the previous position and say why, under the row.
        setRowError({ id: icon.id, message: errorText(error) });
        void refresh();
      } finally {
        setPendingId(null);
      }
    },
    [pendingId, refresh],
  );

  // Opens Windows' own "Desktop Icon Settings" window. Separate from the
  // switches' pendingId state on purpose: a pending launch must never block
  // or revert the switches.
  const openWindowsSettings = useCallback(async () => {
    if (openingSettings) return;
    setOpeningSettings(true);
    setSettingsError(null);
    try {
      await invoke("desktop_icons_open_windows_settings");
    } catch (error) {
      setSettingsError(errorText(error));
    } finally {
      setOpeningSettings(false);
    }
  }, [openingSettings]);

  return (
    <SectionCard
      title="Desktop Icons"
      subtitle="Show or hide the standard desktop icons."
    >
      <InfoNote>
        Changes apply to your Windows account only. If the desktop does not update by itself,
        click an empty spot on the desktop and press F5.
      </InfoNote>

      {icons === null && <p className="tool-placeholder">Checking the desktop icons…</p>}

      {icons !== null && (
        <>
          <button
            type="button"
            className="sm-ql-button sm-icon-settings-button"
            onClick={() => void openWindowsSettings()}
            disabled={openingSettings}
          >
            Open Windows desktop icon settings
          </button>
          {settingsError !== null && (
            <p className="sm-icon-row-error" role="alert">
              Couldn't open the Windows settings window: {settingsError}
            </p>
          )}
          <WarnNote>
            When you hide an icon, Windows may move your other desktop icons into the free spot.
            Windows does the same in its own settings window.
          </WarnNote>
        </>
      )}

      {icons !== null && (
        <div className="sm-icon-rows">
          {icons.map((icon) => {
            const RowIcon = ROW_ICONS[icon.id] ?? Monitor;
            return (
              <div key={icon.id} className="sm-icon-row">
                <RowIcon size={18} aria-hidden="true" />
                <span className="sm-icon-row-name">{icon.name}</span>
                <button
                  type="button"
                  role="switch"
                  aria-checked={icon.visible}
                  aria-label={`${icon.visible ? "Hide" : "Show"} ${icon.name}`}
                  className={`sm-switch ${icon.visible ? "sm-switch-on" : ""}`}
                  onClick={() => void toggle(icon)}
                  disabled={pendingId !== null}
                >
                  <span className="sm-switch-knob" />
                </button>
              </div>
            );
          })}
        </div>
      )}

      {rowError !== null && (
        <p className="sm-icon-row-error" role="alert">
          Couldn't change this icon: {rowError.message}
        </p>
      )}

      <InfoNote>
        ERX Tools never moves or saves your icon positions. If your desktop layout changes, it is
        caused by Windows itself, not by ERX Tools.
      </InfoNote>

      <InfoNote>
        This changes the same settings as Windows Settings → Personalization → Themes → Desktop
        icon settings.
      </InfoNote>
    </SectionCard>
  );
}
