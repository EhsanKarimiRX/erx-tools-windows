import { useCallback, useEffect, useState } from "react";
import { ChevronDown, ChevronUp, RefreshCw } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import SectionCard from "./SectionCard";
import type { SystemCardCell, SystemCardData } from "./system-info-types";
import "./system-info-card.css";

/** Turns anything thrown by invoke into a short display string. */
function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

interface SpecProps {
  label: string;
  cell: SystemCardCell | null;
  loading: boolean;
}

/** One cell of the card. A missing value is shown honestly. */
function Spec({ label, cell, loading }: SpecProps) {
  const text = cell !== null ? cell.value : loading ? "Reading\u2026" : "Not reported";
  return (
    <div className="sic-item">
      <span className="sic-label">{label}</span>
      <strong className={cell === null ? "sic-value sic-muted" : "sic-value"} title={cell?.value}>
        {text}
      </strong>
      {cell !== null && cell.detail !== "" && <span className="sic-detail">{cell.detail}</span>}
    </div>
  );
}

interface SixthProps {
  card: SystemCardData | null;
  loading: boolean;
}

/** Battery health on a PC with a battery; uptime on a PC without one. */
function SixthCell({ card, loading }: SixthProps) {
  if (card !== null && card.battery !== null) {
    const { healthPercent, detail } = card.battery;
    const showDetail = detail !== "" && !(healthPercent === null && detail === "Health not reported");
    return (
      <div className="sic-item">
        <span className="sic-label">Battery Health</span>
        <div className="sic-battery-row">
          <strong className={healthPercent === null ? "sic-value sic-muted" : "sic-value"}>
            {healthPercent === null ? "Not reported" : `${healthPercent}%`}
          </strong>
          {healthPercent !== null && (
            <span className="sic-track" aria-hidden="true">
              <span className="sic-fill" style={{ width: `${healthPercent}%` }} />
            </span>
          )}
        </div>
        {showDetail && <span className="sic-detail">{detail}</span>}
      </div>
    );
  }
  const uptime = card?.uptime ?? null;
  const cell: SystemCardCell | null =
    uptime !== null ? { value: uptime, detail: "since last full restart" } : null;
  return <Spec label="Uptime" cell={cell} loading={loading} />;
}

/** The device card at the top of System Manager. Static, not live: it reads
 * everything when it opens and when the refresh button is pressed. */
export default function SystemInfoCard() {
  const [card, setCard] = useState<SystemCardData | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showMore, setShowMore] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setCard(await invoke<SystemCardData>("system_info_get"));
    } catch (failure) {
      setError(errorText(failure));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const hasMore = card !== null && card.more.length > 0;

  return (
    <>
      <div className="sic-stage">
        <div className="sic-scene">
          <section className="sic-card" aria-label="Device status">
            <span className="sic-fx" aria-hidden="true" />
            <header className="sic-header">
              <div>
                <p className="sic-eyebrow">System Manager</p>
                <h2 className="sic-title">DEVICE STATUS</h2>
              </div>
              <div className="sic-pill">
                <span className="sic-dot" />
                System Overview
              </div>
            </header>

            <div className="sic-grid">
              <Spec label="Processor" cell={card?.processor ?? null} loading={loading} />
              <Spec label="Graphics" cell={card?.graphics ?? null} loading={loading} />
              <Spec label={"Memory \u00B7 RAM"} cell={card?.memory ?? null} loading={loading} />
              <Spec label="Storage" cell={card?.storage ?? null} loading={loading} />
              <Spec label="Operating System" cell={card?.os ?? null} loading={loading} />
              <SixthCell card={card} loading={loading} />
            </div>

            <footer className="sic-footer">
              <span>Hardware Overview</span>
              <span className="sic-actions">
                {hasMore && (
                  <button
                    type="button"
                    className="sic-button"
                    aria-expanded={showMore}
                    onClick={() => setShowMore((open) => !open)}
                  >
                    More
                    {showMore ? (
                      <ChevronUp size={13} aria-hidden="true" />
                    ) : (
                      <ChevronDown size={13} aria-hidden="true" />
                    )}
                  </button>
                )}
                <button
                  type="button"
                  className="sic-button"
                  onClick={() => void load()}
                  disabled={loading}
                  aria-label="Refresh hardware information"
                  title="Refresh"
                >
                  <RefreshCw size={13} className={loading ? "sic-spin" : undefined} aria-hidden="true" />
                </button>
              </span>
            </footer>
          </section>
        </div>
        {error !== null && (
          <p className="sic-error" role="alert">
            Couldn't read the hardware information: {error}
          </p>
        )}
      </div>

      {showMore && card !== null && card.more.length > 0 && (
        <SectionCard title="All hardware" subtitle="Everything Windows reported, one line per item.">
          {card.more.map((group) => (
            <div key={group.title} className="sic-more-group">
              <h3 className="sic-more-title">{group.title}</h3>
              {group.items.map((item, index) => (
                <div key={`${group.title}-${index}`} className="sic-more-row">
                  <span className="sic-more-name">{item.name}</span>
                  {item.detail !== "" && <span className="sic-more-detail">{item.detail}</span>}
                </div>
              ))}
            </div>
          ))}
        </SectionCard>
      )}
    </>
  );
}
