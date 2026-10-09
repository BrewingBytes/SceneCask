"use client";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { Button, EmptyState, ErrorState, Skeleton, TextField } from "../../../components/ui/basic";
import { ToastViewport, useToastQueue } from "../../../components/ui/overlays";
import { createDiscoverApi } from "./api";
import { failureCopy, resultsHint } from "./copy";
import { SearchResultRow } from "./search-result-row";
import { useLibraryActions } from "./use-library-actions";
import { MAX_QUERY, SEARCH_DEBOUNCE_MS, useShowSearch, type SearchView } from "./use-show-search";
import "./discover.css";

export interface DiscoverSearchProps {
  /** Query restored from the URL (?q=) on load, reload and Back/Forward. */
  initialQuery?: string;
  /** Beta social discovery (R28) renders here while the query is empty; alpha passes nothing. */
  recommendations?: ReactNode;
}

function hint(view: SearchView) {
  switch (view.status) {
    case "idle":
      return view.reason === "empty"
        ? "Search by title. Results show year and poster so you can pick the right one."
        : view.reason === "short"
          ? "Type at least 2 characters."
          : `Use up to ${MAX_QUERY} characters.`;
    case "loading":
      return "Searching…";
    case "empty":
      return "No shows found.";
    case "results":
      return resultsHint(view.items.length, view.hasMore);
    default:
      return "";
  }
}

/** D02 Discover: TV search, disambiguation, open-by-import and Add to Plan to watch. */
export function DiscoverSearch({ initialQuery = "", recommendations }: DiscoverSearchProps) {
  const [api] = useState(createDiscoverApi);
  const [query, setQuery] = useState(initialQuery);
  const view = useShowSearch(api, query);
  const router = useRouter();
  const navigate = useCallback((href: string) => router.push(href), [router]);
  const { toasts, show, update, dismiss } = useToastQueue();
  const { rows, open, add } = useLibraryActions(api, { show, update, dismiss }, navigate);
  const list = useRef<HTMLUListElement>(null);
  const focusFrom = useRef<number | null>(null);
  const loaded = view.status === "results" ? view.items.length : 0;

  // Keep the settled query in the URL without adding a history entry per keystroke.
  useEffect(() => {
    const timer = setTimeout(() => {
      const url = new URL(window.location.href);
      const term = query.trim();
      if (term) url.searchParams.set("q", term);
      else url.searchParams.delete("q");
      if (url.href !== window.location.href) window.history.replaceState(null, "", url);
    }, SEARCH_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [query]);

  // After "Show more", focus the first appended result instead of losing focus with the button.
  useEffect(() => {
    if (focusFrom.current === null || loaded <= focusFrom.current) return;
    list.current?.querySelectorAll<HTMLElement>("[data-search-open]")[focusFrom.current]?.focus();
    focusFrom.current = null;
  }, [loaded]);

  return (
    <section className="sc-discover" aria-labelledby="sc-discover-title">
      <h1 id="sc-discover-title">Discover</h1>
      <form role="search" className="sc-discover-form" onSubmit={(event) => event.preventDefault()}>
        <TextField
          type="search"
          label="Search TV shows"
          placeholder="Search TV shows by title"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          maxLength={MAX_QUERY}
          autoComplete="off"
          spellCheck={false}
          enterKeyHint="search"
        />
      </form>
      <p className="sc-discover-hint" role="status">
        {hint(view)}
      </p>
      {view.status === "loading" && (
        <ul className="sc-search-results" aria-hidden="true">
          {[0, 1, 2].map((index) => (
            <li key={index} className="sc-search-result">
              <Skeleton decorative width={60} height={90} />
              <span className="sc-search-skeleton-text">
                <Skeleton decorative width="60%" height={18} />
                <Skeleton decorative width="35%" height={14} />
              </span>
            </li>
          ))}
        </ul>
      )}
      {view.status === "error" &&
        (view.failure.kind === "auth" ? (
          <EmptyState
            title="Sign in to search shows"
            action={<Link className="sc-button sc-button-primary" href="/auth/signin">Sign in</Link>}
          >
            Your session has ended. Your search is kept here.
          </EmptyState>
        ) : view.failure.kind === "unverified" ? (
          <EmptyState
            title="Verify your email to search"
            action={<Link className="sc-button sc-button-primary" href="/auth/verify">Verify email</Link>}
          >
            Check your inbox for the verification link, then search again.
          </EmptyState>
        ) : (
          <ErrorState title="Show search isn’t responding" onRetry={view.retry}>
            {failureCopy(view.failure)}
          </ErrorState>
        ))}
      {view.status === "empty" && (
        <EmptyState title={`No shows match “${view.term}”`}>Check the spelling, or try the original title.</EmptyState>
      )}
      {view.status === "results" && (
        <>
          <ul ref={list} className="sc-search-results" aria-label={`Search results for ${view.term}`}>
            {view.items.map((item) => (
              <SearchResultRow
                key={item.providerId}
                show={item}
                state={rows[item.providerId]}
                onOpen={() => void open(item)}
                onAdd={() => void add(item)}
              />
            ))}
          </ul>
          {typeof view.more === "object" ? (
            <ErrorState
              title="More results didn’t load"
              onRetry={() => {
                focusFrom.current = loaded;
                view.loadMore();
              }}
            >
              {failureCopy(view.more)}
            </ErrorState>
          ) : (
            view.hasMore && (
              <div className="sc-discover-more">
                <Button
                  variant="secondary"
                  busy={view.more === "loading"}
                  onClick={() => {
                    focusFrom.current = loaded;
                    view.loadMore();
                  }}
                >
                  Show more results
                </Button>
              </div>
            )
          )}
        </>
      )}
      {view.status === "idle" && view.reason === "empty" && (
        <>
          <EmptyState title="Start with a show you’re watching">
            Search for any TV show and add it to your library. You don’t need friends on SceneCask to track your
            episodes.
          </EmptyState>
          {recommendations}
        </>
      )}
      <ToastViewport toasts={toasts} onDismiss={dismiss} />
    </section>
  );
}
