import { useCallback, useEffect, useRef, useState } from "react";
import {
  Cpu,
  HardDrive,
  ListTree,
  MonitorCog,
  Settings2,
  Shell,
  ShieldCheck,
  Terminal,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import InfoNote from "./InfoNote";
import SectionCard from "./SectionCard";
import type { QuickLaunchInfo } from "./types";

/** Turns anything thrown by invoke into a short display string. */
function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

interface Tile {
  id: string;
  label: string;
  icon: LucideIcon;
  /** true = the "Run as administrator" variant (purple gradient, UAC). */
  admin?: boolean;
}

/** Top group: one button per Windows tool, in this fixed order. */
const TOOL_TILES: Tile[] = [
  { id: "control_panel", label: "Control Panel", icon: Settings2 },
  { id: "defrag", label: "Defragment and Optimize Drives", icon: HardDrive },
  { id: "device_manager", label: "Device Manager", icon: Cpu },
  { id: "services", label: "Services", icon: ListTree },
  { id: "desktop_icon_settings", label: "Desktop Icon Settings", icon: MonitorCog },
  { id: "pc_manager", label: "Microsoft PC Manager", icon: ShieldCheck },
];

/** Below the divider: the two shells, normal launch first, then the
 * explicit administrator variant. Every id is a fixed entry in the Rust
 * allowlist (quick_launch/core.rs); the frontend never sends a path. */
const SHELL_TILES: Tile[] = [
  { id: "powershell", label: "PowerShell", icon: Shell },
  { id: "powershell_admin", label: "PowerShell", icon: Shell, admin: true },
  { id: "cmd", label: "Command Prompt", icon: Terminal },
  { id: "cmd_admin", label: "Command Prompt", icon: Terminal, admin: true },
];

const ADMIN_SUFFIX = "Run as administrator";

/** The ids that live below the divider (their status shows there). */
const SHELL_IDS = new Set(SHELL_TILES.map((tile) => tile.id));

const PC_MANAGER_EXPLANATION =
  "Microsoft PC Manager is a Microsoft app that is not included with Windows. You can get it from the Microsoft Store.";

const STORE_FAILURE =
  "Couldn't open the Microsoft Store. You can search for 'Microsoft PC Manager' there yourself. It may also not be available in your region or on managed PCs.";

interface TileButtonProps {
  tile: Tile;
  label: string;
  disabled: boolean;
  onOpen: () => void;
}

/** One square button. Long labels wrap; the administrator variant shows
 * the fixed suffix under the name and carries the purple gradient. */
function TileButton({ tile, label, disabled, onOpen }: TileButtonProps) {
  const Icon = tile.icon;
  return (
    <button
      type="button"
      className={`sm-ql-tile${tile.admin === true ? " sm-ql-tile-admin" : ""}`}
      disabled={disabled}
      onClick={onOpen}
    >
      <Icon size={20} aria-hidden="true" />
      <span className="sm-ql-tile-label">
        {label}
        {tile.admin === true && <span className="sm-ql-tile-note">{ADMIN_SUFFIX}</span>}
      </span>
    </button>
  );
}

/** Quick Launch: ten square buttons in a four-column grid. The top group
 * opens a Windows tool normally; the four shell buttons below the divider
 * offer PowerShell and Command Prompt, each also as an explicit
 * administrator launch. The app only reports "Opened" when the launch call
 * succeeded — Windows owns the rest, and a dismissed UAC prompt is a
 * neutral "Cancelled", not an error. */
export default function QuickLaunchSection() {
  const [info, setInfo] = useState<QuickLaunchInfo | null>(null);
  const [status, setStatus] = useState<{ id: string; text: string; isError: boolean } | null>(
    null,
  );
  const [pcManagerExplaining, setPcManagerExplaining] = useState(false);
  const [storeFailed, setStoreFailed] = useState(false);
  const statusTimer = useRef<number | null>(null);

  useEffect(() => {
    invoke<QuickLaunchInfo>("quick_launch_info")
      .then(setInfo)
      .catch((error) => setStatus({ id: "", text: errorText(error), isError: true }));
    return () => {
      if (statusTimer.current !== null) window.clearTimeout(statusTimer.current);
    };
  }, []);

  const flashStatus = useCallback((id: string, text: string, isError: boolean) => {
    setStatus({ id, text, isError });
    if (statusTimer.current !== null) window.clearTimeout(statusTimer.current);
    statusTimer.current = window.setTimeout(() => setStatus(null), 4_000);
  }, []);

  const open = useCallback(
    async (id: string, label: string) => {
      setPcManagerExplaining(false);
      setStoreFailed(false);
      // The not-installed PC Manager button shows an honest explanation
      // with a Store button instead of an error.
      if (id === "pc_manager" && info !== null && !info.pcManager.installed) {
        setPcManagerExplaining(true);
        return;
      }
      try {
        const result = await invoke<{ outcome: "opened" | "cancelled" }>("quick_launch_open", {
          id,
        });
        if (result.outcome === "cancelled") {
          flashStatus(id, "Cancelled.", false);
        } else {
          flashStatus(id, `${label} opened.`, false);
        }
      } catch (error) {
        if (id === "pc_manager_store") {
          setStoreFailed(true);
        } else {
          setStatus({ id, text: errorText(error), isError: true });
        }
      }
    },
    [info, flashStatus],
  );

  const pcManagerInstalled = info?.pcManager.installed ?? false;
  const pcManagerLabel = pcManagerInstalled
    ? "Microsoft PC Manager"
    : info === null
      ? "Microsoft PC Manager"
      : "Microsoft PC Manager (not installed)";

  const labelOf = (tile: Tile) => (tile.id === "pc_manager" ? pcManagerLabel : tile.label);

  /** Status line for one of the two groups (the message sits directly
   * under the button it belongs to, as before). */
  const statusLine = (belowDivider: boolean) => {
    if (status === null || SHELL_IDS.has(status.id) !== belowDivider) return null;
    return status.isError ? (
      <p className="sm-icon-row-error" role="alert">
        {status.text}
      </p>
    ) : (
      <p className="sm-ql-status">{status.text}</p>
    );
  };

  return (
    <SectionCard
      title="Quick Launch"
      subtitle="Open common Windows tools with one click."
    >
      <div className="sm-ql-grid">
        {TOOL_TILES.map((tile) => (
          <TileButton
            key={tile.id}
            tile={tile}
            label={labelOf(tile)}
            disabled={info === null}
            onOpen={() => void open(tile.id, labelOf(tile))}
          />
        ))}
      </div>

      {pcManagerExplaining && (
        <div className="sm-ql-pcmanager">
          <p className="sm-note-info">{PC_MANAGER_EXPLANATION}</p>
          <button
            type="button"
            className="sm-dialog-button"
            onClick={() => void open("pc_manager_store", "Microsoft Store")}
          >
            Open Microsoft Store page
          </button>
          {storeFailed && <p className="sm-note-info">{STORE_FAILURE}</p>}
        </div>
      )}

      {statusLine(false)}

      <hr className="sm-ql-divider" />

      <div className="sm-ql-grid sm-ql-grid-shell">
        {SHELL_TILES.map((tile) => (
          <TileButton
            key={tile.id}
            tile={tile}
            label={tile.label}
            disabled={info === null}
            onOpen={() =>
              void open(
                tile.id,
                tile.admin === true ? `${tile.label} (administrator)` : tile.label,
              )
            }
          />
        ))}
      </div>

      {info !== null && (
        <p className="sm-ql-caption">Will start: {info.powershellLabel}</p>
      )}

      {statusLine(true)}

      <InfoNote>
        The buttons above the line open a tool normally and need no administrator rights. The
        purple {ADMIN_SUFFIX} buttons ask Windows for permission (UAC) first. Some tools ask for
        permission themselves when they start, for example Defragment and Optimize Drives.
      </InfoNote>
    </SectionCard>
  );
}
