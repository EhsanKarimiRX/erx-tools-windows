import type { ReactNode } from "react";

/** Small gray note; used for honest "how it works" explanations. */
export default function InfoNote({ children }: { children: ReactNode }) {
  return <p className="sm-note-info">{children}</p>;
}
