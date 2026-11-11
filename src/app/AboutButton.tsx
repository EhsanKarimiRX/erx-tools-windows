import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Info, X } from "lucide-react";
import logoUrl from "../assets/erx-logo.png";

/** Number of logo clicks inside the About dialog that triggers the Easter egg. */
const EGG_CLICKS = 5;

interface EggMessage {
  kind: "ok" | "error";
  text: string;
}

/** Round floating About button (bottom-right) plus the About dialog. */
export default function AboutButton({ version }: { version: string | null }) {
  const [open, setOpen] = useState(false);
  const [linkError, setLinkError] = useState<string | null>(null);
  const [eggMessage, setEggMessage] = useState<EggMessage | null>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const eggClicksRef = useRef(0);
  const eggBusyRef = useRef(false);
  const eggDoneRef = useRef(false);

  const close = useCallback(() => {
    setOpen(false);
    setLinkError(null);
    setEggMessage(null);
    eggClicksRef.current = 0;
    eggDoneRef.current = false;
  }, []);

  // Dialog open: focus the close button, close on Escape, give focus back on close.
  useEffect(() => {
    if (!open) return;
    closeRef.current?.focus();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") close();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      buttonRef.current?.focus();
    };
  }, [open, close]);

  const openGithub = useCallback(async () => {
    setLinkError(null);
    try {
      await invoke("open_github_page");
    } catch (error) {
      setLinkError(typeof error === "string" ? error : "Couldn't open your browser.");
    }
  }, []);

  // Easter egg: EGG_CLICKS clicks on the logo save the wallpaper to Downloads.
  const onLogoClick = useCallback(async () => {
    if (eggBusyRef.current || eggDoneRef.current) return;
    eggClicksRef.current += 1;
    if (eggClicksRef.current < EGG_CLICKS) return;
    eggClicksRef.current = 0;
    eggBusyRef.current = true;
    try {
      const fileName = await invoke<string>("save_wallpaper_to_downloads");
      eggDoneRef.current = true;
      setEggMessage({
        kind: "ok",
        text: `The suggested ERX Tools wallpaper was saved to your Downloads folder (${fileName}).`,
      });
    } catch (error) {
      setEggMessage({
        kind: "error",
        text: typeof error === "string" ? error : "Couldn't save the wallpaper.",
      });
    } finally {
      eggBusyRef.current = false;
    }
  }, []);

  return (
    <>
      <button
        ref={buttonRef}
        type="button"
        className="about-fab"
        title="About ERX Tools"
        aria-label="About ERX Tools"
        onClick={() => setOpen(true)}
      >
        <Info size={20} strokeWidth={2} aria-hidden="true" />
      </button>

      {open && (
        <div
          className="about-overlay"
          onClick={(event) => {
            if (event.target === event.currentTarget) close();
          }}
        >
          <div className="about-dialog" role="dialog" aria-modal="true" aria-labelledby="about-title">
            <button ref={closeRef} type="button" className="about-close" title="Close" aria-label="Close" onClick={close}>
              <X size={18} strokeWidth={2} aria-hidden="true" />
            </button>

            <div className="about-header">
              <img
                src={logoUrl}
                alt=""
                className="about-logo"
                width={72}
                height={72}
                draggable={false}
                onClick={() => void onLogoClick()}
              />
              <h2 id="about-title" className="about-title">ERX Tools</h2>
              {version !== null && <p className="about-version">Version {version}</p>}
            </div>

            <div className="about-body">
              <p>
                ERX Tools is a small, offline toolbox for Windows. In Ehsan&rsquo;s opinion, it is a set of
                tools that almost everyone who uses Windows will need at some point.
              </p>
              <p>
                It was built by combining Claude Sonnet 5.5 with the Z.ai agent, using the DeepSeek Flash
                and GLM Flash models.
              </p>
              <p className="about-license">
                Released under the MIT License. The ERX Tools name and logo are not covered by the MIT License.
              </p>
            </div>

            {eggMessage !== null && (
              <p
                className={eggMessage.kind === "ok" ? "about-egg" : "about-egg about-egg-error"}
                role="status"
                aria-live="polite"
              >
                {eggMessage.text}
              </p>
            )}

            <div className="about-footer">
              <button type="button" className="button-secondary" onClick={() => void openGithub()}>
                View on GitHub
              </button>
              {linkError !== null && (
                <p className="about-error" role="alert">{linkError}</p>
              )}
            </div>
          </div>
        </div>
      )}
    </>
  );
}
