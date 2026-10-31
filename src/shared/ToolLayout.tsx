import type { ReactNode } from "react";
import { ArrowLeft } from "lucide-react";

interface ToolLayoutProps {
  title: string;
  onBack: () => void;
  children: ReactNode;
}

/** Shared screen layout for every tool: Back button + title header. */
export default function ToolLayout({ title, onBack, children }: ToolLayoutProps) {
  return (
    <div className="tool">
      <header className="tool-header">
        <button
          type="button"
          className="back-button"
          onClick={onBack}
          aria-label="Back to home"
        >
          <ArrowLeft size={16} aria-hidden="true" /> Back
        </button>
        <h1 className="tool-title">{title}</h1>
      </header>
      <main className="tool-content">{children}</main>
    </div>
  );
}
