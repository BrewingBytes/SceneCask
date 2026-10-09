import axe from "axe-core";
import { expect, test, type Page, type Request, type Route } from "@playwright/test";

/**
 * Component race/failure suite for R19. The production /discover route runs in an isolated
 * fixture app; responses are supplied at the /api/v1 network boundary, so the generated client,
 * CSRF bootstrap and Idempotency-Key headers are exercised. This is not live integration (R24).
 */

const CSRF = "csrf-fixture-token";
const SHOW_2024 = "10000000-0000-4000-8000-000000002024";
const SHOW_2011 = "10000000-0000-4000-8000-000000002011";
const ACTION = "20000000-0000-4000-8000-000000000001";
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

const result = (providerId: number, title: string, year: number | null, genres: string[]) => ({
  provider: "tmdb",
  providerId,
  title,
  year,
  genres,
  posterUrl: null,
});
const HOLLOW = [
  result(2024, "Hollow Orchard", 2024, ["Drama", "Mystery"]),
  result(2011, "Hollow Orchard", 2011, ["Documentary"]),
  result(77, "Hollow Crown", null, []),
];
const showIds: Record<number, string> = { 2024: SHOW_2024, 2011: SHOW_2011, 77: "10000000-0000-4000-8000-000000000077" };

const libraryItem = (showId: string, saved: boolean, revision: number, watched = 0) => ({
  showId,
  title: "Hollow Orchard",
  year: 2011,
  posterUrl: null,
  saved,
  status: "plan_to_watch",
  revision,
  progress: { watched, total: 6, percent: 0, state: "in_progress", nextEpisode: null, outOfOrder: false, releaseInfoIncomplete: false },
});
const showDto = (id: string, library: ReturnType<typeof libraryItem> | null) => ({
  id,
  title: "Hollow Orchard",
  year: 2011,
  genres: ["Documentary"],
  synopsis: "A series.",
  posterUrl: null,
  status: "ended",
  catalogRevision: 1,
  metadataStale: false,
  releaseInfoIncomplete: false,
  seasons: [],
  library,
  trackingRevision: 0,
});
const mutation = (showId: string) => ({
  actionId: ACTION,
  undoUntil: "2026-10-09T12:10:00Z",
  changed: 1,
  library: libraryItem(showId, true, 1),
  trackingRevision: 0,
  episodes: [],
});
const json = (route: Route, body: unknown, status = 200, headers: Record<string, string> = {}) =>
  route.fulfill({ status, contentType: "application/json", headers, body: JSON.stringify(body) });
const error = (route: Route, status: number, code: string) =>
  json(route, { error: { code, message: "Safe message.", requestId: "30000000-0000-4000-8000-000000000001" } }, status);
const later = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

type Handler = (route: Route, request: Request) => Promise<unknown> | unknown;
interface Api {
  search?: Handler;
  import?: Handler;
  show?: Handler;
  save?: Handler;
  undo?: Handler;
}
interface Log {
  searches: URLSearchParams[];
  imports: unknown[];
  saves: { url: string; body: unknown; key: string | null; csrf: string | null }[];
  undos: { key: string | null }[];
}

/** Default happy-path API; individual tests override one endpoint at a time. */
async function mockApi(page: Page, api: Api = {}): Promise<Log> {
  const log: Log = { searches: [], imports: [], saves: [], undos: [] };
  await page.route("**/api/v1/**", async (route, request) => {
    const url = new URL(request.url());
    const path = url.pathname.replace("/api/v1", "");
    if (path === "/session") return json(route, { user: null, csrfToken: CSRF });
    if (path === "/shows/search") {
      log.searches.push(url.searchParams);
      if (api.search) return api.search(route, request);
      const q = url.searchParams.get("q")!.toLowerCase();
      return json(route, { items: HOLLOW.filter((item) => item.title.toLowerCase().includes(q)), page: 1, totalPages: 1 });
    }
    if (path === "/shows/import") {
      const body = request.postDataJSON() as { providerId: number };
      log.imports.push(body);
      if (api.import) return api.import(route, request);
      return json(route, { showId: showIds[body.providerId] });
    }
    if (path.startsWith("/library/")) {
      log.saves.push({
        url: path,
        body: request.postDataJSON(),
        key: request.headers()["idempotency-key"] ?? null,
        csrf: request.headers()["x-csrf-token"] ?? null,
      });
      if (api.save) return api.save(route, request);
      return json(route, mutation(path.split("/")[2]));
    }
    if (path.startsWith("/actions/")) {
      log.undos.push({ key: request.headers()["idempotency-key"] ?? null });
      if (api.undo) return api.undo(route, request);
      return json(route, { actionId: ACTION, reverted: 1, skipped: 0, library: libraryItem(SHOW_2011, false, 2) });
    }
    if (path.startsWith("/shows/")) {
      if (api.show) return api.show(route, request);
      return json(route, showDto(path.split("/")[2], null));
    }
    return error(route, 404, "NOT_FOUND");
  });
  return log;
}

const search = (page: Page) => page.getByRole("searchbox", { name: "Search TV shows" });
const results = (page: Page) => page.getByRole("list", { name: /^Search results/ });
const addButton = (page: Page, year: number) =>
  page.getByRole("button", { name: `Add Hollow Orchard (${year}) to Plan to watch` });

async function searchFor(page: Page, q: string) {
  await search(page).fill(q);
  await expect(results(page)).toBeVisible();
}
const addError = (page: Page) => page.getByRole("alert").filter({ hasText: "Couldn’t add Hollow Orchard." });
const added = (page: Page) => page.getByRole("status").filter({ hasText: "Added Hollow Orchard to Plan to watch." });

/** Open Discover, search the ambiguous title and add the 2011 show. */
async function addFromSearch(page: Page) {
  await page.goto("/discover");
  await searchFor(page, "hollow orchard");
  await addButton(page, 2011).click();
}

test.beforeEach(async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
});

test("a slow earlier query never overwrites a later one, and keystrokes are debounced", async ({ page }) => {
  const aborted: string[] = [];
  page.on("requestfailed", (request) => {
    if (request.url().includes("/shows/search")) aborted.push(new URL(request.url()).searchParams.get("q")!);
  });
  await mockApi(page, {
    search: async (route, request) => {
      const q = new URL(request.url()).searchParams.get("q")!;
      if (q === "hollow") {
        // The earlier query answers last, with results that must never appear.
        await later(1500);
        return json(route, { items: [result(9, "Stale Hollow", 1999, ["Drama"])], page: 1, totalPages: 1 }).catch(() => {});
      }
      return json(route, { items: HOLLOW.slice(0, 2), page: 1, totalPages: 1 });
    },
  });
  await page.goto("/discover");
  const requested: string[] = [];
  page.on("request", (request) => {
    if (request.url().includes("/shows/search")) requested.push(new URL(request.url()).searchParams.get("q")!);
  });
  await search(page).pressSequentially("hollow", { delay: 40 });
  await expect(page.getByRole("status").filter({ hasText: "Searching…" })).toBeVisible();
  await expect.poll(() => requested).toEqual(["hollow"]);
  await search(page).pressSequentially(" orchard", { delay: 40 });
  await expect(results(page).getByRole("listitem")).toHaveCount(2);
  // Wait beyond the stale response's delay: the later results stay authoritative.
  await page.waitForTimeout(1800);
  await expect(page.getByText("Stale Hollow")).toHaveCount(0);
  await expect(results(page).getByRole("listitem")).toHaveCount(2);
  expect(requested).toEqual(["hollow", "hollow orchard"]);
  expect(aborted).toEqual(["hollow"]);
  await expect(page).toHaveURL(/\/discover\?q=hollow\+orchard$/);
});

test("ambiguous titles show year, genres and poster; the chosen year is imported and saved", async ({ page }) => {
  const log = await mockApi(page);
  await page.goto("/discover");
  await searchFor(page, "hollow orchard");
  await expect(page.getByRole("status").filter({ hasText: "2 shows · check the year and poster" })).toBeVisible();
  const rows = results(page).getByRole("listitem");
  await expect(rows.nth(0)).toContainText("Hollow Orchard (2024)");
  await expect(rows.nth(0)).toContainText("TV · Drama, Mystery");
  await expect(rows.nth(1)).toContainText("Hollow Orchard (2011)");
  await expect(rows.nth(1)).toContainText("TV · Documentary");
  await expect(rows.locator(".sc-art")).toHaveCount(2);

  await addButton(page, 2011).click();
  await expect(added(page)).toBeVisible();
  expect(log.imports).toEqual([{ providerId: 2011 }]);
  expect(log.saves).toHaveLength(1);
  expect(log.saves[0].url).toBe(`/library/${SHOW_2011}`);
  expect(log.saves[0].body).toEqual({ saved: true, status: "plan_to_watch", expectedRevision: 0 });
  expect(log.saves[0].key).toMatch(UUID);
  expect(log.saves[0].csrf).toBe(CSRF);
  await expect(page.getByRole("button", { name: "Hollow Orchard (2011) is in your library. Open it" })).toBeVisible();
  // The other same-titled show is untouched.
  await expect(addButton(page, 2024)).toBeVisible();
});

test("selecting a result imports it and navigates by application ID", async ({ page }) => {
  const log = await mockApi(page);
  await page.goto("/discover?q=hollow%20orchard");
  await expect(search(page)).toHaveValue("hollow orchard");
  await page.getByRole("button", { name: "Open Hollow Orchard (2024), TV, Drama, Mystery" }).click();
  await expect(page).toHaveURL(`/shows/${SHOW_2024}`);
  await expect(page.getByRole("heading", { name: `Show ${SHOW_2024}` })).toBeVisible();
  expect(log.imports).toEqual([{ providerId: 2024 }]);
});

test("Back from an opened result restores the query and its results", async ({ page }) => {
  await mockApi(page);
  await page.goto("/discover");
  await searchFor(page, "hollow orchard");
  await expect(page).toHaveURL(/\/discover\?q=hollow\+orchard$/);
  await page.getByRole("button", { name: "Open Hollow Orchard (2024), TV, Drama, Mystery" }).click();
  await expect(page).toHaveURL(`/shows/${SHOW_2024}`);
  await page.goBack();
  await expect(search(page)).toHaveValue("hollow orchard");
  await expect(results(page).getByRole("listitem")).toHaveCount(2);
});

test("search failure keeps the query and Retry recovers", async ({ page }) => {
  let fail = true;
  await mockApi(page, {
    search: (route) =>
      fail ? error(route, 502, "PROVIDER_UNAVAILABLE") : json(route, { items: HOLLOW, page: 1, totalPages: 1 }),
  });
  await page.goto("/discover");
  await search(page).fill("hollow");
  const alert = page.getByRole("alert").filter({ hasText: "Show search isn’t responding" });
  await expect(alert).toContainText("Our show listings provider didn’t answer.");
  await expect(alert).not.toContainText("Safe message.");
  await expect(search(page)).toHaveValue("hollow");
  fail = false;
  await page.getByRole("button", { name: "Retry" }).click();
  await expect(results(page).getByRole("listitem")).toHaveCount(3);
  await expect(search(page)).toHaveValue("hollow");
});

test("import failure keeps the query; Retry adds exactly once", async ({ page }) => {
  let fail = true;
  const log = await mockApi(page, {
    import: (route) => (fail ? error(route, 502, "PROVIDER_UNAVAILABLE") : json(route, { showId: SHOW_2011 })),
  });
  await addFromSearch(page);
  const alert = addError(page);
  await expect(alert).toBeVisible();
  expect(log.saves).toHaveLength(0);
  await expect(search(page)).toHaveValue("hollow orchard");
  await expect(addButton(page, 2011)).toBeVisible();
  fail = false;
  await alert.getByRole("button", { name: "Retry" }).click();
  await expect(added(page)).toBeVisible();
  expect(log.imports).toHaveLength(2);
  expect(log.saves).toHaveLength(1);
});

test("an unknown save outcome is retried with the same Idempotency-Key and body", async ({ page }) => {
  let attempts = 0;
  const log = await mockApi(page, {
    save: (route, request) =>
      ++attempts === 1 ? route.abort("connectionreset") : json(route, mutation(new URL(request.url()).pathname.split("/").pop()!)),
  });
  await addFromSearch(page);
  const alert = addError(page);
  await expect(alert).toContainText("Check your connection");
  await expect(search(page)).toHaveValue("hollow orchard");
  await alert.getByRole("button", { name: "Retry" }).click();
  await expect(added(page)).toBeVisible();
  expect(log.saves).toHaveLength(2);
  expect(log.saves[1].key).toBe(log.saves[0].key);
  expect(log.saves[1].body).toEqual(log.saves[0].body);
  // The import was reused, not repeated.
  expect(log.imports).toHaveLength(1);
});

test("a revision conflict refreshes the library entry and uses a new key", async ({ page }) => {
  let conflict = true;
  let shows = 0;
  const log = await mockApi(page, {
    show: (route) => json(route, showDto(SHOW_2011, ++shows === 1 ? null : libraryItem(SHOW_2011, false, 3, 2))),
    save: (route) => (conflict ? ((conflict = false), error(route, 409, "REVISION_CONFLICT")) : json(route, mutation(SHOW_2011))),
  });
  await addFromSearch(page);
  const alert = page.getByRole("alert").filter({ hasText: "Your library changed somewhere else." });
  await alert.getByRole("button", { name: "Retry" }).click();
  await expect(page.getByRole("status").filter({ hasText: "Hollow Orchard is back in your library with your progress." })).toBeVisible();
  expect(log.saves).toHaveLength(2);
  expect(log.saves[1].key).not.toBe(log.saves[0].key);
  // An existing entry keeps its manual status and history.
  expect(log.saves[1].body).toEqual({ saved: true, expectedRevision: 3 });
});

test("a show already in the library is not saved again", async ({ page }) => {
  const log = await mockApi(page, { show: (route) => json(route, showDto(SHOW_2011, libraryItem(SHOW_2011, true, 4))) });
  await addFromSearch(page);
  await expect(page.getByRole("status").filter({ hasText: "Hollow Orchard is already in your library." })).toBeVisible();
  expect(log.saves).toHaveLength(0);
});

test("repeated activation while busy starts one save", async ({ page }) => {
  const log = await mockApi(page, {
    save: async (route) => {
      await later(500);
      return json(route, mutation(SHOW_2011));
    },
  });
  await page.goto("/discover");
  await searchFor(page, "hollow orchard");
  const button = addButton(page, 2011);
  await button.click();
  await expect(button).toHaveAttribute("aria-busy", "true");
  await button.click({ force: true });
  await button.press("Enter");
  await expect(page.getByRole("status").filter({ hasText: "Added Hollow Orchard" })).toBeVisible();
  expect(log.imports).toHaveLength(1);
  expect(log.saves).toHaveLength(1);
});

test("Undo uses the mutation's action and restores Add", async ({ page }) => {
  const log = await mockApi(page);
  await addFromSearch(page);
  await page.getByRole("button", { name: "Undo" }).click();
  await expect(page.getByRole("status").filter({ hasText: "Removed Hollow Orchard from your library." })).toBeVisible();
  expect(log.undos).toHaveLength(1);
  expect(log.undos[0].key).toMatch(UUID);
  await expect(addButton(page, 2011)).toBeVisible();
});

test("pagination appends the next page and moves focus to the first new result", async ({ page }) => {
  let failMore = true;
  const log = await mockApi(page, {
    search: (route, request) => {
      const pageNumber = Number(new URL(request.url()).searchParams.get("page"));
      if (pageNumber === 1) return json(route, { items: HOLLOW.slice(0, 2), page: 1, totalPages: 2 });
      if (failMore) return (failMore = false), error(route, 503, "SERVICE_UNAVAILABLE");
      return json(route, { items: [HOLLOW[1], HOLLOW[2]], page: 2, totalPages: 2 });
    },
  });
  await page.goto("/discover");
  await searchFor(page, "hollow");
  await expect(page.getByRole("status").filter({ hasText: "2 shows so far" })).toBeVisible();
  await page.getByRole("button", { name: "Show more results" }).click();
  await expect(page.getByRole("alert").filter({ hasText: "More results didn’t load" })).toBeVisible();
  await expect(results(page).getByRole("listitem")).toHaveCount(2);
  await page.getByRole("button", { name: "Retry" }).click();
  // Duplicate provider IDs across pages are not repeated.
  await expect(results(page).getByRole("listitem")).toHaveCount(3);
  await expect(page.getByRole("button", { name: "Open Hollow Crown (year unknown), TV, Genre unknown" })).toBeFocused();
  await expect(page.getByRole("button", { name: "Show more results" })).toHaveCount(0);
  expect(log.searches.map((params) => params.get("page"))).toEqual(["1", "2", "2"]);
});

test("empty, short, no-results and signed-out states", async ({ page }) => {
  let status = 200;
  await mockApi(page, {
    search: (route) => (status === 401 ? error(route, 401, "AUTH_REQUIRED") : json(route, { items: [], page: 1, totalPages: 0 })),
  });
  await page.goto("/discover");
  await expect(page.getByRole("heading", { name: "Start with a show you’re watching" })).toBeVisible();
  await search(page).fill("h");
  await expect(page.getByRole("status").filter({ hasText: "Type at least 2 characters." })).toBeVisible();
  await search(page).fill("zzz");
  await expect(page.getByRole("heading", { name: "No shows match “zzz”" })).toBeVisible();
  status = 401;
  await search(page).fill("zzzz");
  await expect(page.getByRole("heading", { name: "Sign in to search shows" })).toBeVisible();
  await expect(page.getByRole("link", { name: "Sign in" })).toHaveAttribute("href", "/auth/signin");
  await expect(search(page)).toHaveValue("zzzz");
});

test("keyboard reaches the search, each result and its Add action in order", async ({ page }) => {
  await mockApi(page);
  await page.goto("/discover");
  await search(page).focus();
  await search(page).fill("hollow orchard");
  await expect(results(page)).toBeVisible();
  await page.keyboard.press("Tab");
  await expect(page.getByRole("button", { name: /^Open Hollow Orchard \(2024\)/ })).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(addButton(page, 2024)).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(page.getByRole("button", { name: /^Open Hollow Orchard \(2011\)/ })).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(addButton(page, 2011)).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("status").filter({ hasText: "Added Hollow Orchard" })).toBeVisible();
  // The control keeps focus as it becomes the In library action.
  await expect(page.getByRole("button", { name: "Hollow Orchard (2011) is in your library. Open it" })).toBeFocused();
});

async function audit(page: Page) {
  await page.addScriptTag({ content: axe.source });
  return page.evaluate(async () => {
    const runner = (window as unknown as {
      axe: { run: (options: object) => Promise<{ violations: { id: string; nodes: { target: string[] }[] }[] }> };
    }).axe;
    const { violations } = await runner.run({ runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21aa"] } });
    return violations.map(({ id, nodes }) => ({ id, targets: nodes.map((node) => node.target.join(" ")) }));
  });
}

for (const width of [320, 390, 859, 860, 1440])
  test(`responsive results, targets and screenshots at ${width}`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await mockApi(page, {
      search: (route, request) =>
        new URL(request.url()).searchParams.get("q") === "hollow"
          ? json(route, { items: HOLLOW, page: 1, totalPages: 2 })
          : error(route, 502, "PROVIDER_UNAVAILABLE"),
      import: (route) => error(route, 502, "PROVIDER_UNAVAILABLE"),
    });
    await page.goto("/discover");
    await page.evaluate(() => document.fonts.ready);
    const navigation = page.getByRole("navigation", { name: "Main navigation" });
    const navBox = (await navigation.boundingBox())!;
    // Bottom navigation below 860px, header navigation from 860px.
    expect(navBox.y > 450).toBe(width < 860);
    const shots = [390, 1440].includes(width);
    if (shots) await page.screenshot({ path: `test-results/r19/empty-${width}.png` });

    await searchFor(page, "hollow");
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    for (const control of await page.locator(".sc-discover button, .sc-discover input").all()) {
      const box = (await control.boundingBox())!;
      expect(box.width).toBeGreaterThanOrEqual(44);
      expect(box.height).toBeGreaterThanOrEqual(44);
    }
    for (const row of await results(page).getByRole("listitem").all()) {
      const box = (await row.boundingBox())!;
      expect(box.x + box.width).toBeLessThanOrEqual(width);
    }
    if (shots) {
      expect(await audit(page), "results").toEqual([]);
      await page.screenshot({ path: `test-results/r19/results-${width}.png`, fullPage: true });
      await addButton(page, 2011).click();
      await expect(addError(page)).toBeVisible();
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
      expect(await audit(page), "add failure").toEqual([]);
      await page.screenshot({ path: `test-results/r19/add-error-${width}.png` });
      await search(page).fill("orchard");
      await expect(page.getByRole("alert").filter({ hasText: "Show search isn’t responding" })).toBeVisible();
      expect(await audit(page), "search failure").toEqual([]);
      await page.screenshot({ path: `test-results/r19/search-error-${width}.png` });
    }
  });

test("loading skeleton and success toast screenshots", async ({ page }) => {
  for (const width of [390, 1440]) {
    await page.setViewportSize({ width, height: 900 });
    let release!: () => void;
    const gate = new Promise<void>((resolve) => (release = resolve));
    await page.unrouteAll({ behavior: "ignoreErrors" });
    await mockApi(page, {
      search: async (route) => {
        await gate;
        return json(route, { items: HOLLOW, page: 1, totalPages: 1 });
      },
    });
    await page.goto("/discover");
    await search(page).fill("hollow");
    await expect(page.getByRole("status").filter({ hasText: "Searching…" })).toBeVisible();
    await expect(page.locator(".sc-skeleton").first()).toBeVisible();
    await page.screenshot({ path: `test-results/r19/loading-${width}.png` });
    release();
    await addButton(page, 2011).click();
    await expect(page.getByRole("button", { name: "Undo" })).toBeVisible();
    await page.screenshot({ path: `test-results/r19/added-${width}.png` });
  }
});
