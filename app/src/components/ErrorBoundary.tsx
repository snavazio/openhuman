import { Component, type ErrorInfo, type ReactNode } from 'react';

/** Arguments handed to the fallback renderer (same shape as Sentry's boundary). */
export interface ErrorBoundaryFallbackArgs {
  error: unknown;
  componentStack: string;
  resetError: () => void;
  /** Always `null`: nothing is reported off-device, so there is no event id. */
  eventId: string | null;
}

interface ErrorBoundaryProps {
  children: ReactNode;
  fallback: (args: ErrorBoundaryFallbackArgs) => ReactNode;
}

interface ErrorBoundaryState {
  hasError: boolean;
  error: unknown;
  componentStack: string;
}

/**
 * LOCAL-ONLY FORK: drop-in replacement for `Sentry.ErrorBoundary`. Catches a
 * render crash anywhere below it and renders `fallback` (the
 * ErrorFallbackScreen), with `resetError` to retry in place. The crash is
 * logged to the console only — it is never sent anywhere.
 */
export default class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  state: ErrorBoundaryState = { hasError: false, error: null, componentStack: '' };

  static getDerivedStateFromError(error: unknown): Partial<ErrorBoundaryState> {
    return { hasError: true, error };
  }

  componentDidCatch(error: unknown, info: ErrorInfo): void {
    console.error('[error-boundary] render crash', error);
    this.setState({ componentStack: info.componentStack ?? '' });
  }

  resetError = (): void => {
    this.setState({ hasError: false, error: null, componentStack: '' });
  };

  render(): ReactNode {
    if (this.state.hasError) {
      return this.props.fallback({
        error: this.state.error,
        componentStack: this.state.componentStack,
        resetError: this.resetError,
        eventId: null,
      });
    }
    return this.props.children;
  }
}
