"use client";
import { useEffect, useState, useSyncExternalStore } from "react";
import { toEpisodeView, type EpisodeView } from "../../tracking";
import type { ApiResult, Failure } from "../../tracking/api";
import { createEpisodeApi, type EpisodeApi } from "./api";

export type EpisodeLoad =
  | { status: "loading" }
  | { status: "error"; failure: Failure }
  | { status: "ready"; showId: string; episode: EpisodeView };

export interface EpisodeSnapshot {
  load: EpisodeLoad;
  /** Announced after details were hidden by the viewer or by the reveal expiring. */
  notice: "hidden" | "expired" | null;
  /**
   * Bumped whenever protected data is evicted. Consumers that cache episode lists (the tracking
   * client behind the catch-up sheet) are keyed by it, so their caches are dropped too.
   */
  cacheEpoch: number;
}

/** Longest delay setTimeout accepts; a longer reveal is rechecked when it fires. */
const MAX_TIMER = 2 ** 31 - 1;

const redact = (episode: EpisodeView): EpisodeView =>
  episode.access === "locked" ? episode : { ...episode, access: "locked", details: null };

/**
 * One episode as the backend authorized it. Protected details exist only in the last
 * authorized read: Hide again, reveal expiry, a restored page and a 401 evict them
 * synchronously, before any request, and reads already in flight are discarded.
 */
export function createEpisodeStore(api: EpisodeApi, episodeId: string) {
  const listeners = new Set<() => void>();
  let state: EpisodeSnapshot = { load: { status: "loading" }, notice: null, cacheEpoch: 0 };
  let generation = 0;
  let expiry: ReturnType<typeof setTimeout> | undefined;
  /** Set by Hide again until the backend confirms; a revealed read is redacted meanwhile. */
  let hiding = false;

  function emit(change: Partial<EpisodeSnapshot>) {
    state = { ...state, ...change };
    for (const listener of listeners) listener();
  }
  const ready = () => (state.load.status === "ready" ? state.load : null);

  function evict(notice: EpisodeSnapshot["notice"] = state.notice) {
    generation += 1;
    const current = ready();
    emit({
      load: current ? { ...current, episode: redact(current.episode) } : state.load,
      notice,
      cacheEpoch: state.cacheEpoch + 1,
    });
  }

  function endSession() {
    clearTimeout(expiry);
    generation += 1;
    emit({ load: { status: "error", failure: { kind: "auth" } }, notice: null, cacheEpoch: state.cacheEpoch + 1 });
  }

  async function load() {
    const run = ++generation;
    const result = await api.getEpisode(episodeId);
    if (run !== generation) return result;
    if (!result.ok) {
      if (result.failure.kind === "auth") endSession();
      else if (!ready()) emit({ load: { status: "error", failure: result.failure } });
      return result;
    }
    let fresh = toEpisodeView(result.data);
    if (hiding && fresh.access === "revealed") fresh = redact(fresh);
    const known = ready()?.episode;
    // A read answered after a newer write never moves the episode back.
    if (known && fresh.revision < known.revision) return result;
    if (fresh.access !== "revealed") clearTimeout(expiry);
    emit({ load: { status: "ready", showId: result.data.showId, episode: fresh } });
    return result;
  }

  function scheduleExpiry(expiresAt: string) {
    clearTimeout(expiry);
    const delay = Date.parse(expiresAt) - Date.now();
    if (!Number.isFinite(delay)) return;
    expiry = setTimeout(() => {
      if (Date.parse(expiresAt) > Date.now()) return scheduleExpiry(expiresAt);
      if (ready()?.episode.access !== "revealed") return;
      evict("expired");
      void load();
    }, Math.min(Math.max(delay, 0), MAX_TIMER));
  }

  /**
   * Grants the session-scoped reveal, then shows details only from the authorized read.
   * `granted` tells a failed grant (nothing changed) from a failed follow-up read.
   */
  async function reveal(): Promise<{ ok: true } | { ok: false; failure: Failure; granted: boolean }> {
    const result = await api.reveal(episodeId);
    if (!result.ok) {
      if (result.failure.kind === "auth") endSession();
      return { ok: false, failure: result.failure, granted: false };
    }
    hiding = false;
    emit({ notice: null });
    scheduleExpiry(result.data.expiresAt);
    const read = await load();
    return read.ok ? { ok: true } : { ok: false, failure: read.failure, granted: true };
  }

  /** Redacts at once, then removes the grant. A failure keeps the screen redacted. */
  async function hide(): Promise<ApiResult<unknown>> {
    hiding = true;
    clearTimeout(expiry);
    evict("hidden");
    const result = await api.hide(episodeId);
    if (!result.ok) {
      if (result.failure.kind === "auth") endSession();
      return result;
    }
    hiding = false;
    void load();
    return result;
  }

  /** Progress from a successful write. Unmarking drops details unlocked by watching. */
  function applyProgress(updates: readonly { id: string; watched: boolean; revision: number }[]) {
    const current = ready();
    const update = updates.find((episode) => episode.id === episodeId);
    if (!current || !update || update.revision < current.episode.revision) return;
    const episode = { ...current.episode, watched: update.watched, revision: update.revision };
    emit({ load: { ...current, episode: !update.watched && episode.access === "watched" ? redact(episode) : episode } });
  }

  /** A page restored from the back/forward cache may belong to an ended session: redact first. */
  function revalidate(restored: boolean) {
    if (restored) evict();
    if (state.load.status !== "error" || restored) void load();
  }

  return {
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    getSnapshot: () => state,
    load,
    reveal,
    hide,
    applyProgress,
    endSession,
    revalidate,
    dispose: () => clearTimeout(expiry),
  };
}

export type EpisodeStore = ReturnType<typeof createEpisodeStore>;

export function useEpisode(episodeId: string) {
  const [store] = useState(() => createEpisodeStore(createEpisodeApi(), episodeId));
  const snapshot = useSyncExternalStore(store.subscribe, store.getSnapshot, store.getSnapshot);
  useEffect(() => {
    void store.load();
    const onShow = (event: PageTransitionEvent) => {
      if (event.persisted) store.revalidate(true);
    };
    const onVisible = () => {
      if (document.visibilityState === "visible") store.revalidate(false);
    };
    window.addEventListener("pageshow", onShow);
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      window.removeEventListener("pageshow", onShow);
      document.removeEventListener("visibilitychange", onVisible);
      store.dispose();
    };
  }, [store]);
  return { store, snapshot };
}
