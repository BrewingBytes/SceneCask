"use client";
import Link from "next/link";
import { useSearchParams } from "next/navigation";
import { useEffect, useRef, useState } from "react";
import { Button, EmptyState, ErrorState, Skeleton, TextField } from "../../components/ui/basic";
import { ToastViewport, useToastQueue } from "../../components/ui/overlays";
import { STATUS_LABELS, STATUSES, trackingFailureCopy } from "../tracking/copy";
import type { Failure, LibraryQuery } from "./api";
import { noMatchText, parseStatus, showCount } from "./copy";
import { LIBRARY_SEARCH_DEBOUNCE_MS, MAX_LIBRARY_QUERY, type LibrarySnapshot } from "./library-client";
import { LibraryRow } from "./library-row";
import { useLibrary } from "./use-library";
import "./library.css";

export interface LibraryViewProps {
  /** Filters restored from the URL (?status=&q=) on load, reload and Back/Forward. */
  initialQuery?: LibraryQuery;
}

function hint(snapshot: LibrarySnapshot) {
  const { list, rows } = snapshot;
  if (list.status === "loading") return "Loading your library…";
  if (list.status === "error" || snapshot.counts?.all === 0) return "";
  if (rows.length === 0) return "No matching shows";
  return `${showCount(rows.length)}${list.nextCursor ? " so far" : ""}`;
}

function LoadFailure({ failure, onRetry }: { failure: Failure; onRetry: () => void }) {
  if (failure.kind === "auth")
    return (
      <EmptyState
        title="Sign in to see your library"
        action={<Link className="sc-button sc-button-primary" href="/auth/signin?returnTo=%2Flibrary">Sign in</Link>}
      >
        Your session has ended. Your library and progress are saved to your account.
      </EmptyState>
    );
  if (failure.kind === "unverified")
    return (
      <EmptyState title="Verify your email to use your library" action={<Link className="sc-button sc-button-primary" href="/auth/verify">Verify email</Link>}>
        Check your inbox for the verification link.
      </EmptyState>
    );
  return (
    <ErrorState title="Your library didn’t load" onRetry={onRetry}>
      {trackingFailureCopy(failure)}
    </ErrorState>
  );
}

/** D02 Library: manual-status filters with counts, title search and saved show rows. */
export function LibraryView({ initialQuery = { status: null, q: "" } }: LibraryViewProps) {
  // Back/Forward can remount from a cached render whose props predate replaceState; the live URL wins.
  const params = useSearchParams();
  const [start] = useState<LibraryQuery>(() => ({
    status: parseStatus(params.get("status")) ?? initialQuery.status,
    q: params.get("q") ?? initialQuery.q,
  }));
  const { toasts, show, update, dismiss } = useToastQueue();
  const [controls] = useState(() => ({ show, update, dismiss }));
  const { client, snapshot } = useLibrary(start, controls);
  const { query, list, rows, counts } = snapshot;
  const list$ = useRef<HTMLUListElement>(null);
  const focusFrom = useRef<number | null>(null);

  // Keep the settled filters in the URL without adding a history entry per keystroke.
  useEffect(() => {
    const timer = setTimeout(() => {
      const url = new URL(window.location.href);
      const term = query.q.trim();
      if (query.status) url.searchParams.set("status", query.status);
      else url.searchParams.delete("status");
      if (term) url.searchParams.set("q", term);
      else url.searchParams.delete("q");
      if (url.href !== window.location.href) window.history.replaceState(window.history.state, "", url);
    }, LIBRARY_SEARCH_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [query]);

  // After "Show more", focus the first appended row instead of losing focus with the button.
  useEffect(() => {
    if (focusFrom.current === null || rows.length <= focusFrom.current) return;
    list$.current?.querySelectorAll<HTMLElement>(".sc-library-open")[focusFrom.current]?.focus();
    focusFrom.current = null;
  }, [rows.length]);

  const empty = list.status === "ready" && counts ? counts.all === 0 : list.status === "ready" && rows.length === 0 && !query.status && !query.q.trim();
  const filtered = !!query.status || !!query.q.trim();
  const loadMore = () => {
    focusFrom.current = rows.length;
    void client.loadMore();
  };
  const clear = () => {
    client.setQuery({ status: null, q: "" });
    document.getElementById("sc-library-q")?.focus();
  };

  return (
    <section className="sc-library" aria-labelledby="sc-library-title">
      <div className="sc-library-head">
        <h1 id="sc-library-title">Library</h1>
        {counts && counts.all > 0 && <span className="sc-library-total">{showCount(counts.all)}</span>}
      </div>
      {!empty && list.status !== "error" && (
        <div className="sc-library-controls">
          <form role="search" className="sc-library-search" onSubmit={(event) => event.preventDefault()}>
            <TextField
              id="sc-library-q"
              type="search"
              label="Search your library"
              placeholder="Search your library"
              value={query.q}
              onChange={(event) => client.setQuery({ q: event.target.value })}
              maxLength={MAX_LIBRARY_QUERY}
              autoComplete="off"
              spellCheck={false}
              enterKeyHint="search"
            />
          </form>
          <div role="group" aria-label="Filter by status" className="sc-library-filters">
            {[null, ...STATUSES].map((status) => {
              const label = status ? STATUS_LABELS[status] : "All";
              const count = counts ? counts[status ?? "all"] : null;
              return (
                <button
                  key={status ?? "all"}
                  type="button"
                  className="sc-library-filter"
                  aria-pressed={query.status === status}
                  onClick={() => client.setQuery({ status })}
                >
                  {label}
                  {count !== null && <span className="sc-library-count"> {count}</span>}
                </button>
              );
            })}
          </div>
        </div>
      )}
      <p className="sc-library-hint" role="status">
        {hint(snapshot)}
      </p>
      {list.status === "loading" && (
        <ul className="sc-library-rows" aria-hidden="true">
          {[0, 1, 2].map((index) => (
            <li key={index} className="sc-library-row">
              <Skeleton decorative width={52} height={78} />
              <span className="sc-library-main">
                <Skeleton decorative width="60%" height={18} />
                <Skeleton decorative width="40%" height={14} />
              </span>
            </li>
          ))}
        </ul>
      )}
      {list.status === "error" && <LoadFailure failure={list.failure} onRetry={() => void client.load()} />}
      {empty && (
        <EmptyState
          title="Your library is empty"
          action={<Link className="sc-button sc-button-primary" href="/discover">Find a show</Link>}
        >
          Shows you add appear here with your progress. If you remove a show, its watch history is kept in case you add it back.
        </EmptyState>
      )}
      {list.status === "ready" && !empty && (
        <>
          {rows.length === 0 ? (
            <EmptyState
              title={noMatchText(query)}
              action={
                filtered ? (
                  <Button variant="secondary" onClick={clear}>
                    Clear search and filter
                  </Button>
                ) : undefined
              }
            >
              {filtered ? "Your search and filter are kept. Clear them to see every saved show." : null}
            </EmptyState>
          ) : (
            <ul ref={list$} className="sc-library-rows" aria-label="Saved shows">
              {rows.map((item) => (
                <LibraryRow
                  key={item.showId}
                  item={item}
                  busy={snapshot.busy.has(item.showId)}
                  onStatus={(status) => client.setStatus(item.showId, status)}
                  onRemove={() => client.remove(item.showId)}
                />
              ))}
            </ul>
          )}
          {typeof list.more === "object" ? (
            <ErrorState title="More shows didn’t load" onRetry={loadMore}>
              {trackingFailureCopy(list.more)}
            </ErrorState>
          ) : (
            list.nextCursor && (
              <div className="sc-library-more">
                <Button variant="secondary" busy={list.more === "loading"} onClick={loadMore}>
                  Show more shows
                </Button>
              </div>
            )
          )}
        </>
      )}
      <ToastViewport toasts={toasts} onDismiss={dismiss} />
    </section>
  );
}
