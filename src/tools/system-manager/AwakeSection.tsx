import { useCallback, useEffect, useRef, useState } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import Banner from "./Banner";
import type { BannerVariant } from "./Banner";
import InfoNote from "./InfoNote";
import SectionCard from "./SectionCard";
import WarnNote from "./WarnNote";
import type { AwakeStatus, DisableResponse, PendingAutoOff, StatusResponse } from "./types";
import { TIMER_CHOICES } from "./types";

/** Turns anything thrown by invoke into a short display string. */
function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

/** One banner in the area above the toggle. Auto-off and error banners
 * stay until dismissed (the spec's "persistent until dismissed"). */
interface BannerEntry {
  id: number;
  variant: BannerVariant;
  message: string;
}

/** Local drawer state. While Awake is OFF these are pending choices sent
 * with enable; while ON every change calls awake_set_options right away. */
interface DrawerOptions {
  keepScreenOn: boolean;
  /** 0 = never (stay on until turned off). */
  timerHours: number;
}

function formatRemaining(endsAtUnix: number, nowUnix: number): string {
  const totalMinutes = Math.max(0, Math.round((endsAtUnix - nowUnix) / 60));
  const hours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;
  if (hours === 0) return `Turns off in ${minutes} min`;
  return `Turns off in ${hours} h ${minutes} min`;
}

/** The Awake section: one big toggle first, everything else quiet. The
 * toggle only ever shows what the backend reported — it never lies. */
export default function AwakeSection() {
  const [status, setStatus] = useState<AwakeStatus | null>(null);
  const [banners, setBanners] = useState<BannerEntry[]>([]);
  const [options, setOptions] = useState<DrawerOptions>({ keepScreenOn: true, timerHours: 0 });
  const [optionsOpen, setOptionsOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [closeDialogOpen, setCloseDialogOpen] = useState(false);
  const [nowUnix, setNowUnix] = useState(() => Math.floor(Date.now() / 1000));

  const nextBannerId = useRef(1);
  const resumedShownRef = useRef(false);

  const addBanner = useCallback((variant: BannerVariant, message: string) => {
    setBanners((current) => {
      // Same message twice (event + status poll) shows once.
      if (current.some((entry) => entry.message === message)) return current;
      return [...current, { id: nextBannerId.current++, variant, message }];
    });
  }, []);

  const refresh = useCallback(async () => {
    try {
      const response = await invoke<StatusResponse>("awake_get_status");
      setStatus(response.status);
      if (response.pendingAutoOff) {
        addBanner("warning", response.pendingAutoOff.message);
      }
      if (response.status.state === "on" && response.status.resumedFromPreviousSession) {
        if (!resumedShownRef.current) {
          resumedShownRef.current = true;
          addBanner("info", "Awake was left on from an earlier session.");
        }
      } else {
        resumedShownRef.current = false;
      }
    } catch (error) {
      addBanner("error", errorText(error));
    }
  }, [addBanner]);

  // First status, and a fresh one whenever the window regains focus (R4).
  useEffect(() => {
    void refresh();
    const onFocus = () => void refresh();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refresh]);

  // Backend events (R4/R7): status changes, auto-off, close request.
  useEffect(() => {
    const unlistens: Array<() => void> = [];
    let cancelled = false;
    const track = (promise: Promise<() => void>) => {
      void promise.then((unlisten) => {
        if (cancelled) unlisten();
        else unlistens.push(unlisten);
      });
    };
    track(listen<AwakeStatus>("awake://changed", (event) => setStatus(event.payload)));
    track(
      listen<PendingAutoOff>("awake://auto-off", (event) => {
        addBanner("warning", event.payload.message);
      }),
    );
    track(listen<null>("awake://close-requested", () => setCloseDialogOpen(true)));
    return () => {
      cancelled = true;
      unlistens.forEach((unlisten) => unlisten());
    };
  }, [addBanner]);

  // Ticking clock for the countdown (minute precision is enough).
  const timerEndsAt = status?.state === "on" ? status.endsAtUnix : null;
  useEffect(() => {
    if (timerEndsAt === null) return;
    const interval = window.setInterval(() => setNowUnix(Math.floor(Date.now() / 1000)), 15_000);
    return () => window.clearInterval(interval);
  }, [timerEndsAt]);

  const toggle = useCallback(async () => {
    if (status === null || busy) return;
    setBusy(true);
    try {
      if (status.state === "on") {
        const response = await invoke<DisableResponse>("awake_disable");
        setStatus(response.status);
        const report = response.report;
        if (report.failed.length > 0) {
          addBanner("error", report.failed.join(" "));
        } else if (report.schemeGone) {
          addBanner(
            "warning",
            "The Windows power plan Awake was using no longer exists. Nothing needed restoring.",
          );
        } else if (report.restored > 0 || report.kept > 0) {
          const kept =
            report.kept > 0
              ? ` ${report.kept} change${report.kept === 1 ? "" : "s"} you made yourself ${report.kept === 1 ? "was" : "were"} kept.`
              : "";
          addBanner(
            "info",
            `Awake is off. ${report.restored} setting${report.restored === 1 ? "" : "s"} restored.${kept}`,
          );
        }
      } else {
        const endsAtUnix =
          options.timerHours > 0
            ? Math.floor(Date.now() / 1000) + options.timerHours * 3600
            : null;
        const newStatus = await invoke<AwakeStatus>("awake_enable", {
          options: { keepScreenOn: options.keepScreenOn, endsAtUnix },
        });
        setStatus(newStatus);
      }
    } catch (error) {
      addBanner("error", errorText(error));
    } finally {
      setBusy(false);
    }
  }, [status, busy, options, addBanner]);

  /** Drawer changes apply instantly while ON (R5), are pending while OFF. */
  const applyOptions = useCallback(
    async (next: DrawerOptions) => {
      setOptions(next);
      if (status?.state !== "on") return;
      try {
        const endsAtUnix =
          next.timerHours > 0 ? Math.floor(Date.now() / 1000) + next.timerHours * 3600 : null;
        const newStatus = await invoke<AwakeStatus>("awake_set_options", {
          options: { keepScreenOn: next.keepScreenOn, endsAtUnix },
        });
        setStatus(newStatus);
      } catch (error) {
        addBanner("error", errorText(error));
      }
    },
    [status, addBanner],
  );

  const chooseOnExit = useCallback(async (choice: "restore_and_close" | "keep_and_close" | "cancel") => {
    setCloseDialogOpen(false);
    if (choice === "cancel") return;
    try {
      await invoke("awake_exit", { choice });
      // The app exits; nothing to render afterwards.
    } catch {
      // The window may already be gone; nothing sensible to show.
    }
  }, []);

  const isOn = status?.state === "on";

  return (
    <SectionCard
      title="Awake"
      subtitle="Keeps this PC from sleeping and the screen from turning off — for long runs."
    >
      {banners.length > 0 && (
        <div className="sm-banners">
          {banners.map((banner) => (
            <Banner
              key={banner.id}
              variant={banner.variant}
              message={banner.message}
              onDismiss={() =>
                setBanners((current) => current.filter((entry) => entry.id !== banner.id))
              }
            />
          ))}
        </div>
      )}

      <div className="sm-awake-toggle-row">
        <button
          type="button"
          role="switch"
          aria-checked={isOn}
          className={`sm-switch ${isOn ? "sm-switch-on" : ""}`}
          onClick={() => void toggle()}
          disabled={status === null || busy}
        >
          <span className="sm-switch-knob" />
        </button>
        <span className="sm-awake-toggle-label">Keep PC awake</span>
      </div>

      <p className="sm-awake-status">
        {status === null
          ? "Checking Awake…"
          : isOn
            ? options.keepScreenOn || status.keepScreenOn
              ? "Awake is ON — sleep and screen-off are set to Never (plugged in and on battery)."
              : "Awake is ON — sleep is set to Never (plugged in and on battery). The screen can still turn off."
            : "Awake is off."}
      </p>
      {isOn && timerEndsAt !== null && (
        <p className="sm-awake-status">{formatRemaining(timerEndsAt, nowUnix)}</p>
      )}

      {isOn && (
        <WarnNote>
          You can close ERX Tools and Awake will stay on. Open ERX Tools later to turn Awake
          off and restore your original settings.
        </WarnNote>
      )}

      <div className="sm-awake-options">
        <button
          type="button"
          className="sm-awake-options-toggle"
          onClick={() => setOptionsOpen((open) => !open)}
          aria-expanded={optionsOpen}
        >
          {optionsOpen ? <ChevronDown size={14} aria-hidden="true" /> : <ChevronRight size={14} aria-hidden="true" />}
          Options
        </button>
        {optionsOpen && (
          <div className="sm-awake-options-body">
            <label className="sm-awake-check">
              <input
                type="checkbox"
                checked={!options.keepScreenOn}
                onChange={(event) =>
                  void applyOptions({ ...options, keepScreenOn: !event.target.checked })
                }
              />
              <span>Let the screen turn off normally</span>
            </label>
            <p className="sm-note-info">
              By default Awake also keeps the screen on, because some runs (for example image
              processing) need it. Tick this if you only want to stop the PC from sleeping.
            </p>
            <label className="sm-awake-select">
              <span>Turn Awake off automatically</span>
              <select
                value={options.timerHours}
                onChange={(event) =>
                  void applyOptions({ ...options, timerHours: Number(event.target.value) })
                }
              >
                {TIMER_CHOICES.map((choice) => (
                  <option key={choice.hours} value={choice.hours}>
                    {choice.label}
                  </option>
                ))}
              </select>
            </label>
            <p className="sm-note-info">The timer only runs while ERX Tools is open.</p>
          </div>
        )}
      </div>

      <InfoNote>
        How it works: Awake temporarily sets the Windows sleep and screen timeouts to Never and
        puts your original values back when you turn it off.
      </InfoNote>
      <InfoNote>
        Awake only blocks automatic sleep and screen-off. Clicking Sleep or shutting down still
        works, and closing a laptop lid follows your Windows lid setting.
      </InfoNote>

      {closeDialogOpen && (
        <div className="sm-dialog-overlay">
          <div className="sm-dialog" role="dialog" aria-modal="true" aria-labelledby="sm-awake-dialog-title">
            <h3 id="sm-awake-dialog-title" className="sm-dialog-title">
              Awake is still on
            </h3>
            <p className="sm-dialog-text">
              Awake is keeping this PC awake. What should happen?
            </p>
            {options.timerHours > 0 && (
              <p className="sm-dialog-text">
                Note: the timer only runs while ERX Tools is open.
              </p>
            )}
            <div className="sm-dialog-buttons">
              <button
                type="button"
                className="button-primary"
                onClick={() => void chooseOnExit("restore_and_close")}
              >
                Restore settings and close
              </button>
              <button
                type="button"
                className="sm-dialog-button"
                onClick={() => void chooseOnExit("keep_and_close")}
              >
                Keep Awake on and close
              </button>
              <button
                type="button"
                className="sm-dialog-button"
                onClick={() => void chooseOnExit("cancel")}
              >
                Cancel
              </button>
            </div>
          </div>
        </div>
      )}
    </SectionCard>
  );
}
