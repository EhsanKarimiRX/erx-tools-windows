import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import ContentTabs from "./ContentTabs";
import OptionsPanel from "./OptionsPanel";
import type {
  ContentTab,
  QrContent,
  QrOptions,
  QrSvgResult,
  WifiSecurity,
} from "./types";
import {
  DEFAULT_MARGIN,
  DEFAULT_PNG_SIZE,
  isHexColor,
  previewUrl,
} from "./types";
import "./qr-code.css";

/** Preview refresh delay: typing pauses under this don't re-render. */
const DEBOUNCE_MS = 200;

/** Turns anything thrown by invoke into a short display string. */
function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

type PreviewState =
  | { status: "empty" }
  | { status: "ok"; result: QrSvgResult }
  | { status: "error"; message: string };

/** QR Code Generator: content on the left, live preview and export on the
 * right. The preview SVG is the exact document Save SVG writes and the PNG
 * is rasterized from the same geometry, so what you scan is what you get. */
export default function QrCodeGenerator() {
  // --- content state ---
  const [tab, setTab] = useState<ContentTab>("text");
  const [text, setText] = useState("");
  const [url, setUrl] = useState("");
  const [ssid, setSsid] = useState("");
  const [password, setPassword] = useState("");
  const [security, setSecurity] = useState<WifiSecurity>("wpa");
  const [hidden, setHidden] = useState(false);

  // --- options state ---
  const [style, setStyle] = useState<QrOptions["style"]>("sharp");
  const [foreground, setForeground] = useState("#000000");
  const [background, setBackground] = useState("#ffffff");
  const [transparent, setTransparent] = useState(false);
  const [errorCorrection, setErrorCorrection] =
    useState<QrOptions["errorCorrection"]>("Q");
  const [margin, setMargin] = useState(DEFAULT_MARGIN);
  const [pngSize, setPngSize] = useState(DEFAULT_PNG_SIZE);

  // --- preview + save ---
  const [preview, setPreview] = useState<PreviewState>({ status: "empty" });
  const [saving, setSaving] = useState<"svg" | "png" | null>(null);
  const [savedNote, setSavedNote] = useState<string | null>(null);
  const requestRef = useRef(0);

  // Leaving the Wi-Fi tab clears the password immediately; it must never
  // linger anywhere but its own tab (unmount clears everything anyway).
  function switchTab(next: ContentTab) {
    if (next !== "wifi") setPassword("");
    setSavedNote(null);
    setTab(next);
  }

  const content: QrContent =
    tab === "text"
      ? { kind: "text", value: text }
      : tab === "website"
        ? { kind: "website", url }
        : { kind: "wifi", ssid, password, security, hidden };

  const colorsValid = isHexColor(foreground) && isHexColor(background);
  const hasContent =
    tab === "text"
      ? text.trim().length > 0
      : tab === "website"
        ? previewUrl(url).length > 0
        : ssid.trim().length > 0;
  const canGenerate = hasContent && colorsValid;

  // Live preview: regenerate (debounced) whenever content or options
  // change. A request counter drops responses that arrive out of order.
  useEffect(() => {
    if (!canGenerate) {
      setPreview({ status: "empty" });
      return;
    }
    const options: QrOptions = {
      content,
      style,
      foreground,
      background,
      transparent,
      errorCorrection,
      margin,
      pngSize,
    };
    const request = ++requestRef.current;
    const timer = window.setTimeout(() => {
      invoke<QrSvgResult>("generate_qr", { options })
        .then((result) => {
          if (requestRef.current === request) setPreview({ status: "ok", result });
        })
        .catch((error) => {
          if (requestRef.current === request)
            setPreview({ status: "error", message: errorText(error) });
        });
    }, DEBOUNCE_MS);
    return () => window.clearTimeout(timer);
  }, [
    text,
    url,
    ssid,
    password,
    security,
    hidden,
    tab,
    style,
    foreground,
    background,
    transparent,
    errorCorrection,
    margin,
    pngSize,
    canGenerate,
  ]);

  /** Shared save path: same options as the preview, native Save As. */
  async function save(kind: "svg" | "png") {
    setSaving(kind);
    setSavedNote(null);
    try {
      const options: QrOptions = {
        content,
        style,
        foreground,
        background,
        transparent,
        errorCorrection,
        margin,
        pngSize,
      };
      const command = kind === "svg" ? "save_qr_svg" : "save_qr_png";
      const savedPath = await invoke<string | null>(command, { options });
      if (savedPath) setSavedNote(`Saved: ${savedPath}`);
    } catch (error) {
      setSavedNote(errorText(error));
    } finally {
      setSaving(null);
    }
  }

  const previewArea = (() => {
    if (preview.status === "empty") {
      return (
        <p className="qr-preview-hint">
          {tab === "wifi"
            ? "Enter the network name to see the QR code."
            : tab === "website"
              ? "Enter a website address to see the QR code."
              : "Enter some text to see the QR code."}
        </p>
      );
    }
    if (preview.status === "error") {
      return (
        <div className="qr-error" role="alert">
          {preview.message}
        </div>
      );
    }
    return (
      <div
        className="qr-svg-holder"
        // The SVG string is fully generated by our own Rust code from
        // validated colors; it contains no scripts and no user text.
        dangerouslySetInnerHTML={{ __html: preview.result.svg }}
      />
    );
  })();

  const result = preview.status === "ok" ? preview.result : null;

  return (
    <div className="qr-layout">
      {/* ----- left: content + options ----- */}
      <section className="qr-main" aria-label="Content and options">
        <ContentTabs
          tab={tab}
          onTab={switchTab}
          text={text}
          onText={setText}
          url={url}
          onUrl={setUrl}
          ssid={ssid}
          onSsid={setSsid}
          password={password}
          onPassword={setPassword}
          security={security}
          onSecurity={setSecurity}
          hidden={hidden}
          onHidden={setHidden}
        />

        {result && (
          <div className="qr-fieldset">
            <p className="qr-counter">
              {result.charCount} / {result.maxChars} characters encoded
            </p>
          </div>
        )}

        <OptionsPanel
          style={style}
          onStyle={setStyle}
          foreground={foreground}
          onForeground={setForeground}
          background={background}
          onBackground={setBackground}
          transparent={transparent}
          onTransparent={setTransparent}
          errorCorrection={errorCorrection}
          onErrorCorrection={setErrorCorrection}
          margin={margin}
          onMargin={setMargin}
          pngSize={pngSize}
          onPngSize={setPngSize}
          pngPixels={result?.pngPixels ?? null}
          pngPixelsPerModule={result?.pngPixelsPerModule ?? null}
          levelLocked={false}
        />
      </section>

      {/* ----- right: preview + export ----- */}
      <section className="qr-side" aria-label="Preview and export">
        <div className="qr-preview" aria-live="polite">
          {previewArea}
        </div>

        {result && result.warnings.length > 0 && (
          <ul className="qr-warnings">
            {result.warnings.map((warning) => (
              <li key={warning}>{warning}</li>
            ))}
          </ul>
        )}

        {result && (
          <p className="qr-meta">
            {result.totalModules} × {result.totalModules} modules · error
            correction {result.levelUsed}
          </p>
        )}

        <div className="qr-actions">
          <button
            type="button"
            className="button-primary"
            onClick={() => save("png")}
            disabled={!canGenerate || preview.status !== "ok" || saving !== null}
            title={canGenerate ? undefined : "Enter some content first"}
          >
            {saving === "png" ? "Saving…" : "Save PNG"}
          </button>
          <button
            type="button"
            className="button-secondary"
            onClick={() => save("svg")}
            disabled={!canGenerate || preview.status !== "ok" || saving !== null}
            title={canGenerate ? undefined : "Enter some content first"}
          >
            {saving === "svg" ? "Saving…" : "Save SVG"}
          </button>
        </div>
        {!canGenerate && hasContent && !colorsValid && (
          <p className="qr-error" role="alert">
            Colors must use the #RRGGBB form.
          </p>
        )}
        {savedNote && <p className="qr-saved-note">{savedNote}</p>}
      </section>
    </div>
  );
}
