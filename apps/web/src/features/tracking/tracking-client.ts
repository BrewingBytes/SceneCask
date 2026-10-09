import type { ToastControls } from "../catalog/search/use-library-actions";
import {
  actionIdOf,
  keyedUndo,
  newIdempotencyKey,
  outcomeUnknown,
  type ApiResult,
  type CatchupPreview,
  type Episode,
  type EpisodeDetails,
  type Failure,
  type LibraryStatus,
  type MutationResult,
  type Release,
  type Show,
  type TrackingApi,
} from "./api";
import { episodeCode, plural, STATUS_LABELS, trackingFailureCopy } from "./copy";

export type { ToastControls };

/** A normalized episode row. Protected details exist only as the backend returned them. */
export interface EpisodeView {
  id: string;
  season: number;
  number: number;
  release: Release;
  watched: boolean;
  revision: number;
  access: "locked" | "watched" | "revealed";
  details: EpisodeDetails | null;
}
export interface SeasonLoad {
  status: "loading" | "ready" | "error";
  episodes: EpisodeView[];
  failure?: Failure;
}
export type ShowLoad = { status: "loading" } | { status: "error"; failure: Failure } | { status: "ready"; show: Show };

/** Optimistic view state computed from in-flight actions. Never contains protected data. */
export interface Overlay {
  episodes: Record<string, boolean>;
  allUnwatched: boolean;
  status?: LibraryStatus;
  saved?: boolean;
}
export interface TrackingSnapshot {
  show: ShowLoad;
  seasons: Record<number, SeasonLoad>;
  busy: ReadonlySet<string>;
  overlay: Overlay;
}

/** One action's optimistic change. Removing it on settle rolls back only that action. */
interface Patch {
  episodes?: Record<string, boolean>;
  allUnwatched?: boolean;
  status?: LibraryStatus;
  saved?: boolean;
}

interface MutationOptions {
  /** Idempotency scope: an unknown outcome retried with the same body reuses its key. */
  scope: string;
  /** One in-flight action per busy key; a repeat activation is ignored. */
  busy: string;
  body: unknown;
  patch?: Patch;
  run: (key: string) => Promise<ApiResult<MutationResult>>;
  onSuccess: (result: MutationResult) => void;
  /** Failure toast text and Retry. Omit for actions that show failures inline (sheet, dialog). */
  failure?: { message: string; retry: () => void };
}

export type MutationOutcome = ApiResult<MutationResult> | { ok: false; failure: { kind: "busy" } };

export function toEpisodeView(episode: Episode): EpisodeView {
  const { id, season, number, release, watched, revision } = episode;
  if (episode.detailsAccess === "locked") return { id, season, number, release, watched, revision, access: "locked", details: null };
  return { id, season, number, release, watched, revision, access: episode.detailsAccess, details: episode.details };
}

/** Watched state as the viewer should see it, including in-flight optimistic changes. */
export function isWatched(overlay: Overlay, episode: EpisodeView) {
  return overlay.episodes[episode.id] ?? (overlay.allUnwatched ? false : episode.watched);
}

/**
 * Details are shown only as the backend authorized them. An optimistic mark never unlocks them;
 * an optimistic unmark hides watched-unlocked details until the authoritative refetch.
 */
export function visibleDetails(overlay: Overlay, episode: EpisodeView) {
  if (episode.access === "revealed") return episode.details;
  return episode.access === "watched" && isWatched(overlay, episode) ? episode.details : null;
}

/**
 * Shared tracking client for one show. Every write carries an Idempotency-Key: an unknown
 * outcome (network/503) retried with the same body reuses its key, so a retry is never applied
 * twice; a known rejection discards it. Each action owns its optimistic patch, so concurrent
 * actions settle independently. Responses are applied in server order (tracking revision, then
 * library revision) and episode revisions never move backwards.
 */
export function createTrackingClient(api: TrackingApi, showId: string, toasts: ToastControls) {
  const listeners = new Set<() => void>();
  const patches = new Map<number, Patch>();
  const pendingKeys = new Map<string, { key: string; body: string }>();
  const runUndo = keyedUndo(api.undo);
  const busy = new Set<string>();
  let patchId = 0;
  let generation = 0;
  let state: TrackingSnapshot = { show: { status: "loading" }, seasons: {}, busy: new Set(), overlay: { episodes: {}, allUnwatched: false } };

  function overlay(): Overlay {
    const next: Overlay = { episodes: {}, allUnwatched: false };
    for (const patch of patches.values()) {
      if (patch.allUnwatched) Object.assign(next, { allUnwatched: true, episodes: {} });
      Object.assign(next.episodes, patch.episodes);
      if (patch.status) next.status = patch.status;
      if (patch.saved !== undefined) next.saved = patch.saved;
    }
    return next;
  }
  function emit(change: Partial<Pick<TrackingSnapshot, "show" | "seasons">> = {}) {
    state = { ...state, ...change, busy: new Set(busy), overlay: overlay() };
    for (const listener of listeners) listener();
  }
  const current = () => (state.show.status === "ready" ? state.show.show : null);
  const title = () => current()?.title ?? "This show";

  function updateEpisodes(update: (episode: EpisodeView) => EpisodeView) {
    const seasons: Record<number, SeasonLoad> = {};
    for (const [season, load] of Object.entries(state.seasons)) seasons[Number(season)] = { ...load, episodes: load.episodes.map(update) };
    return seasons;
  }
  function findEpisode(id: string) {
    for (const load of Object.values(state.seasons)) {
      const found = load.episodes.find((episode) => episode.id === id);
      if (found) return found;
    }
    return null;
  }
  /** Replace a row only with an equal or newer revision, so a slow read never undoes a write. */
  function acceptEpisode(fresh: EpisodeView) {
    emit({ seasons: updateEpisodes((episode) => (episode.id === fresh.id && fresh.revision >= episode.revision ? fresh : episode)) });
  }

  async function loadShow() {
    const run = ++generation;
    const result = await api.getShow(showId);
    if (run !== generation) return;
    emit({ show: result.ok ? { status: "ready", show: result.data } : { status: "error", failure: result.failure } });
  }
  async function loadSeason(season: number, force = false) {
    const existing = state.seasons[season];
    if (!force && existing && existing.status !== "error") return;
    if (!existing || existing.status === "error") emit({ seasons: { ...state.seasons, [season]: { status: "loading", episodes: [] } } });
    const result = await api.listSeason(showId, season);
    const seasons = { ...state.seasons };
    if (result.ok) seasons[season] = { status: "ready", episodes: result.data.map(toEpisodeView) };
    else if (!force) seasons[season] = { status: "error", episodes: [], failure: result.failure };
    emit({ seasons });
  }
  /** Authoritative reload after undo or a conflict: show and every loaded season. */
  async function refresh() {
    await Promise.all([loadShow(), ...Object.keys(state.seasons).map((season) => loadSeason(Number(season), true))]);
  }
  async function refetchEpisodes(ids: readonly string[]) {
    await Promise.all(
      ids.map(async (id) => {
        const result = await api.getEpisode(id);
        if (result.ok) acceptEpisode(toEpisodeView(result.data));
      }),
    );
  }

  function applyMutation(result: MutationResult) {
    const show = current();
    if (!show) return;
    const newer =
      result.trackingRevision > show.trackingRevision ||
      (result.trackingRevision === show.trackingRevision && (show.library?.revision ?? -1) <= result.library.revision);
    const nextShow = newer ? { ...show, library: result.library, trackingRevision: result.trackingRevision } : show;
    const changed = new Map(result.episodes.map((episode) => [episode.id, episode]));
    const seasons = updateEpisodes((episode) => {
      const update = changed.get(episode.id);
      if (!update || update.revision < episode.revision) return episode;
      // Unlocked-by-watching details are dropped on unmark; a mark never fabricates details.
      const hide = !update.watched && episode.access === "watched";
      return { ...episode, watched: update.watched, revision: update.revision, ...(hide ? { access: "locked", details: null } : {}) };
    });
    emit({ show: { status: "ready", show: nextShow }, seasons });
    // Newly watched rows unlock only through an authorized fetch.
    void refetchEpisodes(result.episodes.filter((episode) => episode.watched).map((episode) => episode.id));
  }

  function idempotencyKey(scope: string, body: unknown) {
    const text = JSON.stringify(body);
    const known = pendingKeys.get(scope);
    if (known && known.body === text) return known.key;
    const key = newIdempotencyKey();
    pendingKeys.set(scope, { key, body: text });
    return key;
  }

  async function mutate(options: MutationOptions): Promise<MutationOutcome> {
    if (busy.has(options.busy)) return { ok: false, failure: { kind: "busy" } };
    busy.add(options.busy);
    const id = ++patchId;
    if (options.patch) patches.set(id, options.patch);
    emit();
    const result = await options.run(idempotencyKey(options.scope, options.body));
    if (result.ok || !outcomeUnknown(result.failure)) pendingKeys.delete(options.scope);
    busy.delete(options.busy);
    patches.delete(id);
    if (result.ok) {
      applyMutation(result.data);
      options.onSuccess(result.data);
      return result;
    }
    emit();
    if (result.failure.kind === "conflict") void refresh();
    if (options.failure) {
      toasts.show({
        id: `tracking-error-${options.busy}`,
        tone: "error",
        message: `${options.failure.message} ${trackingFailureCopy(result.failure)}`,
        onRetry: options.failure.retry,
      });
    }
    return result;
  }

  async function undo(actionId: string, toastId: string, undone: string) {
    toasts.update(toastId, { busy: "undo" });
    const result = await runUndo(actionId);
    if (result.ok) {
      const { skipped } = result.data;
      toasts.show({
        id: toastId,
        tone: "success",
        message: skipped ? `Undone. ${plural(skipped, "change")} you made afterwards ${skipped === 1 ? "was" : "were"} kept.` : undone,
      });
      await refresh();
      return;
    }
    const expired = result.failure.kind === "expired";
    toasts.show({
      id: toastId,
      tone: "error",
      message: expired ? "Undo is no longer available." : `Couldn’t undo. ${trackingFailureCopy(result.failure)}`,
      onRetry: expired ? undefined : () => void undo(actionId, toastId, undone),
    });
  }

  /** Success toast with Undo when the action changed something; a no-op gets an info note. */
  function announce(result: MutationResult, toastId: string, message: string, undone: string, noop: string) {
    const actionId = actionIdOf(result);
    if (!actionId) {
      toasts.show({ id: toastId, tone: "info", message: noop });
      return;
    }
    toasts.show({ id: toastId, tone: "success", message, onUndo: () => void undo(actionId, toastId, undone) });
  }

  function setWatched(episode: EpisodeView, watched: boolean): Promise<MutationOutcome> {
    const code = episodeCode(episode.season, episode.number);
    const wasSaved = current()?.library?.saved ?? false;
    const body = { watched, expectedRevision: episode.revision };
    return mutate({
      scope: `episode:${episode.id}`,
      busy: `episode:${episode.id}`,
      body,
      patch: { episodes: { [episode.id]: watched } },
      run: (key) => api.setProgress(episode.id, body, key),
      onSuccess: (result) =>
        announce(
          result,
          `tracking-episode-${episode.id}`,
          watched ? `Marked ${code} watched${wasSaved || episode.season === 0 ? "." : " and added the show to your library."}` : `Marked ${code} unwatched.`,
          watched ? `${code} is unwatched again.` : `${code} is watched again.`,
          watched ? `${code} was already watched.` : `${code} was already unwatched.`,
        ),
      failure: {
        message: watched ? `Couldn’t mark ${code} watched.` : `Couldn’t mark ${code} unwatched.`,
        retry: () => void retryEpisode(episode.id, watched),
      },
    });
  }
  /** Retry from the latest row so the body (and therefore the reused key) matches. */
  async function retryEpisode(id: string, watched: boolean) {
    const episode = findEpisode(id) ?? (await fetchEpisode(id));
    if (episode) await setWatched(episode, watched);
  }
  async function fetchEpisode(id: string) {
    const result = await api.getEpisode(id);
    if (!result.ok) {
      toasts.show({ id: `tracking-error-episode:${id}`, tone: "error", message: `Couldn’t load that episode. ${trackingFailureCopy(result.failure)}`, onRetry: () => void markWatchedById(id) });
      return null;
    }
    return toEpisodeView(result.data);
  }
  /** Mark one episode (for example "next") even when its season is not loaded yet. */
  async function markWatchedById(id: string) {
    const episode = findEpisode(id) ?? (await fetchEpisode(id));
    if (episode) await setWatched(episode, true);
  }

  function saveLibrary(saved: boolean, status: LibraryStatus | undefined, inline: boolean): Promise<MutationOutcome> {
    const show = current();
    if (!show) return Promise.resolve({ ok: false, failure: { kind: "busy" } });
    const hadHistory = (show.library?.progress.watched ?? 0) > 0;
    const body = { saved, expectedRevision: show.library?.revision ?? 0, ...(status ? { status } : {}) };
    return mutate({
      scope: "library",
      busy: "library",
      body,
      patch: { saved },
      run: (key) => api.saveToLibrary(showId, body, key),
      onSuccess: (result) =>
        announce(
          result,
          "tracking-library",
          saved
            ? hadHistory
              ? `${show.title} is back in your library with your progress.`
              : `Added ${show.title} to ${STATUS_LABELS[result.library.status]}.`
            : `Removed ${show.title}. Your watch history is kept.`,
          saved ? `Removed ${show.title} from your library.` : `${show.title} is back in your library.`,
          saved ? `${show.title} is already in your library.` : `${show.title} was already removed.`,
        ),
      failure: inline
        ? undefined
        : { message: saved ? `Couldn’t add ${show.title}.` : `Couldn’t remove ${show.title}.`, retry: () => void saveLibrary(saved, status, inline) },
    });
  }

  function setStatus(status: LibraryStatus): Promise<MutationOutcome> {
    const library = current()?.library;
    if (!library?.saved || library.status === status) return Promise.resolve({ ok: false, failure: { kind: "busy" } });
    const body = { status, expectedRevision: library.revision };
    return mutate({
      scope: "library",
      busy: "library",
      body,
      patch: { status },
      run: (key) => api.setStatus(showId, body, key),
      onSuccess: (result) =>
        announce(
          result,
          "tracking-library",
          `${title()} is now ${STATUS_LABELS[status]}.`,
          `${title()} is back to ${STATUS_LABELS[library.status]}.`,
          `${title()} is already ${STATUS_LABELS[status]}.`,
        ),
      failure: { message: "Couldn’t change the status.", retry: () => void setStatus(status) },
    });
  }

  /** Commits exactly the previewed set. Stale/expired previews are returned for re-preview. */
  function commitCatchup(preview: CatchupPreview): Promise<MutationOutcome> {
    const through = episodeCode(preview.through.season, preview.through.episode);
    return mutate({
      scope: `catchup:${preview.previewId}`,
      busy: "catchup",
      body: { previewId: preview.previewId },
      patch: { episodes: Object.fromEntries(preview.included.map((episode) => [episode.id, true])) },
      run: (key) => api.commitCatchup(showId, preview.previewId, key),
      onSuccess: (result) =>
        announce(
          result,
          "tracking-catchup",
          `Marked ${plural(result.changed, "episode")} watched through ${through}.`,
          "Undone. Episodes you’d already watched are still marked.",
          `Everything released through ${through} was already marked.`,
        ),
    });
  }

  function eraseHistory(): Promise<MutationOutcome> {
    const show = current();
    if (!show) return Promise.resolve({ ok: false, failure: { kind: "busy" } });
    const body = { expectedRevision: show.trackingRevision };
    return mutate({
      scope: "history",
      busy: "history",
      body,
      patch: { allUnwatched: true },
      run: (key) => api.eraseHistory(showId, body.expectedRevision, key),
      onSuccess: (result) =>
        announce(
          result,
          "tracking-history",
          `Erased watch history for ${show.title}.`,
          `Your watch history for ${show.title} is back.`,
          `${show.title} had no watch history to erase.`,
        ),
    });
  }

  return {
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    getSnapshot: () => state,
    loadShow,
    loadSeason,
    refresh,
    setWatched,
    markWatchedById,
    setStatus,
    addToLibrary: (inline = false) => saveLibrary(true, current()?.library ? undefined : "plan_to_watch", inline),
    removeFromLibrary: () => saveLibrary(false, undefined, false),
    previewCatchup: (throughEpisodeId: string) => api.previewCatchup(showId, throughEpisodeId),
    commitCatchup,
    eraseHistory,
  };
}

export type TrackingClient = ReturnType<typeof createTrackingClient>;
