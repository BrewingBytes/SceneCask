import axe from "axe-core";
import { expect, test, type Page, type Request, type Route } from "@playwright/test";

/**
 * Component suite for R21. The production /shows/[showId] route runs in an isolated fixture app;
 * a stateful fake of the C05 API answers at the /api/v1 network boundary, so the generated
 * client, CSRF bootstrap, Idempotency-Key headers and revisions are exercised. This is not live
 * integration (R24 owns that gate).
 */

const CSRF = "csrf-fixture-token";
const SHOW = "10000000-0000-4000-8000-000000000001";
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const SECRET = "Secret title";

type ReleaseState = "released" | "future" | "unknown";
interface FakeEpisode {
  id: string;
  season: number;
  number: number;
  state: ReleaseState;
  date: string | null;
}
const episodeId = (season: number, number: number) =>
  `30000000-0000-4000-8000-0000000${String(season).padStart(2, "0")}${String(number).padStart(3, "0")}`;
const ep = (season: number, number: number, state: ReleaseState = "released", date: string | null = "2026-01-01"): FakeEpisode => ({
  id: episodeId(season, number),
  season,
  number,
  state,
  date: state === "unknown" ? null : date,
});
/** S1: E1–E3 released, E4 undated, E5 released, E6 future. S2: E1–E2 released. One special. */
const EPISODES = [
  ep(1, 1),
  ep(1, 2),
  ep(1, 3),
  ep(1, 4, "unknown"),
  ep(1, 5),
  ep(1, 6, "future", "2026-11-12"),
  ep(2, 1),
  ep(2, 2),
  ep(0, 1),
];
const byId = new Map(EPISODES.map((episode) => [episode.id, episode]));
const regular = EPISODES.filter((episode) => episode.season > 0);
const code = (episode: FakeEpisode) => `S${episode.season} E${episode.number}`;
const later = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

interface Library {
  saved: boolean;
  status: "plan_to_watch" | "watching" | "on_hold" | "dropped";
  revision: number;
}
interface Change {
  id: string;
  before: boolean;
  after: number;
}
interface Logged {
  method: string;
  path: string;
  body: unknown;
  key: string | null;
  csrf: string | null;
}

/** A small in-memory C05 backend: per-episode revisions, aggregate tracking revision, undo. */
class FakeApi {
  progress = new Map<string, { watched: boolean; revision: number }>();
  tracking = 0;
  library: Library | null = null;
  actions = new Map<string, { changes: Change[]; library: { before: Library | null; after: number } | null; result?: unknown }>();
  previews = new Map<string, { included: string[]; tracking: number }>();
  idempotent = new Map<string, { body: string; response: unknown; status: number }>();
  log: Logged[] = [];
  private sequence = 0;

  constructor(watched: number[][] = [], library: Library | null = null) {
    for (const [season, number] of watched) this.progress.set(episodeId(season, number), { watched: true, revision: 1 });
    this.tracking = watched.length;
    this.library = library;
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
  episodeDto(episode: FakeEpisode) {
    const base = {
      id: episode.id,
      showId: SHOW,
      season: episode.season,
      number: episode.number,
      release: { state: episode.state, date: episode.date, timezone: episode.date ? "UTC" : null, estimated: episode.state === "future" },
      watched: this.watched(episode.id),
      revision: this.revision(episode.id),
    };
    return base.watched
      ? { ...base, detailsAccess: "watched", details: { title: `${SECRET} ${code(episode)}`, overview: "Protected overview.", stillUrl: null } }
      : { ...base, detailsAccess: "locked" };
  }
  libraryItem() {
    const marked = regular.filter((episode) => this.watched(episode.id));
    const next = regular.find((episode) => episode.state === "released" && !this.watched(episode.id));
    const firstGap = regular.findIndex((episode) => episode.state === "released" && !this.watched(episode.id));
    return {
      showId: SHOW,
      title: "Hollow Orchard",
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
    return {
      id: SHOW,
      title: "Hollow Orchard",
      year: 2024,
      genres: ["Drama", "Mystery"],
      synopsis: "A small orchard town keeps its secrets.",
      posterUrl: null,
      status: "returning",
      catalogRevision: 3,
      metadataStale: false,
      releaseInfoIncomplete: true,
      seasons: [
        { number: 1, count: 6 },
        { number: 2, count: 2 },
        { number: 0, count: 1 },
      ],
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
  changed(changes: Change[], library: { before: Library | null; after: number } | null) {
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
    const through = byId.get(throughId)!;
    const before = regular.filter(
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
        specials: EPISODES.filter((episode) => episode.season === 0).map(short),
      },
      count: included.length,
    };
  }

  async handle(route: Route, request: Request) {
    const url = new URL(request.url());
    const path = url.pathname.replace("/api/v1", "");
    const method = request.method();
    const body = request.postData() ? request.postDataJSON() : undefined;
    const key = request.headers()["idempotency-key"] ?? null;
    if (path === "/session") return json(route, { user: null, csrfToken: CSRF });
    this.log.push({ method, path, body, key, csrf: request.headers()["x-csrf-token"] ?? null });
    if (method === "GET") {
      if (path === `/shows/${SHOW}`) return json(route, this.showDto());
      if (path === `/shows/${SHOW}/episodes`) {
        const season = Number(url.searchParams.get("season"));
        return json(route, { items: EPISODES.filter((episode) => episode.season === season).map((episode) => this.episodeDto(episode)), nextCursor: null });
      }
      const episode = byId.get(path.replace("/episodes/", ""));
      if (episode) return json(route, this.episodeDto(episode));
      return error(route, 404, "NOT_FOUND");
    }
    // Same key + same body replays the stored response; a changed body is a conflict.
    if (key) {
      const stored = this.idempotent.get(key);
      if (stored) return stored.body === JSON.stringify(body) ? json(route, stored.response, stored.status) : error(route, 409, "REVISION_CONFLICT");
    }
    const [status, response] = this.mutate(method, path, body);
    if (key && status < 300) this.idempotent.set(key, { body: JSON.stringify(body), response, status });
    return status < 300 ? json(route, response, status) : error(route, status, response as string);
  }

  mutate(method: string, path: string, body: Record<string, unknown> | undefined): [number, unknown] {
    if (method === "PUT" && path.startsWith("/progress/episodes/")) {
      const id = path.split("/")[3];
      if (body!.expectedRevision !== this.revision(id)) return [409, "REVISION_CONFLICT"];
      return [200, this.write([id], body!.watched as boolean, true)];
    }
    if (path === `/library/${SHOW}`) {
      if (body!.expectedRevision !== (this.library?.revision ?? 0)) return [409, "REVISION_CONFLICT"];
      const before = this.library ? { ...this.library } : null;
      const next: Library =
        method === "PATCH"
          ? { ...this.library!, status: body!.status as Library["status"] }
          : { saved: body!.saved as boolean, status: (body!.status as Library["status"]) ?? this.library?.status ?? "plan_to_watch", revision: 0 };
      if (before && before.saved === next.saved && before.status === next.status) return [200, this.noop()];
      this.library = { ...next, revision: (before?.revision ?? 0) + 1 };
      return [200, this.changed([], { before, after: this.library.revision })];
    }
    if (method === "POST" && path === `/shows/${SHOW}/catch-up/preview`) return [200, this.preview(body!.throughEpisodeId as string)];
    if (method === "POST" && path === `/shows/${SHOW}/catch-up`) {
      const preview = this.previews.get(body!.previewId as string);
      if (!preview) return [410, "ACTION_EXPIRED"];
      if (preview.tracking !== this.tracking) return [409, "PREVIEW_STALE"];
      return [200, this.write(preview.included, true, true)];
    }
    if (method === "DELETE" && path === `/shows/${SHOW}/history`) {
      if (body!.expectedRevision !== this.tracking) return [409, "REVISION_CONFLICT"];
      return [200, this.write(EPISODES.map((episode) => episode.id), false, false)];
    }
    if (method === "POST" && path.startsWith("/actions/")) {
      const action = this.actions.get(path.split("/")[2]);
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
        this.library = action.library.before ? { ...action.library.before, revision: this.library.revision + 1 } : { ...this.library, saved: false, revision: this.library.revision + 1 };
        reverted += 1;
      }
      action.result = { actionId: path.split("/")[2], reverted, skipped, library: this.libraryItem() };
      return [200, action.result];
    }
    return [404, "NOT_FOUND"];
  }
  /** Writes only: GETs and catch-up previews change nothing. */
  writes() {
    return this.log.filter((entry) => entry.method !== "GET" && !entry.path.endsWith("/preview"));
  }
}

const json = (route: Route, body: unknown, status = 200) =>
  route.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) });
const error = (route: Route, status: number, code: string) =>
  json(route, { error: { code, message: "Safe server message.", requestId: "60000000-0000-4000-8000-000000000001" } }, status);

async function openShow(page: Page, fake: FakeApi, query = "") {
  await page.route("**/api/v1/**", (route, request) => fake.handle(route, request));
  await page.goto(`/shows/${SHOW}${query}`);
  await expect(page.getByRole("heading", { level: 1, name: "Hollow Orchard" })).toBeVisible();
  await expect(episodes(page, "Season 1").getByRole("listitem")).toHaveCount(6);
}
const episodes = (page: Page, season: string) => page.getByRole("list", { name: `${season} episodes` });
const toggle = (page: Page, label: string) => page.getByRole("button", { name: `${label} watched`, exact: true });
const sheet = (page: Page) => page.getByRole("dialog");
const toast = (page: Page, text: string) => page.getByRole("status").filter({ hasText: text });

test.beforeEach(async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
});

test("one click marks only that episode; details unlock only after the authorized fetch", async ({ page }) => {
  const fake = new FakeApi();
  await openShow(page, fake);
  await expect(page.getByText(SECRET)).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Add to library" })).toBeVisible();

  await toggle(page, "S1 E3").click();
  await expect(toggle(page, "S1 E3")).toHaveAttribute("aria-pressed", "true");
  await expect(toast(page, "Marked S1 E3 watched and added the show to your library.")).toBeVisible();
  await expect(episodes(page, "Season 1").getByText(`${SECRET} S1 E3`)).toBeVisible();

  const writes = fake.writes();
  expect(writes).toHaveLength(1);
  expect(writes[0]).toMatchObject({ method: "PUT", path: `/progress/episodes/${episodeId(1, 3)}`, body: { watched: true, expectedRevision: 0 }, csrf: CSRF });
  expect(writes[0].key).toMatch(UUID);
  for (const earlier of ["S1 E1", "S1 E2"]) await expect(toggle(page, earlier)).toHaveAttribute("aria-pressed", "false");
  expect(fake.watched(episodeId(1, 1)) || fake.watched(episodeId(1, 2))).toBe(false);
  // Out-of-order viewing keeps the earlier gap as next.
  await expect(page.getByText("Next: S1 E1")).toBeVisible();
  await expect(page.getByText("Some episodes are marked out of order.")).toBeVisible();
  // No protected text reaches other rows or any accessible name.
  await expect(page.getByText(SECRET)).toHaveCount(1);
  await expect(page.getByRole("link", { name: new RegExp(SECRET) })).toHaveCount(0);
});

test("a future episode can be marked and keeps its future label; undated rows can be marked too", async ({ page }) => {
  const fake = new FakeApi();
  await openShow(page, fake);
  const future = episodes(page, "Season 1").getByRole("listitem").nth(5);
  await expect(future).toContainText("Not released yet");
  await expect(future).toContainText("Airs Nov 12, 2026 (estimated)");
  await expect(toggle(page, "S1 E6")).toBeEnabled();
  await toggle(page, "S1 E6").click();
  await expect(toggle(page, "S1 E6")).toHaveAttribute("aria-pressed", "true");
  await expect(future).toContainText("Airs Nov 12, 2026 (estimated) · Watched");
  await expect(page.getByRole("link", { name: "Open S1 E6, Airs Nov 12, 2026 (estimated) · Watched" })).toBeVisible();

  const undated = episodes(page, "Season 1").getByRole("listitem").nth(3);
  await expect(undated).toContainText("Release date unknown");
  await toggle(page, "S1 E4").click();
  await expect(undated).toContainText("Release date unknown · Watched");
  expect(fake.writes().map((write) => write.path)).toEqual([`/progress/episodes/${episodeId(1, 6)}`, `/progress/episodes/${episodeId(1, 4)}`]);
  // Next suggestion stays on released episodes.
  await expect(page.getByRole("button", { name: "Mark S1 E1 watched" })).toBeVisible();
});

test("catch-up previews the exact scope; a stale preview is refreshed and must be confirmed again", async ({ page }) => {
  const fake = new FakeApi([[1, 1]], { saved: true, status: "watching", revision: 1 });
  await openShow(page, fake);
  await page.getByRole("button", { name: "Catch up…" }).click();
  await expect(sheet(page)).toBeVisible();
  await sheet(page).getByRole("button", { name: "S1 E5" }).click();
  await expect(sheet(page)).toContainText("Marks 3 episodes watched: S1 E2–E3, E5.");
  await expect(sheet(page)).toContainText("Already watched, left as they are: S1 E1");
  await expect(sheet(page)).toContainText("Release date unknown: S1 E4");
  await expect(sheet(page)).toContainText("Specials: Special 1");

  // Another device marks S1 E3 before confirmation.
  fake.markElsewhere(episodeId(1, 3));
  await sheet(page).getByRole("button", { name: "Mark 3 watched" }).click();
  await expect(sheet(page).getByRole("alert").filter({ hasText: "Episodes changed since you reviewed this." })).toBeVisible();
  await expect(sheet(page)).toContainText("Marks 2 episodes watched: S1 E2, E5.");
  const commits = () => fake.writes().filter((write) => write.path.endsWith("/catch-up"));
  expect(commits()).toHaveLength(1);
  expect(fake.watched(episodeId(1, 2))).toBe(false);
  // Nothing is submitted until the new scope is confirmed.
  await page.waitForTimeout(300);
  expect(commits()).toHaveLength(1);

  await sheet(page).getByRole("button", { name: "Mark 2 watched" }).click();
  await expect(sheet(page)).toBeHidden();
  await expect(toast(page, "Marked 2 episodes watched through S1 E5.")).toBeVisible();
  expect(commits()).toHaveLength(2);
  expect(commits()[1].body).toEqual({ previewId: expect.not.stringMatching(String((commits()[0].body as { previewId: string }).previewId)) });
  expect(commits()[1].key).toMatch(UUID);
  for (const label of ["S1 E1", "S1 E2", "S1 E3", "S1 E5"]) await expect(toggle(page, label)).toHaveAttribute("aria-pressed", "true");
  for (const label of ["S1 E4", "S1 E6"]) await expect(toggle(page, label)).toHaveAttribute("aria-pressed", "false");
});

test("the add sheet saves without progress or previews before committing", async ({ page }) => {
  const fake = new FakeApi();
  await openShow(page, fake);
  await page.getByRole("button", { name: "Add to library" }).click();
  await expect(sheet(page).getByRole("heading", { name: "Where are you in Hollow Orchard?" })).toBeVisible();
  await sheet(page).getByRole("button", { name: /Haven’t started yet/ }).click();
  await expect(sheet(page)).toContainText("Saved as Plan to watch. No episodes are marked.");
  await sheet(page).getByRole("button", { name: "Add to library" }).click();
  await expect(sheet(page)).toBeHidden();
  await expect(toast(page, "Added Hollow Orchard to Plan to watch.")).toBeVisible();
  expect(fake.writes()).toEqual([
    expect.objectContaining({ method: "PUT", path: `/library/${SHOW}`, body: { saved: true, status: "plan_to_watch", expectedRevision: 0 } }),
  ]);
  await expect(page.getByRole("button", { name: "Library status: Plan to watch" })).toBeVisible();
  // No preview request without an explicit endpoint choice.
  expect(fake.log.some((entry) => entry.path.endsWith("/preview"))).toBe(false);
});

test("erase history asks for separate confirmation first, then offers a working Undo", async ({ page }) => {
  const fake = new FakeApi([[1, 1], [1, 2], [0, 1]], { saved: true, status: "on_hold", revision: 1 });
  await openShow(page, fake);
  await page.getByRole("button", { name: "Erase watch history…" }).click();
  const dialog = page.getByRole("alertdialog", { name: "Erase watch history for Hollow Orchard?" });
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText("including specials");
  expect(fake.writes()).toHaveLength(0);
  await expect(dialog.getByRole("button", { name: "Cancel" })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  expect(fake.writes()).toHaveLength(0);

  await page.getByRole("button", { name: "Erase watch history…" }).click();
  await dialog.getByRole("button", { name: "Erase history" }).click();
  await expect(dialog).toBeHidden();
  const erased = toast(page, "Erased watch history for Hollow Orchard.");
  await expect(erased).toBeVisible();
  expect(fake.writes()).toEqual([expect.objectContaining({ method: "DELETE", path: `/shows/${SHOW}/history`, body: { expectedRevision: 3 } })]);
  await expect(toggle(page, "S1 E1")).toHaveAttribute("aria-pressed", "false");
  await expect(page.getByRole("button", { name: "Library status: On hold" })).toBeVisible();

  await erased.getByRole("button", { name: "Undo" }).click();
  await expect(toast(page, "Your watch history for Hollow Orchard is back.")).toBeVisible();
  await expect(toggle(page, "S1 E1")).toHaveAttribute("aria-pressed", "true");
  await expect(toggle(page, "S1 E2")).toHaveAttribute("aria-pressed", "true");
  expect(fake.writes().at(-1)).toMatchObject({ method: "POST", path: expect.stringMatching(/^\/actions\//), body: {} });
});

test("erase failure stays in the dialog and changes nothing", async ({ page }) => {
  const fake = new FakeApi([[1, 1]], { saved: true, status: "watching", revision: 1 });
  await openShow(page, fake);
  await page.route("**/api/v1/shows/*/history", (route) => error(route, 422, "VALIDATION_ERROR"));
  await page.getByRole("button", { name: "Erase watch history…" }).click();
  const dialog = page.getByRole("alertdialog");
  await dialog.getByRole("button", { name: "Erase history" }).click();
  await expect(dialog.getByRole("alert")).toContainText("Couldn’t erase history. Nothing was changed.");
  await expect(dialog).not.toContainText("Safe server message.");
  await expect(dialog.getByRole("button", { name: "Try again" })).toBeVisible();
  await expect(toggle(page, "S1 E1")).toHaveAttribute("aria-pressed", "true");
});

test("concurrent marks settle independently; an unknown outcome retries with the same key", async ({ page }) => {
  const fake = new FakeApi();
  await openShow(page, fake);
  let failFirst = true;
  const e1: { key: string | null; body: unknown }[] = [];
  await page.route(`**/api/v1/progress/episodes/${episodeId(1, 1)}`, async (route, request) => {
    e1.push({ key: request.headers()["idempotency-key"] ?? null, body: request.postDataJSON() });
    await later(600);
    if (failFirst) {
      failFirst = false;
      return route.fulfill({ status: 503, contentType: "application/json", body: "{}" });
    }
    return route.fallback();
  });
  await toggle(page, "S1 E1").click();
  await toggle(page, "S1 E2").click();
  // Both optimistic, before either answer.
  await expect(toggle(page, "S1 E1")).toHaveAttribute("aria-pressed", "true");
  await expect(toggle(page, "S1 E2")).toHaveAttribute("aria-pressed", "true");
  // A repeat click while E1 is in flight is ignored.
  await toggle(page, "S1 E1").click();

  const failure = page.getByRole("alert").filter({ hasText: "Couldn’t mark S1 E1 watched." });
  await expect(failure).toBeVisible();
  await expect(toggle(page, "S1 E1")).toHaveAttribute("aria-pressed", "false");
  await expect(toggle(page, "S1 E2")).toHaveAttribute("aria-pressed", "true");
  expect(fake.watched(episodeId(1, 2))).toBe(true);

  await failure.getByRole("button", { name: "Retry" }).click();
  await expect(toggle(page, "S1 E1")).toHaveAttribute("aria-pressed", "true");
  await expect(toast(page, "Marked S1 E1 watched.")).toBeVisible();
  expect(e1).toHaveLength(2);
  expect(e1[0].key).toMatch(UUID);
  expect(e1[1].key).toBe(e1[0].key);
  expect(e1[1].body).toEqual(e1[0].body);
});

test("a revision conflict rolls back to the authoritative state", async ({ page }) => {
  const fake = new FakeApi();
  await openShow(page, fake);
  fake.markElsewhere(episodeId(1, 2));
  await toggle(page, "S1 E2").click();
  await expect(page.getByRole("alert").filter({ hasText: "Your progress changed somewhere else." })).toBeVisible();
  // The refetch shows the other device's mark.
  await expect(toggle(page, "S1 E2")).toHaveAttribute("aria-pressed", "true");
  await expect(episodes(page, "Season 1").getByText(`${SECRET} S1 E2`)).toBeVisible();
});

test("seasons, specials and statuses work by keyboard; the season is kept in the URL", async ({ page }) => {
  const fake = new FakeApi([[1, 1]], { saved: true, status: "watching", revision: 1 });
  await openShow(page, fake);
  await page.getByRole("tab", { name: "Season 1" }).focus();
  await page.keyboard.press("ArrowRight");
  await expect(page.getByRole("tab", { name: "Season 2" })).toHaveAttribute("aria-selected", "true");
  await expect(episodes(page, "Season 2").getByRole("listitem")).toHaveCount(2);
  await expect(page).toHaveURL(/season=2/);
  await page.keyboard.press("End");
  await expect(episodes(page, "Specials").getByRole("listitem")).toHaveCount(1);
  await expect(page.getByText("Specials are tracked separately.")).toBeVisible();
  // Specials have no catch-up action.
  await page.getByRole("button", { name: "More actions for Special 1" }).click();
  await expect(page.getByRole("menuitem", { name: /Mark watched through/ })).toHaveCount(0);
  await page.keyboard.press("Escape");

  await page.reload();
  await expect(page.getByRole("tab", { name: "Specials" })).toHaveAttribute("aria-selected", "true");

  const status = page.getByRole("button", { name: "Library status: Watching" });
  await status.focus();
  await page.keyboard.press("Enter");
  await page.getByRole("menuitemcheckbox", { name: "On hold" }).focus();
  await page.keyboard.press("Enter");
  // Optimistic first, then the write lands.
  await expect(page.getByRole("button", { name: "Library status: On hold" })).toBeVisible();
  await expect.poll(() => fake.writes().at(-1)).toMatchObject({ method: "PATCH", body: { status: "on_hold", expectedRevision: 1 } });
  await expect(toast(page, "Hollow Orchard is now On hold.")).toBeVisible();
});

test("row menu opens catch-up through that episode; Escape cancels without writing", async ({ page }) => {
  const fake = new FakeApi([], { saved: true, status: "watching", revision: 1 });
  await openShow(page, fake);
  await page.getByRole("button", { name: "More actions for S1 E3" }).click();
  await page.getByRole("menuitem", { name: "Mark watched through S1 E3…" }).click();
  await expect(sheet(page)).toContainText("Marks 3 episodes watched: S1 E1–E3.");
  await expect(sheet(page).getByRole("button", { name: "S1 E3" })).toHaveAttribute("aria-pressed", "true");
  await page.keyboard.press("Escape");
  await expect(sheet(page)).toBeHidden();
  await expect(page.getByRole("button", { name: "More actions for S1 E3" })).toBeFocused();
  expect(fake.writes()).toHaveLength(0);
});

test("load failure and missing show states use product copy", async ({ page }) => {
  let fail = true;
  const fake = new FakeApi();
  await page.route("**/api/v1/**", (route, request) => {
    if (fail && request.url().endsWith(`/shows/${SHOW}`)) return error(route, 503, "SERVICE_UNAVAILABLE");
    return fake.handle(route, request);
  });
  await page.goto(`/shows/${SHOW}`);
  const alert = page.getByRole("alert").filter({ hasText: "This show didn’t load" });
  await expect(alert).toBeVisible();
  await expect(alert).not.toContainText("Safe server message.");
  fail = false;
  await page.getByRole("button", { name: "Retry" }).click();
  await expect(page.getByRole("heading", { level: 1, name: "Hollow Orchard" })).toBeVisible();

  await page.goto("/shows/10000000-0000-4000-8000-00000000dead");
  await expect(page.getByRole("heading", { name: "Show not found" })).toBeVisible();
});

const scan = async (page: Page) => {
  await page.addScriptTag({ content: axe.source });
  return page.evaluate(async () => {
    const result = await (window as unknown as { axe: typeof axe }).axe.run(document, {
      runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21aa"] },
    });
    return result.violations.map((violation) => `${violation.id}: ${violation.nodes.map((node) => node.target).join(", ")}`);
  });
};
const overflow = (page: Page) => page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);

for (const width of [320, 390, 860, 1440]) {
  test(`layout at ${width}px: no overflow, 44px targets, axe clean`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    const fake = new FakeApi([[1, 1]], { saved: true, status: "watching", revision: 1 });
    await openShow(page, fake);
    expect(await overflow(page)).toBe(0);
    await expect(page.locator(width >= 860 ? "header nav" : "nav").first()).toBeVisible();
    const small = await page.locator(".sc-show button:visible, .sc-show a:visible").evaluateAll((elements) =>
      elements.filter((element) => element.getBoundingClientRect().height < 44).map((element) => element.textContent),
    );
    expect(small).toEqual([]);
    expect(await scan(page)).toEqual([]);
    if (width === 390 || width === 1440) await page.screenshot({ path: `apps/web/src/features/catalog/show/verification/show-${width}.png`, fullPage: true });

    await page.getByRole("button", { name: "Catch up…" }).click();
    await sheet(page).getByRole("button", { name: "S1 E5" }).click();
    await expect(sheet(page)).toContainText("Marks 3 episodes watched");
    expect(await overflow(page)).toBe(0);
    expect(await scan(page)).toEqual([]);
    if (width === 390 || width === 1440) await page.screenshot({ path: `apps/web/src/features/catalog/show/verification/catchup-${width}.png` });

    fake.markElsewhere(episodeId(1, 2));
    await sheet(page).getByRole("button", { name: "Mark 3 watched" }).click();
    await expect(sheet(page).getByRole("alert").filter({ hasText: "Episodes changed" })).toBeVisible();
    if (width === 390 || width === 1440) await page.screenshot({ path: `apps/web/src/features/catalog/show/verification/catchup-stale-${width}.png` });
    await page.keyboard.press("Escape");

    await page.getByRole("button", { name: "Erase watch history…" }).click();
    await expect(page.getByRole("alertdialog")).toBeVisible();
    expect(await scan(page)).toEqual([]);
    if (width === 390 || width === 1440) await page.screenshot({ path: `apps/web/src/features/catalog/show/verification/erase-${width}.png` });
  });
}
