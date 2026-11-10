import { Component, type ReactNode } from "react";

interface Props {
  children: ReactNode;
}

interface State {
  error: Error | null;
}

export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(
    error: Error,
    info: { componentStack?: string | null },
  ): void {
    console.error("[corral] render error", error, info.componentStack);
  }

  render() {
    if (this.state.error) {
      return (
        <div className="flex h-screen items-center justify-center bg-bg text-text p-5 text-center">
          <div>
            <p className="text-base font-semibold mb-2">
              Something broke on my end. Restart and try again.
            </p>
            <p className="text-muted text-sub font-mono break-all">
              {this.state.error.message}
            </p>
          </div>
        </div>
      );
    }
    return this.props.children;
  }
}
