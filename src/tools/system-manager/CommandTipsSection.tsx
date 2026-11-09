import { Fragment, useCallback, useState } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import InfoNote from "./InfoNote";
import SectionCard from "./SectionCard";
import type { CommandStep, CommandTip, TipShell } from "./commandTips";
import { COMMAND_TIPS_MAIN, COMMAND_TIPS_MORE, stepText } from "./commandTips";

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

/** One step of one command: its own code box and its own Copy button, so
 * the copied text is always exactly the one line shown in that box. */
function StepBox({ step, tipTitle }: { step: CommandStep; tipTitle: string }) {
  const [copied, setCopied] = useState(false);
  const [failed, setFailed] = useState(false);

  const copy = useCallback(async () => {
    const result = await copyText(stepText(step));
    setCopied(result === "copied");
    setFailed(result === "failed");
    if (result === "copied") {
      window.setTimeout(() => setCopied(false), 2_000);
    }
  }, [step]);

  return (
    <div className="sm-tip-step">
      {step.label !== undefined && <div className="sm-tip-step-label">{step.label}</div>}
      <div className="sm-tip-command">
        {/* The parts are joined with NOTHING between them — whitespace is
            significant inside <pre> and the copy text must match exactly. */}
        <pre className="sm-tip-command-text">
          {step.parts.map((part, index) =>
            typeof part === "string" ? (
              <Fragment key={index}>{part}</Fragment>
            ) : (
              <span key={index} className="sm-tip-placeholder">
                {part.placeholder}
              </span>
            ),
          )}
        </pre>
        <button
          type="button"
          className="sm-tip-copy"
          onClick={() => void copy()}
          aria-label={step.label !== undefined ? `Copy ${step.label}` : `Copy command for ${tipTitle}`}
        >
          {copied ? "Copied" : "Copy"}
        </button>
      </div>
      {failed && (
        <p className="sm-icon-row-error" role="alert">
          {COPY_FAILED}
        </p>
      )}
      {step.replaceWith !== undefined && (
        <p className="sm-tip-replace-note">
          Replace the <span className="sm-tip-placeholder">green text</span> with {step.replaceWith}
        </p>
      )}
    </div>
  );
}

/** One command = one clearly separated card, so a note can never look like
 * it belongs to the next command. */
function TipCard({ tip }: { tip: CommandTip }) {
  return (
    <div className="sm-tip-card">
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
      <div className="sm-tip-steps">
        {tip.steps.map((step, index) => (
          <StepBox key={index} step={step} tipTitle={tip.title} />
        ))}
      </div>
    </div>
  );
}

/** Command Tips, the bottom-most section: a static, copy-only list. The
 * data lives in commandTips.ts so it stays easy to edit. No category
 * headings; the five rarer commands live in a collapsed drawer. */
export default function CommandTipsSection() {
  const [moreOpen, setMoreOpen] = useState(false);

  return (
    <SectionCard
      title="Command Tips"
      subtitle="Handy Windows commands. Copy one, then paste it into PowerShell or Command Prompt (the buttons above open them)."
    >
      <div className="sm-tip-list">
        {COMMAND_TIPS_MAIN.map((tip) => (
          <TipCard key={tip.id} tip={tip} />
        ))}
      </div>

      <button
        type="button"
        className="sm-awake-options-toggle sm-tip-more-toggle"
        onClick={() => setMoreOpen((open) => !open)}
        aria-expanded={moreOpen}
        aria-controls="sm-tip-more-body"
      >
        {moreOpen ? <ChevronDown size={14} aria-hidden="true" /> : <ChevronRight size={14} aria-hidden="true" />}
        More commands ({COMMAND_TIPS_MORE.length})
      </button>
      {moreOpen && (
        <div id="sm-tip-more-body" className="sm-tip-more-body sm-tip-list">
          {COMMAND_TIPS_MORE.map((tip) => (
            <TipCard key={tip.id} tip={tip} />
          ))}
        </div>
      )}

      <InfoNote>These commands are only text. ERX Tools never runs them for you.</InfoNote>
    </SectionCard>
  );
}
