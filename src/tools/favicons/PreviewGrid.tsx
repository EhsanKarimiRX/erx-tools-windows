import type { IconPreview } from "./types";
import { formatBytes } from "./types";

interface PreviewGridProps {
  icons: IconPreview[];
  busy: boolean;
}

/** Icons up to this width are small enough to need enlarging. */
const ENLARGE_UP_TO = 48;
/** Shown size (px) of an enlarged small icon, and the cap for big icons. */
const SHOWN_SIZE = 96;

/** Every generated PNG, drawn on a checkerboard so transparency is visible.
 * The tiny favicons (16 and 32 px) are enlarged so padding can be seen. */
export default function PreviewGrid({ icons, busy }: PreviewGridProps) {
  return (
    <section className="fav-card" aria-labelledby="fav-preview-title" aria-busy={busy}>
      <h2 id="fav-preview-title" className="fav-heading">
        Preview
      </h2>
      <ul className={busy ? "fav-grid fav-grid-busy" : "fav-grid"}>
        {icons.map((icon) => {
          const enlarged = icon.width <= ENLARGE_UP_TO;
          const shown = enlarged ? SHOWN_SIZE : Math.min(icon.width, SHOWN_SIZE);
          return (
            <li key={icon.fileName} className="fav-icon">
              <div className="fav-icon-box fav-checker">
                <img
                  className={enlarged ? "fav-pixelated" : undefined}
                  src={icon.dataUrl}
                  width={shown}
                  height={shown}
                  alt={`Preview of ${icon.fileName}`}
                />
              </div>
              <span className="fav-icon-name">{icon.fileName}</span>
              <span className="fav-icon-meta">
                {icon.width}×{icon.height} · {formatBytes(icon.sizeBytes)}
              </span>
            </li>
          );
        })}
      </ul>
      <p className="fav-hint">
        The 16×16 and 32×32 icons are enlarged here so you can see the padding. Their real size is much smaller.
      </p>
    </section>
  );
}
