"use client";
import type { ReactNode } from "react";
import { Button } from "./button";
import "../../../styles/foundation.css";

export interface ErrorStateProps {
  title?: string;
  children?: ReactNode;
  onRetry?: () => void;
  onDismiss?: () => void;
  busy?: boolean;
}
export function ErrorState({
  title = "Something went wrong",
  children,
  onRetry,
  onDismiss,
  busy,
}: ErrorStateProps) {
  return (
    <div className="sc-error">
      <div role="alert">
        <h3>{title}</h3>
        {children && <div className="sc-state-copy">{children}</div>}
      </div>
      <div className="sc-actions">
        {onRetry && (
          <Button variant="secondary" onClick={onRetry} busy={busy}>
            Retry
          </Button>
        )}
        {onDismiss && (
          <Button variant="quiet" onClick={onDismiss}>
            Dismiss
          </Button>
        )}
      </div>
    </div>
  );
}
