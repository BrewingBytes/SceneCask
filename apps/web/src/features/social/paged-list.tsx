"use client";
import type { ReactNode } from "react";
import { Button, ErrorState, Skeleton } from "../../components/ui/basic";
import { socialFailureCopy } from "./copy";
import type { Paged } from "./use-paged";

export interface PagedListProps<T> {
  list: Paged<T>;
  /** Accessible name of the list. */
  label: string;
  loadingLabel: string;
  errorTitle: string;
  empty: ReactNode;
  retry: () => void;
  more: () => void;
  itemKey: (item: T) => string;
  children: (item: T) => ReactNode;
  className?: string;
}

/**
 * Loading skeleton, error with Retry, empty state, the rows, and cursor "Show more" with its own
 * failure. Idle renders nothing: the caller explains what's needed to start.
 */
export function PagedList<T>({ list, label, loadingLabel, errorTitle, empty, retry, more, itemKey, children, className = "sc-social-list" }: PagedListProps<T>) {
  if (list.state === "idle") return null;
  if (list.state === "loading")
    return (
      <div className="sc-social-loading">
        <Skeleton label={loadingLabel} height={44} />
        <Skeleton decorative height={44} />
        <Skeleton decorative height={44} />
      </div>
    );
  if (list.state === "error")
    return (
      <ErrorState title={errorTitle} onRetry={retry}>
        {socialFailureCopy(list.failure)}
      </ErrorState>
    );
  if (list.items.length === 0) return <>{empty}</>;
  return (
    <>
      <ul className={className} aria-label={label}>
        {list.items.map((item) => (
          <li key={itemKey(item)}>{children(item)}</li>
        ))}
      </ul>
      {typeof list.more === "object" && (
        <p className="sc-social-problem" role="alert">
          More didn’t load. {socialFailureCopy(list.more)}
        </p>
      )}
      {list.nextCursor && (
        <div className="sc-actions">
          <Button variant="secondary" busy={list.more === "loading"} onClick={more}>
            Show more
          </Button>
        </div>
      )}
    </>
  );
}
