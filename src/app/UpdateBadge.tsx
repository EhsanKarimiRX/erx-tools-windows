import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ArrowUp, Check, TriangleAlert } from "lucide-react";

/** Mirror of the Rust `UpdateCheckResult` enum (camelCase `kind` tag). */
type UpdateCheckResult =
  | { kind: "upToDate"; current: string; latest: string }
  | { kind: "updateAvailable"; current: string; latest: string }
  | { kind: "noRelease"; current: string }
  | { kind: "failed"; message: string };

/** The badge's visual states; `idle` is the state at every app start. */
type BadgeState = "idle" | "checking" | "available" | "uptodate" | "failed" | "norelease";

/** The circle's tooltip and aria-label per state: color is never the only signal. */
const CIRCLE_LABELS: Record<BadgeState, string> = {
  idle: "Check for updates",
  checking: "Checking for updates…",
  available: "Update available",
  uptodate: "You have the latest version",
  failed: "Update check failed",
  norelease: "Check for updates",
};

/**
 * The version pill at the bottom-left of the home screen: the version text
 * plus a small circular button. The check runs ONLY when the button is
 * clicked — never automatically. The result popover opens above the pill
 * (the pill sits at the bottom edge). State lives in React only.
 */
export default function UpdateBadge({ version }: { version: string }) {
  const [state, setState] = useState<BadgeState>("idle");
  const [result, setResult] = useState<UpdateCheckResult | null>(null);
  const [popoverOpen, setPopoverOpen] = useState(false);
  const [openError, setOpenError] = useState<string | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  const circleRef = useRef<HTMLButtonElement>(null);

  const runCheck = useCallback(async () => {
    setState("checking");
    setPopoverOpen(false);
    setOpenError(null);
    try {
      const check = await invoke<UpdateCheckResult>("check_for_updates");
      setResult(check);
      setState(
        check.kind === "updateAvailable"
          ? "available"
          : check.kind === "upToDate"
            ? "uptodate"
            : check.kind === "noRelease"
              ? "norelease"
              : "failed",
      );
    } catch (error) {
      // Rust command errors arrive as plain strings; show them unchanged.
      const message =
        typeof error === "string" ? error : "The update check could not be started.";
      setResult({ kind: "failed", message });
      setState("failed");
    }
    setPopoverOpen(true);
  }, []);

  const openDownloadPage = useCallback(async () => {
    setOpenError(null);
    try {
      await invoke("open_update_page");
    } catch (error) {
      setOpenError(typeof error === "string" ? error : "Couldn't open your browser.");
    }
  }, []);

  const onCircleClick = () => {
    if (state === "checking") return; // clicks are ignored while checking
    if (state === "available") {
      setPopoverOpen((open) => !open);
      return;
    }
    void runCheck();
  };

  // Popover: close on outside click; Escape also returns focus to the circle.
  useEffect(() => {
    if (!popoverOpen) return;
    const onPointerDown = (event: PointerEvent) => {
      if (rootRef.current && !rootRef.current.contains(event.target as Node)) {
        setPopoverOpen(false);
      }
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setPopoverOpen(false);
        circleRef.current?.focus();
      }
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [popoverOpen]);

  const popoverMessage = (() => {
    if (!result) return null;
    switch (result.kind) {
      case "upToDate":
        return `You have the latest version (${result.current}).`;
      case "noRelease":
        return "No published release was found on GitHub yet.";
      case "failed":
        return result.message;
      case "updateAvailable":
        return `Version ${result.latest} is available. You have ${result.current}.`;
    }
  })();

  return (
    <div className="update-badge" ref={rootRef}>
      <span className="update-badge-pill">
        Version {version}
        <button
          ref={circleRef}
          type="button"
          className={`update-badge-circle update-badge-${state}`}
          title={CIRCLE_LABELS[state]}
          aria-label={CIRCLE_LABELS[state]}
          onClick={onCircleClick}
        >
          {state === "checking" ? (
            <>
              <span className="update-badge-spinner" aria-hidden="true" />
              <span className="update-badge-paused" aria-hidden="true">
                …
              </span>
            </>
          ) : state === "uptodate" ? (
            <Check size={13} strokeWidth={2.5} aria-hidden="true" />
          ) : state === "failed" ? (
            <TriangleAlert size={13} strokeWidth={2.5} aria-hidden="true" />
          ) : (
            <ArrowUp size={13} strokeWidth={2.5} aria-hidden="true" />
          )}
        </button>
      </span>
      {popoverOpen && popoverMessage !== null && (
        <div className="update-badge-popover" role="status" aria-live="polite">
          <p className="update-badge-message">{popoverMessage}</p>
          {result?.kind === "updateAvailable" && (
            <>
              <button
                type="button"
                className="button-secondary update-badge-open"
                onClick={() => void openDownloadPage()}
              >
                Open download page
              </button>
              {openError !== null && <p className="update-badge-open-error">{openError}</p>}
              <p className="update-badge-never">
                ERX Tools never downloads or installs updates by itself.
              </p>
            </>
          )}
        </div>
      )}
    </div>
  );
}
