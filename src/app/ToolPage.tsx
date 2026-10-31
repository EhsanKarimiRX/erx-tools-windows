import { Suspense, lazy, useMemo } from "react";
import type { ToolDefinition } from "../tools/registry";
import ToolLayout from "../shared/ToolLayout";
import ErrorBoundary from "./ErrorBoundary";

interface ToolPageProps {
  tool: ToolDefinition;
  onBack: () => void;
}

/**
 * Host screen for one tool: shared header (ToolLayout), error boundary and
 * lazy loading. The tool component itself only renders its content.
 */
export default function ToolPage({ tool, onBack }: ToolPageProps) {
  // Built once per tool; going back unmounts this page, so state resets.
  const ToolComponent = useMemo(() => lazy(tool.load), [tool]);

  return (
    <ErrorBoundary toolName={tool.name} onBack={onBack}>
      <ToolLayout title={tool.name} onBack={onBack}>
        <Suspense fallback={<p className="tool-loading">Loading…</p>}>
          <ToolComponent />
        </Suspense>
      </ToolLayout>
    </ErrorBoundary>
  );
}
