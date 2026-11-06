import { useCallback, useState } from "react";
import InfoNote from "./InfoNote";
import SectionCard from "./SectionCard";
import type { CommandTip, TipShell } from "./commandTips";
import { COMMAND_TIP_GROUPS } from "./commandTips";

/** Copies text with an honest fallback chain (spec 5f): the async
 * clipboard API first, then the classic hidden-textarea trick, and a
 * plain "do it by hand" message if both fail. No Tauri clipboard plugin. */
async function copyText(text: string): Promise<"copied" | "failed"> {
  try {
    await navigator.clipboard.writeText(text);
    return "copied";
  } catch {
    // fall through to the legacy path
  }
  try {
    const area = document.createElement("textarea");
    area.value = text;
    area.setAttribute("readonly", "");
    area.style.position = "fixed";
    area.style.opacity = "0";
    document.body.appendChild(area);
    area.select();
    const ok = document.execCommand("copy");
    document.body.removeChild(area);
    return ok ? "copied" : "failed";
  } catch {
    return "failed";
  }
}

const COPY_FAILED =
  "Couldn't copy. Select the text and press Ctrl+C.";

function ShellBadge({ shell }: { shell: TipShell }) {
  return <span className="sm-tip-badge">{shell}</span>;
}

function TipRow({ tip }: { tip: CommandTip }) {
  const [copied, setCopied] = useState(false);
  const [copyFailed, setCopyFailed] = useState(false);

  const copy = useCallback(async () => {
    const result = await copyText(tip.commands.join("\n"));
    setCopied(result === "copied");
    setCopyFailed(result === "failed");
    if (result === "copied") {
      window.setTimeout(() => setCopied(false), 2_000);
    }
  }, [tip]);

  return (
    <div className="sm-tip">
      <div className="sm-tip-head">
        <span className="sm-tip-title">{tip.title}</span>
        <span className="sm-tip-badges">
          <ShellBadge shell={tip.shell} />
          {tip.needsAdministrator && <span className="sm-tip-badge sm-tip-badge-warn">Needs administrator</span>}
          {tip.canTakeLong && <span className="sm-tip-badge sm-tip-badge-warn">Can take a long time</span>}
          {tip.usesInternet && <span className="sm-tip-badge sm-tip-badge-warn">Uses the internet</span>}
        </span>
      </div>
      <p className="sm-note-info">{tip.explanation}</p>
      <div className="sm-tip-command">
        <pre className="sm-tip-command-text">{tip.commands.join("\n")}</pre>
        <button
          type="button"
          className="sm-tip-copy"
          onClick={() => void copy()}
          aria-label={`Copy command for ${tip.title}`}
        >
          {copied ? "Copied" : "Copy"}
        </button>
      </div>
      {copyFailed && (
        <p className="sm-icon-row-error" role="alert">
          {COPY_FAILED}
        </p>
      )}
      {tip.hasPlaceholder && (
        <p className="sm-note-info">Replace the part in &lt; &gt; before running.</p>
      )}
    </div>
  );
}

/** Command Tips, the bottom-most section: a static, copy-only list. The
 * data lives in commandTips.ts so it stays easy to edit. */
export default function CommandTipsSection() {
  return (
    <SectionCard
      title="Command Tips"
      subtitle="Handy Windows commands. Copy one, then paste it into PowerShell or Command Prompt (the buttons above open them)."
    >
      {COMMAND_TIP_GROUPS.map((group) => (
        <div key={group.heading} className="sm-tip-group">
          <h3 className="sm-tip-heading">{group.heading}</h3>
          {group.tips.map((tip) => (
            <TipRow key={tip.id} tip={tip} />
          ))}
        </div>
      ))}
      <InfoNote>These commands are only text. ERX Tools never runs them for you.</InfoNote>
    </SectionCard>
  );
}
