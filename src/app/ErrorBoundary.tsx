import { Component } from "react";
import type { ErrorInfo, ReactNode } from "react";
import ToolLayout from "../shared/ToolLayout";

interface ErrorBoundaryProps {
  toolName: string;
  onBack: () => void;
  children: ReactNode;
}

interface ErrorBoundaryState {
  error: Error | null;
}

/**
 * Keeps a crashed tool from taking down the whole app: the tool screen is
 * replaced by a friendly error message with a Back button; the home screen
 * and the other tools keep working.
 */
export default class ErrorBoundary extends Component<
  ErrorBoundaryProps,
  ErrorBoundaryState
> {
  state: ErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    // Full details go to the devtools console; the user sees a short message.
    console.error("Tool crashed:", error, info.componentStack);
  }

  render(): ReactNode {
    if (this.state.error !== null) {
      return (
        <ToolLayout title={this.props.toolName} onBack={this.props.onBack}>
          <div className="tool-error">
            <p className="tool-error-title">Something went wrong in this tool.</p>
            <p className="tool-error-hint">
              The rest of the app still works. Please go back and try again.
            </p>
          </div>
        </ToolLayout>
      );
    }
    return this.props.children;
  }
}
