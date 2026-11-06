import { useCallback, useEffect, useRef, useState } from "react";
import {
  Cpu,
  HardDrive,
  ListTree,
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

interface Row {
  id: string;
  label: string;
  icon: LucideIcon;
  /** The small secondary button (only Command Prompt and PowerShell). */
  adminId?: string;
  /** Small caption under the row (which PowerShell flavor will start). */
  caption?: string;
}

const ROWS: Row[] = [
  { id: "control_panel", label: "Control Panel", icon: Settings2 },
  { id: "defrag", label: "Defragment and Optimize Drives", icon: HardDrive },
  { id: "cmd", label: "Command Prompt", icon: Terminal, adminId: "cmd_admin" },
  { id: "powershell", label: "PowerShell", icon: Shell, adminId: "powershell_admin" },
  { id: "device_manager", label: "Device Manager", icon: Cpu },
  { id: "services", label: "Services", icon: ListTree },
  { id: "pc_manager", label: "Microsoft PC Manager", icon: ShieldCheck },
];

const PC_MANAGER_EXPLANATION =
  "Microsoft PC Manager is a Microsoft app that is not included with Windows. You can get it from the Microsoft Store.";

const STORE_FAILURE =
  "Couldn't open the Microsoft Store. You can search for 'Microsoft PC Manager' there yourself. It may also not be available in your region or on managed PCs.";

/** Quick Launch: one-click buttons for common Windows tools. The app only
 * reports "Opened" when the launch call succeeded — Windows owns the rest,
 * and a dismissed UAC prompt is a neutral "Cancelled", not an error. */
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

  return (
    <SectionCard
      title="Quick Launch"
      subtitle="Open common Windows tools with one click."
    >
      <div className="sm-ql-rows">
        {ROWS.map((row) => {
          const RowIcon = row.icon;
          const label = row.id === "pc_manager" ? pcManagerLabel : row.label;
          const caption = row.id === "powershell" && info !== null
            ? info.powershellLabel
            : undefined;
          return (
            <div key={row.id} className="sm-ql-row">
              <div className="sm-ql-row-main">
                <button
                  type="button"
                  className="sm-ql-button"
                  disabled={info === null}
                  onClick={() => void open(row.id, label)}
                >
                  <RowIcon size={16} aria-hidden="true" />
                  {label}
                </button>
                {row.adminId !== undefined && (
                  <button
                    type="button"
                    className="sm-ql-admin"
                    disabled={info === null}
                    onClick={() =>
                      void open(row.adminId ?? "", `${row.label} (administrator)`)
                    }
                  >
                    Run as administrator
                  </button>
                )}
              </div>
              {caption !== undefined && (
                <span className="sm-ql-caption">Will start: {caption}</span>
              )}
              {row.id === "pc_manager" && pcManagerExplaining && (
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
              {status?.id === row.id && status.isError && (
                <p className="sm-icon-row-error" role="alert">
                  {status.text}
                </p>
              )}
              {status?.id === row.id && !status.isError && (
                <p className="sm-ql-status">{status.text}</p>
              )}
            </div>
          );
        })}
      </div>

      <InfoNote>
        Opening these tools does not need administrator rights. Some tools ask for permission
        themselves when they start, for example Defragment and Optimize Drives.
      </InfoNote>
    </SectionCard>
  );
}
