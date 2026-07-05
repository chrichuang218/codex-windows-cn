import { Component, type ErrorInfo, type ReactNode } from "react";
import { ErrorShell } from "./components/Shell";

type ErrorBoundaryProps = {
  children: ReactNode;
};

type ErrorBoundaryState = {
  message: string | null;
};

export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  state: ErrorBoundaryState = {
    message: null
  };

  static getDerivedStateFromError(error: unknown): ErrorBoundaryState {
    return {
      message: error instanceof Error ? error.message : "React 首页启动失败"
    };
  }

  componentDidCatch(error: unknown, info: ErrorInfo) {
    console.error("Codex Windows React startup failed", error, info);
  }

  render() {
    if (this.state.message) {
      return <ErrorShell message={this.state.message} />;
    }

    return this.props.children;
  }
}
