import { useEffect, useRef, useState } from "react";

type CopyStatus = "idle" | "copied" | "failed";

interface SnippetPanelProps {
  snippet: string;
}

/** The HTML <head> snippet with a Copy button. */
export default function SnippetPanel({ snippet }: SnippetPanelProps) {
  const [status, setStatus] = useState<CopyStatus>("idle");
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => () => window.clearTimeout(timer.current), []);

  const flash = (next: CopyStatus) => {
    setStatus(next);
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setStatus("idle"), 2500);
  };

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(snippet);
      flash("copied");
      return;
    } catch {
      // fall through to the textarea fallback
    }
    try {
      const area = document.createElement("textarea");
      area.value = snippet;
      area.setAttribute("readonly", "");
      area.style.position = "fixed";
      area.style.opacity = "0";
      document.body.appendChild(area);
      area.select();
      const ok = document.execCommand("copy");
      document.body.removeChild(area);
      flash(ok ? "copied" : "failed");
    } catch {
      flash("failed");
    }
  };

  return (
    <section className="fav-card" aria-labelledby="fav-snippet-title">
      <h2 id="fav-snippet-title" className="fav-heading">
        HTML snippet
      </h2>
      <p className="fav-hint">
        Paste these lines inside the &lt;head&gt; of your pages, and upload all generated files to the root folder of
        your website.
      </p>
      <pre className="fav-snippet" tabIndex={0}>
        <code>{snippet}</code>
      </pre>
      <div className="fav-snippet-row">
        <button type="button" className="button-secondary" onClick={copy}>
          Copy snippet
        </button>
        <span className="fav-hint" role="status">
          {status === "copied" && "Copied."}
          {status === "failed" && "Could not copy. Select the text and press Ctrl+C."}
        </span>
      </div>
    </section>
  );
}
