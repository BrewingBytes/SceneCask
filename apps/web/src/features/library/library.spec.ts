import axe from "axe-core";
import { expect, test, type Page, type Request, type Route } from "@playwright/test";
import { CSRF, error, later, serve, UUID, type Logged } from "../tracking/fixture-api";

/**
 * Component suite for R20. The production /library route runs in an isolated fixture app; a
 * stateful fake of the C05 library API answers at the /api/v1 network boundary, so the
 * generated client, CSRF bootstrap, Idempotency-Key headers and revisions are exercised. This
 * is not live integration (R24 owns that gate).
 */

const STATUSES = ["plan_to_watch", "watching", "on_hold", "dropped"] as const;
type Status = (typeof STATUSES)[number];
type ProgressState = "in_progress" | "caught_up" | "completed" | "not_yet_available" | "release_unknown";

interface Entry {
  showId: string;
  title: string;
  year: number | null;
  saved: boolean;
  status: Status;
  revision: number;
  progress: {
    watched: number;
    total: number;
    percent: number;
    state: ProgressState;
    nextEpisode: { id: string; season: number; number: number } | null;
    outOfOrder: boolean;
    releaseInfoIncomplete: boolean;
  };
}
const showId = (n: number) => `10000000-0000-4000-8000-${String(n).padStart(12, "0")}`;
const entry = (n: number, title: string, status: Status, progress: Partial<Entry["progress"]> = {}): Entry => ({
  showId: showId(n),
  title,
  year: 2020 + n,
  saved: true,
  status,
  revision: 1,
  progress: { watched: 2, total: 6, percent: 33, state: "in_progress", nextEpisode: null, outOfOrder: false, releaseInfoIncomplete: false, ...progress },
});
const HOLLOW = 1;
const LANTERN = 2;
const NIGHT = 3;
const PAPER = 4;
/** Saved_at descending: Hollow (watching), Lantern (canceled, incomplete), Night (ended), Paper (plan). */
const LIBRARY = () => [
  entry(HOLLOW, "Hollow Orchard", "watching", {
    nextEpisode: { id: "30000000-0000-4000-8000-000000000002", season: 1, number: 2 },
    outOfOrder: true,
  }),
  entry(LANTERN, "Lantern Coast", "watching", { watched: 8, total: 8, percent: 100, state: "caught_up", releaseInfoIncomplete: true }),
  entry(NIGHT, "Night Shift", "on_hold", { watched: 10, total: 10, percent: 100, state: "completed" }),
  entry(PAPER, "Paper Moons", "plan_to_watch", { watched: 0, total: 0, percent: 0, state: "not_yet_available" }),
];

/** A small in-memory C05 library: revisions, counts, filters, cursor pages and undo. */
class FakeApi {
  entries: Entry[];
  actions = new Map<string, { showId: string; before: Entry; after: number; result?: unknown }>();
  idempotent = new Map<string, { body: string; response: unknown; status: number }>();
  log: Logged[] = [];
  pageSize = 50;
  private sequence = 0;

  constructor(entries: Entry[] = LIBRARY()) {
    this.entries = entries;
  }
  get(n: number) {
    return this.entries.find((item) => item.showId === showId(n))!;
  }
  /** Another device edits the entry: the revision moves on. */
  elsewhere(n: number, change: Partial<Entry>) {
    Object.assign(this.get(n), change, { revision: this.get(n).revision + 1 });
  }
  counts() {
    const saved = this.entries.filter((item) => item.saved);
    return Object.fromEntries([["all", saved.length], ...STATUSES.map((status) => [status, saved.filter((item) => item.status === status).length])]);
  }
  list(url: URL) {
    const status = url.searchParams.get("status");
    const q = (url.searchParams.get("q") ?? "").toLowerCase();
    const matching = this.entries.filter((item) => item.saved && (!status || item.status === status) && item.title.toLowerCase().includes(q));
    const start = Number(url.searchParams.get("cursor") ?? 0);
    const end = start + this.pageSize;
    return { items: structuredClone(matching.slice(start, end)), nextCursor: end < matching.length ? String(end) : null, counts: this.counts() };
  }

  handle(route: Route, request: Request) {
    return serve(this, route, request);
  }
  read(path: string, url: URL) {
    if (path === "/library") return this.list(url);
    // Only the fields the recovery read uses; Show DTOs are covered by the show-detail suite.
    const found = this.entries.find((item) => `/shows/${item.showId}` === path);
    return found ? { id: found.showId, title: found.title, library: structuredClone(found), trackingRevision: 1 } : null;
  }

  mutate(method: string, path: string, body: Record<string, unknown> | undefined): [number, unknown] {
    if (path.startsWith("/library/")) {
      const found = this.entries.find((item) => item.showId === path.split("/")[2]);
      if (!found || (method === "PATCH" && !found.saved)) return [404, "NOT_FOUND"];
      if (body!.expectedRevision !== found.revision) return [409, "REVISION_CONFLICT"];
      const before = structuredClone(found);
      if (method === "PATCH") found.status = body!.status as Status;
      else found.saved = body!.saved as boolean;
      if (before.status === found.status && before.saved === found.saved)
        return [200, { actionId: null, undoUntil: null, changed: 0, library: structuredClone(found), trackingRevision: 1, episodes: [] }];
      found.revision += 1;
      this.sequence += 1;
      const actionId = `40000000-0000-4000-8000-${String(this.sequence).padStart(12, "0")}`;
      this.actions.set(actionId, { showId: found.showId, before, after: found.revision });
      return [200, { actionId, undoUntil: "2026-10-12T12:10:00Z", changed: 1, library: structuredClone(found), trackingRevision: 1, episodes: [] }];
    }
    if (method === "POST" && path.startsWith("/actions/")) return this.undo(path.split("/")[2]);
    return [404, "NOT_FOUND"];
  }
  /** Reverts only when the entry is still at the action's revision; a repeat replays the outcome. */
  undo(actionId: string): [number, unknown] {
    const action = this.actions.get(actionId);
    if (!action) return [404, "NOT_FOUND"];
    if (!action.result) {
      const found = this.entries.find((item) => item.showId === action.showId)!;
      const current = found.revision === action.after;
      if (current) Object.assign(found, { saved: action.before.saved, status: action.before.status, revision: found.revision + 1 });
      action.result = { actionId, reverted: current ? 1 : 0, skipped: current ? 0 : 1, library: structuredClone(found) };
    }
    return [200, action.result];
  }
  writes() {
    return this.log.filter((item) => item.method !== "GET");
  }
  reads() {
    return this.log.filter((item) => item.method === "GET" && item.path === "/library");
  }
}

async function openLibrary(page: Page, fake: FakeApi, query = "") {
  await page.route("**/api/v1/**", (route, request) => fake.handle(route, request));
  await page.goto(`/library${query}`);
  await expect(page.getByRole("heading", { level: 1, name: "Library" })).toBeVisible();
  await expect(page.getByText("Loading your library…")).toHaveCount(0);
}
const rows = (page: Page) => page.getByRole("list", { name: "Saved shows" }).getByRole("listitem");
const row = (page: Page, title: string) => rows(page).filter({ has: page.getByRole("link", { name: new RegExp(`^${title} `) }) });
const filter = (page: Page, label: string) => page.getByRole("group", { name: "Filter by status" }).getByRole("button", { name: new RegExp(`^${label}`) });
const search = (page: Page) => page.getByRole("searchbox", { name: "Search your library" });
const menu = (page: Page, title: string) => page.getByRole("button", { name: new RegExp(`^${title}: .*Change status or remove$`) });
const toast = (page: Page, text: string) => page.getByRole("status").filter({ hasText: text });
const failure = (page: Page, text: string) => page.getByRole("alert").filter({ hasText: text });

async function choose(page: Page, title: string, item: string) {
  await menu(page, title).click();
  await page.getByRole(item === "Remove from library" ? "menuitem" : "menuitemcheckbox", { name: item }).click();
}

test.beforeEach(async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
});

test("rows show derived progress; four manual filters with counts and no Completed filter", async ({ page }) => {
  const fake = new FakeApi();
  await openLibrary(page, fake);
  await expect(rows(page)).toHaveCount(4);
  await expect(page.getByText("4 shows", { exact: true }).first()).toBeVisible();
  const labels = await page.getByRole("group", { name: "Filter by status" }).getByRole("button").allTextContents();
  expect(labels).toEqual(["All 4", "Plan to watch 1", "Watching 2", "On hold 1", "Dropped 0"]);
  await expect(filter(page, "All")).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByRole("button", { name: /^Completed/ })).toHaveCount(0);

  await expect(row(page, "Hollow Orchard")).toContainText("2 of 6 episodes watched · Next S1 E2 · some out of order");
  await expect(menu(page, "Hollow Orchard")).toHaveAccessibleName("Hollow Orchard: Watching. Change status or remove");
  // A canceled show with incomplete release info is caught up, never completed.
  const lantern = row(page, "Lantern Coast");
  await expect(lantern).toContainText("Caught up");
  await expect(lantern).toContainText("8 of 8 episodes watched · release info incomplete");
  await expect(lantern).not.toContainText("Completed");
  await expect(row(page, "Night Shift")).toContainText("Completed");
  await expect(row(page, "Night Shift")).toContainText("On hold");
  await expect(row(page, "Paper Moons")).toContainText("Not yet available");
  await expect(row(page, "Hollow Orchard").getByRole("progressbar", { name: "Hollow Orchard progress" })).toHaveAttribute("aria-valuenow", "33");

  await row(page, "Lantern Coast").getByRole("link").click();
  await expect(page).toHaveURL(new RegExp(`/shows/${showId(LANTERN)}$`));
});

test("status filter and title search use the API, stay in the URL and survive reload", async ({ page }) => {
  const fake = new FakeApi();
  await openLibrary(page, fake);
  await filter(page, "Watching").click();
  await expect(rows(page)).toHaveCount(2);
  await expect(filter(page, "Watching")).toHaveAttribute("aria-pressed", "true");
  await search(page).fill("hollow");
  await expect(rows(page)).toHaveCount(1);
  await expect(page).toHaveURL(/\/library\?status=watching&q=hollow$/);
  expect(fake.reads().at(-1)?.query).toEqual({ status: "watching", q: "hollow" });
  // Typing settles once instead of a request per keystroke.
  expect(fake.reads().filter((read) => read.query.q !== undefined)).toHaveLength(1);
  // Counts stay across all saved shows, not just the matches.
  await expect(filter(page, "All")).toHaveText("All 4");

  await page.reload();
  await expect(search(page)).toHaveValue("hollow");
  await expect(filter(page, "Watching")).toHaveAttribute("aria-pressed", "true");
  await expect(rows(page)).toHaveCount(1);
  await expect(row(page, "Hollow Orchard")).toBeVisible();
});

test("an empty library offers Discover", async ({ page }) => {
  const fake = new FakeApi([]);
  await openLibrary(page, fake);
  await expect(page.getByRole("heading", { name: "Your library is empty" })).toBeVisible();
  await expect(search(page)).toHaveCount(0);
  await page.getByRole("link", { name: "Find a show" }).click();
  await expect(page).toHaveURL(/\/discover$/);
});

test("no matches keeps the search and filter and offers a clear action", async ({ page }) => {
  const fake = new FakeApi();
  await openLibrary(page, fake, "?status=dropped&q=orchard");
  await expect(page.getByRole("heading", { name: "No Dropped shows match “orchard”." })).toBeVisible();
  await expect(page.getByRole("status").filter({ hasText: "No matching shows" })).toBeVisible();
  await expect(search(page)).toHaveValue("orchard");
  await expect(filter(page, "Dropped")).toHaveAttribute("aria-pressed", "true");
  await expect(filter(page, "All")).toHaveText("All 4");

  await page.getByRole("button", { name: "Clear search and filter" }).click();
  await expect(rows(page)).toHaveCount(4);
  await expect(search(page)).toHaveValue("");
  await expect(search(page)).toBeFocused();
  await expect(filter(page, "All")).toHaveAttribute("aria-pressed", "true");
  await expect(page).toHaveURL(/\/library$/);
});

test("a status change is optimistic, keyed, recounted and undoable", async ({ page }) => {
  const fake = new FakeApi();
  await openLibrary(page, fake);
  let release!: () => void;
  const held = new Promise<void>((resolve) => (release = resolve));
  await page.route(`**/api/v1/library/${showId(HOLLOW)}`, async (route) => {
    await held;
    return route.fallback();
  });
  await choose(page, "Hollow Orchard", "Dropped");
  // Optimistic before the server answers.
  await expect(menu(page, "Hollow Orchard")).toHaveAccessibleName("Hollow Orchard: Dropped. Change status or remove");
  await expect(filter(page, "Dropped")).toHaveText("Dropped 1");
  await expect(filter(page, "Watching")).toHaveText("Watching 1");
  release();
  const moved = toast(page, "Hollow Orchard is now Dropped.");
  await expect(moved).toBeVisible();
  expect(fake.writes()).toEqual([
    expect.objectContaining({ method: "PATCH", path: `/library/${showId(HOLLOW)}`, body: { status: "dropped", expectedRevision: 1 }, csrf: CSRF }),
  ]);
  expect(fake.writes()[0].key).toMatch(UUID);

  await moved.getByRole("button", { name: "Undo" }).click();
  await expect(toast(page, "Hollow Orchard is back to Watching.")).toBeVisible();
  await expect(menu(page, "Hollow Orchard")).toHaveAccessibleName("Hollow Orchard: Watching. Change status or remove");
  await expect(filter(page, "Dropped")).toHaveText("Dropped 0");
  expect(fake.get(HOLLOW)).toMatchObject({ status: "watching", revision: 3 });
});

test("a status change under a filter moves the row out of view; removal keeps history and undoes", async ({ page }) => {
  const fake = new FakeApi();
  await openLibrary(page, fake, "?status=watching");
  await expect(rows(page)).toHaveCount(2);
  await choose(page, "Lantern Coast", "On hold");
  await expect(rows(page)).toHaveCount(1);
  await expect(filter(page, "On hold")).toHaveText("On hold 2");

  await choose(page, "Hollow Orchard", "Remove from library");
  await expect(page.getByRole("heading", { name: "No shows are Watching." })).toBeVisible();
  await expect(filter(page, "All")).toHaveText("All 3");
  const removed = toast(page, "Removed Hollow Orchard. Your watch history is kept.");
  await expect(removed).toBeVisible();
  expect(fake.writes().at(-1)).toMatchObject({ method: "PUT", body: { saved: false, expectedRevision: 1 } });
  // Removal is only the saved flag: status and progress are untouched.
  expect(fake.get(HOLLOW)).toMatchObject({ saved: false, status: "watching", progress: { watched: 2 } });

  await removed.getByRole("button", { name: "Undo" }).click();
  await expect(toast(page, "Hollow Orchard is back in your library.")).toBeVisible();
  await expect(row(page, "Hollow Orchard")).toBeVisible();
  await expect(filter(page, "All")).toHaveText("All 4");
});

test("a rejected status change recovers the server state without overwriting a later edit", async ({ page }) => {
  const fake = new FakeApi();
  await openLibrary(page, fake);
  // Another device already put Hollow Orchard on hold, so this page's revision is stale.
  fake.elsewhere(HOLLOW, { status: "on_hold" });
  let release!: () => void;
  const held = new Promise<void>((resolve) => (release = resolve));
  await page.route(`**/api/v1/library/${showId(HOLLOW)}`, async (route) => {
    await held;
    return route.fallback();
  });
  await choose(page, "Hollow Orchard", "Dropped");
  // A later edit on another row lands while the first is in flight.
  await choose(page, "Paper Moons", "Watching");
  await expect(toast(page, "Paper Moons is now Watching.")).toBeVisible();
  release();

  await expect(failure(page, "Couldn’t move Hollow Orchard to Dropped.")).toContainText("Your progress changed somewhere else.");
  await expect(failure(page, "Couldn’t move Hollow Orchard to Dropped.")).not.toContainText("Safe server message.");
  // The row shows the authoritative state from a single-row read, not a rollback snapshot.
  await expect(menu(page, "Hollow Orchard")).toHaveAccessibleName("Hollow Orchard: On hold. Change status or remove");
  expect(fake.log.some((entry) => entry.method === "GET" && entry.path === `/shows/${showId(HOLLOW)}`)).toBe(true);
  // The later edit is kept on screen and on the server.
  await expect(menu(page, "Paper Moons")).toHaveAccessibleName("Paper Moons: Watching. Change status or remove");
  expect(fake.get(PAPER).status).toBe("watching");
  await expect(filter(page, "On hold")).toHaveText("On hold 2");
  await expect(filter(page, "Watching")).toHaveText("Watching 2");
  await expect(filter(page, "Plan to watch")).toHaveText("Plan to watch 0");

  // Retry starts from the recovered revision with a new key.
  await failure(page, "Couldn’t move Hollow Orchard to Dropped.").getByRole("button", { name: "Retry" }).click();
  await expect(toast(page, "Hollow Orchard is now Dropped.")).toBeVisible();
  const patches = fake.writes().filter((write) => write.path === `/library/${showId(HOLLOW)}`);
  expect(patches.map((write) => write.body)).toEqual([
    { status: "dropped", expectedRevision: 1 },
    { status: "dropped", expectedRevision: 2 },
  ]);
  expect(patches[1].key).not.toBe(patches[0].key);
});

test("a remove with an unknown outcome rolls back and retries with the same key", async ({ page }) => {
  const fake = new FakeApi();
  await openLibrary(page, fake);
  let failFirst = true;
  const keys: (string | null)[] = [];
  await page.route(`**/api/v1/library/${showId(NIGHT)}`, async (route, request) => {
    keys.push(request.headers()["idempotency-key"] ?? null);
    await later(300);
    if (failFirst) {
      failFirst = false;
      return route.fulfill({ status: 503, contentType: "application/json", body: "{}" });
    }
    return route.fallback();
  });
  await choose(page, "Night Shift", "Remove from library");
  await expect(row(page, "Night Shift")).toHaveCount(0);
  await expect(filter(page, "All")).toHaveText("All 3");
  const alert = failure(page, "Couldn’t remove Night Shift.");
  await expect(alert).toContainText("the change won’t be applied twice");
  await expect(row(page, "Night Shift")).toBeVisible();
  await expect(filter(page, "All")).toHaveText("All 4");

  await page.getByRole("button", { name: "Retry" }).click();
  await expect(toast(page, "Removed Night Shift. Your watch history is kept.")).toBeVisible();
  await expect(row(page, "Night Shift")).toHaveCount(0);
  // The first attempt failed at the boundary; the retry reuses its key and body.
  expect(keys).toHaveLength(2);
  expect(keys[0]).toMatch(UUID);
  expect(keys[1]).toBe(keys[0]);
  expect(fake.writes()).toEqual([expect.objectContaining({ method: "PUT", body: { saved: false, expectedRevision: 1 }, key: keys[0] })]);
});

test("keyboard: filters and the row menu work without a pointer", async ({ page }) => {
  const fake = new FakeApi();
  await openLibrary(page, fake);
  await filter(page, "On hold").focus();
  await page.keyboard.press("Enter");
  await expect(rows(page)).toHaveCount(1);
  await menu(page, "Night Shift").focus();
  await page.keyboard.press("Enter");
  await page.getByRole("menuitemcheckbox", { name: "Watching" }).focus();
  await page.keyboard.press("Enter");
  await expect(toast(page, "Night Shift is now Watching.")).toBeVisible();
  await expect(rows(page)).toHaveCount(0);
  await expect(page.getByRole("heading", { name: "No shows are On hold." })).toBeVisible();
});

test("load failure, retry and signed-out states use product copy", async ({ page }) => {
  const fake = new FakeApi();
  let mode: "fail" | "auth" | "ok" = "fail";
  await page.route("**/api/v1/**", (route, request) => {
    if (request.url().includes("/api/v1/library") && mode === "fail") return error(route, 503, "SERVICE_UNAVAILABLE");
    if (request.url().includes("/api/v1/library") && mode === "auth") return error(route, 401, "AUTH_REQUIRED");
    return fake.handle(route, request);
  });
  await page.goto("/library");
  const alert = page.getByRole("alert").filter({ hasText: "Your library didn’t load" });
  await expect(alert).toBeVisible();
  await expect(alert).not.toContainText("Safe server message.");
  mode = "ok";
  await page.getByRole("button", { name: "Retry" }).click();
  await expect(rows(page)).toHaveCount(4);

  mode = "auth";
  await page.reload();
  await expect(page.getByRole("heading", { name: "Sign in to see your library" })).toBeVisible();
  await expect(page.getByRole("link", { name: "Sign in" })).toHaveAttribute("href", "/auth/signin?returnTo=%2Flibrary");
});

test("more pages load on request and focus moves to the first new row", async ({ page }) => {
  const fake = new FakeApi();
  fake.pageSize = 2;
  await openLibrary(page, fake);
  await expect(rows(page)).toHaveCount(2);
  await expect(page.getByText("2 shows so far")).toBeVisible();
  await page.getByRole("button", { name: "Show more shows" }).click();
  await expect(rows(page)).toHaveCount(4);
  await expect(row(page, "Night Shift").getByRole("link")).toBeFocused();
  await expect(page.getByRole("button", { name: "Show more shows" })).toHaveCount(0);
  expect(fake.reads().map((read) => read.query.cursor)).toEqual([undefined, "2"]);
});

const scan = (page: Page) =>
  page.evaluate(async (source) => {
    if (!("axe" in window)) new Function(source)();
    const result = await (window as unknown as { axe: typeof axe }).axe.run(document, { runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21aa"] } });
    return result.violations.map((violation) => `${violation.id}: ${violation.nodes.map((node) => node.target).join(", ")}`);
  }, axe.source);
const overflowAt = (page: Page) => page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
const shot = (page: Page, width: number, name: string) =>
  width === 390 || width === 1440 ? page.screenshot({ path: `apps/web/src/features/library/verification/${name}-${width}.png`, fullPage: true }) : null;

for (const width of [320, 390, 860, 1440]) {
  test(`layout at ${width}px: no overflow, 44px targets, axe clean`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    const fake = new FakeApi();
    await openLibrary(page, fake);
    await expect(rows(page)).toHaveCount(4);
    expect(await overflowAt(page)).toBe(0);
    await expect(page.locator(width >= 860 ? "header nav" : "nav").first()).toBeVisible();
    const small = await page.locator(".sc-library button:visible, .sc-library a:visible").evaluateAll((elements) =>
      elements.filter((element) => element.getBoundingClientRect().height < 44).map((element) => element.textContent),
    );
    expect(small).toEqual([]);
    expect(await scan(page)).toEqual([]);
    await shot(page, width, "library");

    await menu(page, "Hollow Orchard").click();
    await expect(page.getByRole("menu")).toBeVisible();
    expect(await overflowAt(page)).toBe(0);
    expect(await scan(page)).toEqual([]);
    await page.keyboard.press("Escape");

    await page.goto("/library?status=dropped&q=orchard");
    await expect(page.getByRole("button", { name: "Clear search and filter" })).toBeVisible();
    expect(await overflowAt(page)).toBe(0);
    expect(await scan(page)).toEqual([]);
    await shot(page, width, "no-match");

    fake.entries = [];
    await page.goto("/library");
    await expect(page.getByRole("heading", { name: "Your library is empty" })).toBeVisible();
    expect(await overflowAt(page)).toBe(0);
    expect(await scan(page)).toEqual([]);
    await shot(page, width, "empty");
  });
}

test("loading and error states at 390 and 1440 for review", async ({ page }) => {
  const fake = new FakeApi();
  let release!: () => void;
  const held = new Promise<void>((resolve) => (release = resolve));
  await page.route("**/api/v1/**", async (route, request) => {
    if (!request.url().includes("/api/v1/library")) return fake.handle(route, request);
    await held;
    return route.abort().catch(() => undefined);
  });
  for (const width of [390, 1440]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/library");
    await expect(page.getByText("Loading your library…")).toBeVisible();
    await shot(page, width, "loading");
  }
  release();
  await page.unroute("**/api/v1/**");
  await page.route("**/api/v1/**", (route, request) =>
    request.url().includes("/api/v1/library") ? error(route, 503, "SERVICE_UNAVAILABLE") : fake.handle(route, request),
  );
  for (const width of [390, 1440]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/library");
    await expect(page.getByRole("alert").filter({ hasText: "Your library didn’t load" })).toBeVisible();
    expect(await scan(page)).toEqual([]);
    await shot(page, width, "error");
  }
});
