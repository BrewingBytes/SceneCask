import type { ToastControls } from "../catalog/search/use-library-actions";
import { STATUS_LABELS, trackingFailureCopy } from "../tracking/copy";
import { createIdempotencyKeys, createToastUndo } from "../tracking/mutation-support";
import {
  actionIdOf,
  outcomeUnknown,
  type ApiResult,
  type Failure,
  type LibraryApi,
  type LibraryCounts,
  type LibraryItem,
  type LibraryQuery,
  type LibraryStatus,
  type MutationResult,
} from "./api";

export type { ToastControls };

/** Debounce for the title search, matching Discover. */
export const LIBRARY_SEARCH_DEBOUNCE_MS = 300;
export const MAX_LIBRARY_QUERY = 100;

export type ListState =
  | { status: "loading" }
  | { status: "error"; failure: Failure }
  | { status: "ready"; nextCursor: string | null; more: "idle" | "loading" | Failure };

export interface LibrarySnapshot {
  /** The controls' current values; `q` is untrimmed while typing. */
  query: LibraryQuery;
  list: ListState;
  /** Saved rows matching the status filter, with in-flight optimistic changes applied. */
  rows: LibraryItem[];
  /** Server counts adjusted for loaded rows' optimistic and settled changes; null if absent. */
  counts: LibraryCounts | null;
  busy: ReadonlySet<string>;
}

/** One action's optimistic change. Removing it on settle rolls back only that action. */
interface Patch {
  showId: string;
  status?: LibraryStatus;
  saved?: boolean;
}
type Entry = Pick<LibraryItem, "saved" | "status">;

export const sameQuery = (a: LibraryQuery, b: LibraryQuery) => a.status === b.status && a.q.trim() === b.q.trim();

function tally(counts: LibraryCounts, entry: Entry, sign: 1 | -1) {
  if (!entry.saved) return;
  counts.all += sign;
  counts[entry.status] += sign;
}

/**
 * Library list and row mutations. Each write owns a patch keyed by action, so a failure
 * removes only its own change and never restores a snapshot over a later edit. Responses and
 * recovery reads replace a row only with an equal or newer library revision. Every write sends
 * an Idempotency-Key; after an unknown outcome the same body reuses it.
 */
export function createLibraryClient(api: LibraryApi, toasts: ToastControls, initialQuery: LibraryQuery) {
  const listeners = new Set<() => void>();
  /** Latest authoritative entry per show, whether or not it is still saved. */
  const items = new Map<string, LibraryItem>();
  /** Each loaded row's state as the server counts included it. */
  const baseline = new Map<string, Entry>();
  const patches = new Map<number, Patch>();
  const keys = createIdempotencyKeys();
  const busy = new Set<string>();
  const undo = createToastUndo(api.undo, toasts, (result) => {
    accept(result.library);
    emit();
  });
  let order: string[] = [];
  let serverCounts: LibraryCounts | null = null;
  let query = initialQuery;
  let list: ListState = { status: "loading" };
  let patchId = 0;
  let generation = 0;
  let controller: AbortController | null = null;
  let debounce: ReturnType<typeof setTimeout> | undefined;
  let state = snapshot();

  function effective(item: LibraryItem): LibraryItem {
    let next = item;
    for (const patch of patches.values()) {
      if (patch.showId !== item.showId) continue;
      if (patch.status) next = { ...next, status: patch.status };
      if (patch.saved !== undefined) next = { ...next, saved: patch.saved };
    }
    return next;
  }
  function snapshot(): LibrarySnapshot {
    const current = order.map((id) => effective(items.get(id)!));
    let counts: LibraryCounts | null = null;
    if (serverCounts) {
      counts = { ...serverCounts };
      for (const [id, entry] of baseline) {
        tally(counts, entry, -1);
        tally(counts, effective(items.get(id)!), 1);
      }
    }
    return {
      query,
      list,
      rows: current.filter((item) => item.saved && (!query.status || item.status === query.status)),
      counts,
      busy: new Set(busy),
    };
  }
  function emit() {
    state = snapshot();
    for (const listener of listeners) listener();
  }
  /** Replace a row only with an equal or newer revision, so a slow read never undoes a write. */
  function accept(fresh: LibraryItem) {
    const known = items.get(fresh.showId);
    if (!known || fresh.revision >= known.revision) items.set(fresh.showId, fresh);
    // A row outside the loaded pages is not in the counts' baseline; reload to keep counts true.
    if (!baseline.has(fresh.showId)) void load();
  }

  async function load(append = false) {
    if (append && (list.status !== "ready" || !list.nextCursor || list.more === "loading")) return;
    const cursor = append && list.status === "ready" ? (list.nextCursor ?? undefined) : undefined;
    if (!append) {
      controller?.abort();
      generation += 1;
      list = { status: "loading" };
    } else if (list.status === "ready") list = { ...list, more: "loading" };
    emit();
    const run = generation;
    controller = new AbortController();
    const requested = { status: query.status, q: query.q.trim() };
    const result = await api.list(requested, cursor, controller.signal);
    // Only the latest query (and the page it started from) may settle.
    if (run !== generation) return;
    if (!result.ok) {
      if (result.failure.kind === "aborted") return;
      list = append && list.status === "ready" ? { ...list, more: result.failure } : { status: "error", failure: result.failure };
      emit();
      return;
    }
    if (!append) {
      order = [];
      baseline.clear();
      serverCounts = result.data.counts ?? null;
    }
    for (const fresh of result.data.items) {
      const known = items.get(fresh.showId);
      // Keep a row a concurrent write already moved past this read; counts use the read's state.
      if (!known || fresh.revision >= known.revision) items.set(fresh.showId, fresh);
      baseline.set(fresh.showId, { saved: fresh.saved, status: fresh.status });
      if (!order.includes(fresh.showId)) order.push(fresh.showId);
    }
    list = { status: "ready", nextCursor: result.data.nextCursor, more: "idle" };
    emit();
  }

  /** Controls update at once; a title search waits for typing to settle. */
  function setQuery(next: Partial<LibraryQuery>) {
    const previous = query;
    query = { ...query, ...next };
    clearTimeout(debounce);
    if (sameQuery(previous, query)) return emit();
    if (previous.status !== query.status) return void load();
    emit();
    debounce = setTimeout(() => void load(), LIBRARY_SEARCH_DEBOUNCE_MS);
  }

  /** Authoritative state for one row after a rejected write; later edits keep their revisions. */
  async function recover(showId: string) {
    const result = await api.getShow(showId);
    if (result.ok && result.data.library) {
      accept(result.data.library);
      emit();
    } else if (result.ok || result.failure.kind === "not_found") void load();
  }

  interface Mutation {
    patch: Omit<Patch, "showId">;
    body: unknown;
    run: (key: string) => Promise<ApiResult<MutationResult>>;
    message: string;
    undone: string;
    noop: string;
    failure: string;
    retry: () => void;
  }

  async function mutate(showId: string, mutation: Mutation) {
    const scope = `library:${showId}`;
    busy.add(showId);
    const id = ++patchId;
    patches.set(id, { showId, ...mutation.patch });
    toasts.dismiss(`library-error-${showId}`);
    emit();
    const result = await mutation.run(keys.keyFor(scope, mutation.body));
    keys.settle(scope, result);
    busy.delete(showId);
    patches.delete(id);
    if (result.ok) {
      accept(result.data.library);
      emit();
      const actionId = actionIdOf(result.data);
      const toastId = `library-${showId}`;
      if (!actionId) toasts.show({ id: toastId, tone: "info", message: mutation.noop });
      else toasts.show({ id: toastId, tone: "success", message: mutation.message, onUndo: () => void undo(actionId, toastId, mutation.undone) });
      return;
    }
    emit();
    // A known rejection means the row may be out of date: fetch that row alone.
    if (!outcomeUnknown(result.failure) && result.failure.kind !== "auth" && result.failure.kind !== "rate_limited") void recover(showId);
    toasts.show({
      id: `library-error-${showId}`,
      tone: "error",
      message: `${mutation.failure} ${trackingFailureCopy(result.failure)}`,
      onRetry: mutation.retry,
    });
  }

  /** Retries rebuild the body from the latest entry, so an unknown outcome reuses its key. */
  function setStatus(showId: string, status: LibraryStatus) {
    const item = items.get(showId);
    if (!item?.saved || busy.has(showId) || item.status === status) return;
    const title = item.title;
    const body = { status, expectedRevision: item.revision };
    void mutate(showId, {
      patch: { status },
      body,
      run: (key) => api.setStatus(showId, body, key),
      message: `${title} is now ${STATUS_LABELS[status]}.`,
      undone: `${title} is back to ${STATUS_LABELS[item.status]}.`,
      noop: `${title} is already ${STATUS_LABELS[status]}.`,
      failure: `Couldn’t move ${title} to ${STATUS_LABELS[status]}.`,
      retry: () => setStatus(showId, status),
    });
  }

  function remove(showId: string) {
    const item = items.get(showId);
    if (!item?.saved || busy.has(showId)) return;
    const title = item.title;
    const body = { saved: false, expectedRevision: item.revision };
    void mutate(showId, {
      patch: { saved: false },
      body,
      run: (key) => api.remove(showId, body.expectedRevision, key),
      message: `Removed ${title}. Your watch history is kept.`,
      undone: `${title} is back in your library.`,
      noop: `${title} was already removed.`,
      failure: `Couldn’t remove ${title}.`,
      retry: () => remove(showId),
    });
  }

  return {
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    getSnapshot: () => state,
    load: () => load(),
    loadMore: () => load(true),
    setQuery,
    setStatus,
    remove,
    dispose() {
      clearTimeout(debounce);
      controller?.abort();
    },
  };
}

export type LibraryClient = ReturnType<typeof createLibraryClient>;
