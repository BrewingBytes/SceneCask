import type { Request, Route } from "@playwright/test";

/**
 * Shared plumbing for the stateful C05 fakes in component suites. Test-only: imported by
 * Playwright specs, never by application code.
 */
export const CSRF = "csrf-fixture-token";
export const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

export interface Logged {
  method: string;
  path: string;
  query: Record<string, string>;
  body: unknown;
  key: string | null;
  csrf: string | null;
}

export interface FakeBackend {
  log: Logged[];
  idempotent: Map<string, { body: string; response: unknown; status: number }>;
  /** Body for a GET, or null for 404. */
  read(path: string, url: URL): unknown;
  /** Status and body for a write; a non-2xx body is the error code. */
  mutate(method: string, path: string, body: Record<string, unknown> | undefined): [number, unknown];
}

export const json = (route: Route, body: unknown, status = 200) =>
  route.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) });
export const error = (route: Route, status: number, code: string) =>
  json(route, { error: { code, message: "Safe server message.", requestId: "60000000-0000-4000-8000-000000000001" } }, status);
export const later = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/**
 * Answers /session with the CSRF token and logs every other call. The same Idempotency-Key
 * and body replays the stored response; a changed body is a conflict.
 */
export function serve(fake: FakeBackend, route: Route, request: Request) {
  const url = new URL(request.url());
  const path = url.pathname.replace("/api/v1", "");
  const method = request.method();
  const body = request.postData() ? request.postDataJSON() : undefined;
  const key = request.headers()["idempotency-key"] ?? null;
  if (path === "/session") return json(route, { user: null, csrfToken: CSRF });
  fake.log.push({ method, path, query: Object.fromEntries(url.searchParams), body, key, csrf: request.headers()["x-csrf-token"] ?? null });
  if (method === "GET") {
    const found = fake.read(path, url);
    return found === null ? error(route, 404, "NOT_FOUND") : json(route, found);
  }
  if (key) {
    const stored = fake.idempotent.get(key);
    if (stored) return stored.body === JSON.stringify(body) ? json(route, stored.response, stored.status) : error(route, 409, "REVISION_CONFLICT");
  }
  const [status, response] = fake.mutate(method, path, body);
  if (key && status < 300) fake.idempotent.set(key, { body: JSON.stringify(body), response, status });
  return status < 300 ? json(route, response, status) : error(route, status, response as string);
}

export const FIXTURE_SHOW = "10000000-0000-4000-8000-000000000001";
/** Prefix of every protected episode title the fakes return; suites assert where it may appear. */
export const SECRET = "Secret title";

export type ReleaseState = "released" | "future" | "unknown";
export interface FakeEpisode {
  id: string;
  season: number;
  number: number;
  state: ReleaseState;
  date: string | null;
}
export const episodeId = (season: number, number: number) =>
  `30000000-0000-4000-8000-0000000${String(season).padStart(2, "0")}${String(number).padStart(3, "0")}`;
export const ep = (season: number, number: number, state: ReleaseState = "released", date: string | null = "2026-01-01"): FakeEpisode => ({
  id: episodeId(season, number),
  season,
  number,
  state,
  date: state === "unknown" ? null : date,
});
export const code = (episode: FakeEpisode) => (episode.season === 0 ? `Special ${episode.number}` : `S${episode.season} E${episode.number}`);

export interface FakeLibrary {
  saved: boolean;
  status: "plan_to_watch" | "watching" | "on_hold" | "dropped";
  revision: number;
}
interface Change {
  id: string;
  before: boolean;
  after: number;
}

/**
 * A small in-memory C05 backend for one show: per-episode revisions, aggregate tracking revision,
 * library auto-add, catch-up previews, history erase and undo. Suites extend it for their own
 * endpoints (for example C07 reveals).
 */
export class FakeTrackingApi implements FakeBackend {
  progress = new Map<string, { watched: boolean; revision: number }>();
  tracking = 0;
  actions = new Map<string, { changes: Change[]; library: { before: FakeLibrary | null; after: number } | null; result?: unknown }>();
  previews = new Map<string, { included: string[]; tracking: number }>();
  idempotent = new Map<string, { body: string; response: unknown; status: number }>();
  log: Logged[] = [];
  readonly byId: Map<string, FakeEpisode>;
  readonly regular: FakeEpisode[];
  private sequence = 0;

  constructor(
    readonly episodes: FakeEpisode[],
    watched: string[] = [],
    public library: FakeLibrary | null = null,
    /** Show identity, so one suite can serve several shows (Home); defaults to the fixture show. */
    readonly show: { id: string; title: string } = { id: FIXTURE_SHOW, title: "Hollow Orchard" },
  ) {
    this.byId = new Map(episodes.map((episode) => [episode.id, episode]));
    this.regular = episodes.filter((episode) => episode.season > 0);
    for (const id of watched) this.progress.set(id, { watched: true, revision: 1 });
    this.tracking = watched.length;
  }
  uuid(prefix: string) {
    this.sequence += 1;
    return `${prefix}-0000-4000-8000-${String(this.sequence).padStart(12, "0")}`;
  }
  watched(id: string) {
    return this.progress.get(id)?.watched ?? false;
  }
  revision(id: string) {
    return this.progress.get(id)?.revision ?? 0;
  }
  /** Details access before protected fields are added; extended by reveal-aware suites. */
  access(episode: FakeEpisode): "locked" | "watched" | "revealed" {
    return this.watched(episode.id) ? "watched" : "locked";
  }
  /** Still URL inside unlocked details; none by default. */
  stillUrl: (episode: FakeEpisode) => string | null = () => null;
  episodeDto(episode: FakeEpisode) {
    const base = {
      id: episode.id,
      showId: this.show.id,
      season: episode.season,
      number: episode.number,
      release: { state: episode.state, date: episode.date, timezone: episode.date ? "UTC" : null, estimated: episode.state === "future" },
      watched: this.watched(episode.id),
      revision: this.revision(episode.id),
    };
    const access = this.access(episode);
    if (access === "locked") return { ...base, detailsAccess: access };
    return {
      ...base,
      detailsAccess: access,
      details: { title: `${SECRET} ${code(episode)}`, overview: `Protected overview ${code(episode)}.`, stillUrl: this.stillUrl(episode) },
    };
  }
  libraryItem() {
    const { regular } = this;
    const marked = regular.filter((episode) => this.watched(episode.id));
    const next = regular.find((episode) => episode.state === "released" && !this.watched(episode.id));
    const firstGap = regular.findIndex((episode) => episode.state === "released" && !this.watched(episode.id));
    return {
      showId: this.show.id,
      title: this.show.title,
      year: 2024,
      posterUrl: null,
      saved: this.library?.saved ?? false,
      status: this.library?.status ?? "plan_to_watch",
      revision: this.library?.revision ?? 0,
      progress: {
        watched: marked.length,
        total: regular.length,
        percent: Math.floor((marked.length * 100) / regular.length),
        state: next ? "in_progress" : "caught_up",
        nextEpisode: next ? { id: next.id, season: next.season, number: next.number } : null,
        outOfOrder: firstGap >= 0 && regular.slice(firstGap).some((episode) => this.watched(episode.id)),
        releaseInfoIncomplete: true,
      },
    };
  }
  showDto() {
    const seasons = new Map<number, number>();
    for (const episode of this.episodes) seasons.set(episode.season, (seasons.get(episode.season) ?? 0) + 1);
    return {
      id: this.show.id,
      title: this.show.title,
      year: 2024,
      genres: ["Drama", "Mystery"],
      synopsis: "A small orchard town keeps its secrets.",
      posterUrl: null,
      status: "returning",
      catalogRevision: 3,
      metadataStale: false,
      releaseInfoIncomplete: true,
      seasons: [...seasons].map(([number, count]) => ({ number, count })),
      library: this.library ? this.libraryItem() : null,
      trackingRevision: this.tracking,
    };
  }
  /** Writes episodes and records an undoable action; returns a MutationResult. */
  write(targets: string[], watched: boolean, autoAdd: boolean) {
    const changes: Change[] = [];
    for (const id of targets) {
      if (this.watched(id) === watched) continue;
      const revision = this.revision(id) + 1;
      changes.push({ id, before: this.watched(id), after: revision });
      this.progress.set(id, { watched, revision });
    }
    if (changes.length === 0) return this.noop();
    this.tracking += 1;
    let library = null;
    if (autoAdd && watched) {
      const before = this.library ? { ...this.library } : null;
      if (!this.library) this.library = { saved: true, status: "watching", revision: 1 };
      else if (!this.library.saved || this.library.status === "plan_to_watch")
        this.library = { saved: true, status: this.library.status === "plan_to_watch" ? "watching" : this.library.status, revision: this.library.revision + 1 };
      if (this.library.revision !== before?.revision) library = { before, after: this.library.revision };
    }
    return this.changed(changes, library);
  }
  changed(changes: Change[], library: { before: FakeLibrary | null; after: number } | null) {
    const actionId = this.uuid("40000000");
    this.actions.set(actionId, { changes, library });
    return {
      actionId,
      undoUntil: "2026-10-10T12:10:00Z",
      changed: changes.length + (library ? 1 : 0),
      library: this.libraryItem(),
      trackingRevision: this.tracking,
      episodes: changes.map((change) => ({ id: change.id, watched: this.watched(change.id), revision: change.after })),
    };
  }
  noop() {
    return { actionId: null, undoUntil: null, changed: 0, library: this.libraryItem(), trackingRevision: this.tracking, episodes: [] };
  }
  /** Another device marks an episode: bumps the aggregate revision so open previews go stale. */
  markElsewhere(id: string) {
    this.progress.set(id, { watched: true, revision: this.revision(id) + 1 });
    this.tracking += 1;
  }
  preview(throughId: string) {
    const through = this.byId.get(throughId)!;
    const before = this.regular.filter(
      (episode) => episode.season < through.season || (episode.season === through.season && episode.number <= through.number),
    );
    const short = (episode: FakeEpisode) => ({ id: episode.id, season: episode.season, episode: episode.number });
    const included = before.filter((episode) => episode.state === "released" && !this.watched(episode.id));
    const previewId = this.uuid("50000000");
    this.previews.set(previewId, { included: included.map((episode) => episode.id), tracking: this.tracking });
    return {
      previewId,
      expiresAt: new Date(Date.now() + 5 * 60_000).toISOString(),
      through: { season: through.season, episode: through.number },
      included: included.map(short),
      excluded: {
        alreadyWatched: before.filter((episode) => episode.state === "released" && this.watched(episode.id)).map(short),
        future: before.filter((episode) => episode.state === "future").map(short),
        undated: before.filter((episode) => episode.state === "unknown").map(short),
        specials: this.episodes.filter((episode) => episode.season === 0).map(short),
      },
      count: included.length,
    };
  }

  handle(route: Route, request: Request) {
    return serve(this, route, request);
  }
  read(path: string, url: URL): unknown {
    if (path === `/shows/${this.show.id}`) return this.showDto();
    if (path === `/shows/${this.show.id}/episodes`) {
      const season = Number(url.searchParams.get("season"));
      return { items: this.episodes.filter((episode) => episode.season === season).map((episode) => this.episodeDto(episode)), nextCursor: null };
    }
    const episode = this.byId.get(path.replace("/episodes/", ""));
    return episode ? this.episodeDto(episode) : null;
  }

  mutate(method: string, path: string, body: Record<string, unknown> | undefined): [number, unknown] {
    if (method === "PUT" && path.startsWith("/progress/episodes/")) {
      const id = path.split("/")[3];
      if (body!.expectedRevision !== this.revision(id)) return [409, "REVISION_CONFLICT"];
      return [200, this.write([id], body!.watched as boolean, true)];
    }
    if (path === `/library/${this.show.id}`) {
      if (body!.expectedRevision !== (this.library?.revision ?? 0)) return [409, "REVISION_CONFLICT"];
      const before = this.library ? { ...this.library } : null;
      const next: FakeLibrary =
        method === "PATCH"
          ? { ...this.library!, status: body!.status as FakeLibrary["status"] }
          : { saved: body!.saved as boolean, status: (body!.status as FakeLibrary["status"]) ?? this.library?.status ?? "plan_to_watch", revision: 0 };
      if (before && before.saved === next.saved && before.status === next.status) return [200, this.noop()];
      this.library = { ...next, revision: (before?.revision ?? 0) + 1 };
      return [200, this.changed([], { before, after: this.library.revision })];
    }
    if (method === "POST" && path === `/shows/${this.show.id}/catch-up/preview`) return [200, this.preview(body!.throughEpisodeId as string)];
    if (method === "POST" && path === `/shows/${this.show.id}/catch-up`) {
      const preview = this.previews.get(body!.previewId as string);
      if (!preview) return [410, "ACTION_EXPIRED"];
      if (preview.tracking !== this.tracking) return [409, "PREVIEW_STALE"];
      return [200, this.write(preview.included, true, true)];
    }
    if (method === "DELETE" && path === `/shows/${this.show.id}/history`) {
      if (body!.expectedRevision !== this.tracking) return [409, "REVISION_CONFLICT"];
      return [200, this.write(this.episodes.map((episode) => episode.id), false, false)];
    }
    if (method === "POST" && path.startsWith("/actions/")) return this.undo(path.split("/")[2]);
    return [404, "NOT_FOUND"];
  }
  undo(actionId: string): [number, unknown] {
    const action = this.actions.get(actionId);
    if (!action) return [404, "NOT_FOUND"];
    if (action.result) return [200, action.result];
    let reverted = 0;
    let skipped = 0;
    for (const change of action.changes) {
      if (this.revision(change.id) !== change.after) {
        skipped += 1;
        continue;
      }
      this.progress.set(change.id, { watched: change.before, revision: change.after + 1 });
      reverted += 1;
    }
    if (reverted) this.tracking += 1;
    if (action.library && this.library?.revision === action.library.after) {
      this.library = action.library.before
        ? { ...action.library.before, revision: this.library.revision + 1 }
        : { ...this.library, saved: false, revision: this.library.revision + 1 };
      reverted += 1;
    }
    action.result = { actionId, reverted, skipped, library: this.libraryItem() };
    return [200, action.result];
  }
  /** Writes only: GETs and catch-up previews change nothing. */
  writes() {
    return this.log.filter((entry) => entry.method !== "GET" && !entry.path.endsWith("/preview"));
  }
}
