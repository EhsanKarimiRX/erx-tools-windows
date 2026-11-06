import type { ReactNode } from "react";

/** Small yellow note; used for cautionary text that stays true but
 * deserves attention (e.g. "closing the app keeps Awake on"). */
export default function WarnNote({ children }: { children: ReactNode }) {
  return <p className="sm-note-warn">{children}</p>;
}
