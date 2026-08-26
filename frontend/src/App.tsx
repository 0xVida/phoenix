import { Component, useEffect, type ErrorInfo, type ReactNode } from "react";

import Console from "./pages/Console";
import Landing from "./pages/Landing";

type ErrorBoundaryProps = {
  children: ReactNode;
};

type ErrorBoundaryState = {
  hasError: boolean;
};

class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  override state: ErrorBoundaryState = { hasError: false };

  static getDerivedStateFromError(): ErrorBoundaryState {
    return { hasError: true };
  }

  override componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    console.error("Phoenix CI application error", error, errorInfo);
  }

  override render() {
    if (this.state.hasError) {
      return <ErrorPage />;
    }

    return this.props.children;
  }
}

function ErrorPage() {
  return (
    <div className="flex min-h-screen items-center justify-center bg-background px-4 text-foreground">
      <div className="max-w-md text-center">
        <h1 className="text-xl font-semibold tracking-tight">This page didn&apos;t load</h1>
        <p className="mt-2 text-sm text-muted">
          Something went wrong on our end. You can try refreshing or head back home.
        </p>
        <div className="mt-6 flex flex-wrap justify-center gap-2">
          <button
            onClick={() => window.location.reload()}
            className="inline-flex items-center justify-center rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90"
          >
            Try again
          </button>
          <a
            href="/"
            className="inline-flex items-center justify-center rounded-md border border-input bg-background px-4 py-2 text-sm font-medium text-foreground transition-colors hover:bg-accent"
          >
            Go home
          </a>
        </div>
      </div>
    </div>
  );
}

function NotFoundPage() {
  return (
    <div className="flex min-h-screen items-center justify-center bg-background px-4 text-foreground">
      <div className="max-w-md text-center">
        <h1 className="text-7xl font-bold">404</h1>
        <h2 className="mt-4 text-xl font-semibold">Page not found</h2>
        <p className="mt-2 text-sm text-muted">
          The page you&apos;re looking for doesn&apos;t exist or has been moved.
        </p>
        <a
          href="/"
          className="mt-6 inline-flex items-center justify-center rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90"
        >
          Go home
        </a>
      </div>
    </div>
  );
}

function normalizePath(pathname: string) {
  const path = pathname.replace(/\/+$/, "");
  return path || "/";
}

function RouteContent() {
  const path = normalizePath(window.location.pathname);

  useEffect(() => {
    const description = document.querySelector('meta[name="description"]');

    if (path === "/console") {
      document.title = "Phoenix CI — Self-Healing PR Review Console";
      description?.setAttribute(
        "content",
        "Operator console for Phoenix CI with lease heartbeats and live kill-and-reassign recovery.",
      );
      return;
    }

    if (path === "/") {
      document.title = "Phoenix CI — Self-Healing PR Review Behind a Merge Gate";
      description?.setAttribute(
        "content",
        "Phoenix CI pairs a planner and implementer agent with a deterministic merge gate.",
      );
      return;
    }

    document.title = "Page not found — Phoenix CI";
  }, [path]);

  if (path === "/") return <Landing />;
  if (path === "/console") return <Console />;
  return <NotFoundPage />;
}

export default function App() {
  return (
    <ErrorBoundary>
      <RouteContent />
    </ErrorBoundary>
  );
}
