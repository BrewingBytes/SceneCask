"use client";
import { useCallback, useEffect, useRef, useState } from "react";
import type { ApiResult, DiscoverApi, Failure, SearchResults, SearchShow } from "./api";

/** C05: q is trimmed and must be 2–100 characters. */
export const MIN_QUERY = 2;
export const MAX_QUERY = 100;
export const SEARCH_DEBOUNCE_MS = 300;
const MAX_PAGE = 500;

export type SearchView =
  | { status: "idle"; reason: "empty" | "short" | "long" }
  | { status: "loading"; term: string }
  | { status: "error"; term: string; failure: Failure; retry: () => void }
  | { status: "empty"; term: string }
  | {
      status: "results";
      term: string;
      items: SearchShow[];
      hasMore: boolean;
      more: "idle" | "loading" | Failure;
      loadMore: () => void;
    };

interface Settled {
  /** Term and retry attempt the response belongs to; any other key is stale. */
  key: string;
  outcome: ApiResult<SearchResults>;
  items: SearchShow[];
  page: number;
  totalPages: number;
  more: "idle" | "loading" | Failure;
}

const countChars = (text: string) => Array.from(text).length;

function append(items: SearchShow[], next: SearchShow[]) {
  const seen = new Set(items.map((item) => item.providerId));
  return [...items, ...next.filter((item) => !seen.has(item.providerId))];
}

/**
 * Debounced TV search. Each term change aborts the earlier request, and a response only
 * settles when its key still matches the current term, so a slow earlier query can never
 * overwrite a later one. Pagination appends under the same guard.
 */
export function useShowSearch(api: DiscoverApi, query: string): SearchView {
  const term = query.trim();
  const length = countChars(term);
  const valid = length >= MIN_QUERY && length <= MAX_QUERY;
  const [attempt, setAttempt] = useState(0);
  const [settled, setSettled] = useState<Settled | null>(null);
  const key = `${attempt}\u0000${term}`;
  const moreRequest = useRef<AbortController | null>(null);

  useEffect(() => {
    if (!valid) return;
    const controller = new AbortController();
    let live = true;
    const timer = setTimeout(async () => {
      const outcome = await api.search(term, 1, controller.signal);
      if (!live) return;
      setSettled(
        outcome.ok
          ? { key, outcome, items: outcome.data.items, page: outcome.data.page, totalPages: outcome.data.totalPages, more: "idle" }
          : { key, outcome, items: [], page: 0, totalPages: 0, more: "idle" },
      );
    }, SEARCH_DEBOUNCE_MS);
    return () => {
      live = false;
      clearTimeout(timer);
      controller.abort();
      moreRequest.current?.abort();
    };
  }, [api, key, term, valid]);

  const retry = useCallback(() => setAttempt((count) => count + 1), []);

  const loadMore = useCallback(() => {
    if (!settled || settled.key !== key || settled.more === "loading") return;
    const page = settled.page + 1;
    const controller = new AbortController();
    moreRequest.current?.abort();
    moreRequest.current = controller;
    setSettled((current) => (current?.key === key ? { ...current, more: "loading" } : current));
    void api.search(term, page, controller.signal).then((outcome) => {
      if (controller.signal.aborted) return;
      setSettled((current) => {
        if (current?.key !== key) return current;
        if (!outcome.ok) return { ...current, more: outcome.failure };
        return {
          ...current,
          items: append(current.items, outcome.data.items),
          page: outcome.data.page,
          totalPages: outcome.data.totalPages,
          more: "idle",
        };
      });
    });
  }, [api, key, settled, term]);

  if (!valid) return { status: "idle", reason: length === 0 ? "empty" : length < MIN_QUERY ? "short" : "long" };
  if (!settled || settled.key !== key) return { status: "loading", term };
  const { outcome } = settled;
  if (!outcome.ok) return { status: "error", term, failure: outcome.failure, retry };
  if (settled.items.length === 0) return { status: "empty", term };
  return {
    status: "results",
    term,
    items: settled.items,
    hasMore: settled.page < Math.min(settled.totalPages, MAX_PAGE),
    more: settled.more,
    loadMore,
  };
}
