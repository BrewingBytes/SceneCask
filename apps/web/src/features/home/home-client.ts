import { trackingFailureCopy } from "../tracking/copy";
import { createTrackingClient, type ShowLoad, type ToastControls, type TrackingClient } from "../tracking/tracking-client";
import type { Episode, Failure, Home, HomeApi, LibraryItem } from "./api";
import { nextCode } from "./copy";

export type { ToastControls };

export type HomeLoad = { status: "loading" } | { status: "error"; failure: Failure } | { status: "ready"; home: Home };

export interface HomeSnapshot {
  load: HomeLoad;
  /** Shows with a mark or catch-up in flight. */
  busy: ReadonlySet<string>;
  /** Listings of each caught-up show's latest season; null while unknown or unavailable. */
  upcoming: Readonly<Record<string, readonly Episode[] | null>>;
}

interface Tracker {
  client: TrackingClient;
  shown: ShowLoad;
}

/** Listings change when progress or releases do; a key per state keeps notes current. */
const upcomingKey = (item: LibraryItem) =>
  `${item.showId}:${item.progress.state}:${item.progress.watched}:${item.progress.total}:${item.progress.releaseInfoIncomplete}`;

/**
 * Home sections exactly as `GET /home` returns them: the first Up next item is featured, the
 * rest follow, and caught-up shows list what they are waiting for. Marks, catch-up and Undo use
 * the shared per-show tracking client (Idempotency-Keys, toasts with Undo); whenever one of them
 * changes a show, Home refetches the authoritative sections instead of guessing where the show
 * moves. Only the latest Home read may settle.
 */
export function createHomeClient(api: HomeApi, toasts: ToastControls) {
  const listeners = new Set<() => void>();
  const trackers = new Map<string, Tracker>();
  const pending = new Set<string>();
  const upcoming = new Map<string, readonly Episode[] | null>();
  let load: HomeLoad = { status: "loading" };
  let generation = 0;
  let controller: AbortController | null = null;
  let state = snapshot();

  function snapshot(): HomeSnapshot {
    const busy = new Set(pending);
    for (const [showId, tracker] of trackers) if (tracker.client.getSnapshot().busy.size > 0) busy.add(showId);
    const notes: Record<string, readonly Episode[] | null> = {};
    if (load.status === "ready") for (const item of load.home.caughtUp) notes[item.showId] = upcoming.get(upcomingKey(item)) ?? null;
    return { load, busy, upcoming: notes };
  }
  function emit() {
    state = snapshot();
    for (const listener of listeners) listener();
  }

  /** One tracking client per show, created on first use. A changed show means Home is stale. */
  function tracker(showId: string): Tracker {
    const known = trackers.get(showId);
    if (known) return known;
    const created: Tracker = { client: createTrackingClient(api.tracking, showId, toasts), shown: { status: "loading" } };
    created.client.subscribe(() => {
      const { show } = created.client.getSnapshot();
      const changed = show !== created.shown && created.shown.status === "ready";
      created.shown = show;
      if (changed) void reload();
      else emit();
    });
    trackers.set(showId, created);
    return created;
  }

  async function reload() {
    controller?.abort();
    const run = ++generation;
    controller = new AbortController();
    // A refetch keeps the sections on screen; only the first read shows the loading state.
    if (load.status !== "ready") {
      load = { status: "loading" };
      emit();
    }
    const result = await api.get(controller.signal);
    if (run !== generation) return;
    if (result.ok) {
      load = { status: "ready", home: result.data };
      emit();
      for (const item of result.data.caughtUp) void fetchUpcoming(item);
      return;
    }
    if (result.failure.kind === "aborted") return;
    // A failed background refetch keeps the last authoritative sections, unless the session ended.
    const signedOut = result.failure.kind === "auth" || result.failure.kind === "unverified";
    if (load.status !== "ready" || signedOut) load = { status: "error", failure: result.failure };
    emit();
  }

  async function fetchUpcoming(item: LibraryItem) {
    const key = upcomingKey(item);
    if (upcoming.has(key)) return;
    upcoming.set(key, null);
    const show = await api.tracking.getShow(item.showId);
    const latest = show.ok ? Math.max(0, ...show.data.seasons.filter((season) => season.number > 0 && season.count > 0).map((season) => season.number)) : 0;
    if (!latest) return;
    const episodes = await api.tracking.listSeason(item.showId, latest);
    if (!episodes.ok) return;
    upcoming.set(key, episodes.data);
    emit();
  }

  /** The show must be loaded first so the tracking client knows it is already saved. */
  async function ready(showId: string, failure: string, retry?: () => void) {
    const { client } = tracker(showId);
    if (client.getSnapshot().show.status !== "ready") await client.loadShow();
    const { show } = client.getSnapshot();
    if (show.status === "ready") return client;
    toasts.show({
      id: `home-error-${showId}`,
      tone: "error",
      message: `${failure} ${show.status === "error" ? trackingFailureCopy(show.failure) : ""}`.trim(),
      onRetry: retry,
    });
    return null;
  }

  /** Marks the show's next episode watched; the toast offers Undo. */
  async function markNext(showId: string) {
    if (load.status !== "ready" || state.busy.has(showId)) return;
    const item = load.home.upNext.find((entry) => entry.showId === showId);
    const next = item?.progress.nextEpisode;
    if (!item || !next) return;
    pending.add(showId);
    toasts.dismiss(`home-error-${showId}`);
    emit();
    const client = await ready(showId, `Couldn’t mark ${nextCode(item)} watched.`, () => void markNext(showId));
    if (client) await client.markWatchedById(next.id);
    pending.delete(showId);
    emit();
  }

  /** The tracking client for a show's catch-up sheet, loaded; null when the show didn't load. */
  async function catchupClient(showId: string) {
    if (state.busy.has(showId)) return null;
    pending.add(showId);
    emit();
    // No Retry here: reopening is the person's choice, so they choose catch-up again.
    const client = await ready(showId, "Couldn’t open catch-up.");
    pending.delete(showId);
    emit();
    return client;
  }

  return {
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    getSnapshot: () => state,
    load: reload,
    markNext,
    catchupClient,
    dispose() {
      controller?.abort();
    },
  };
}

export type HomeClient = ReturnType<typeof createHomeClient>;
