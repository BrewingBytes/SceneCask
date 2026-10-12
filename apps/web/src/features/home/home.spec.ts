import axe from "axe-core";
import { expect, test, type Page, type Request, type Route } from "@playwright/test";
import {
  error,
  FakeTrackingApi,
  SECRET,
  serve,
  type FakeBackend,
  type FakeEpisode,
  type FakeLibrary,
  type Logged,
  type ReleaseState,
} from "../tracking/fixture-api";

/**
 * Component suite for R23. The production /home and / routes run in an isolated fixture app; a
 * stateful fake of the C05 Home, show, progress, catch-up and undo API answers at the /api/v1
 * network boundary, so the generated client, CSRF bootstrap, Idempotency-Key headers and
 * revisions are exercised. This is not live integration (R24 owns that gate).
 */

const showId = (n: number) => `10000000-0000-4000-8000-${String(n).padStart(12, "0")}`;
/** Episode ids unique per show, so one fake can serve several shows. */
const ep = (show: number, season: number, number: number, state: ReleaseState = "released", date: string | null = "2026-01-01"): FakeEpisode => ({
  id: `3000000${show}-0000-4000-8000-${String(season).padStart(6, "0")}${String(number).padStart(6, "0")}`,
  season,
  number,
  state,
  date: state === "unknown" ? null : date,
});
const watching: FakeLibrary = { saved: true, status: "watching", revision: 1 };

/** A C05 Home over several single-show fakes; shows are listed in saved_at descending order. */
class FakeHome implements FakeBackend {
  log: Logged[] = [];
  idempotent = new Map<string, { body: string; response: unknown; status: number }>();
  shows: FakeTrackingApi[] = [];

  add(n: number, title: string, episodes: FakeEpisode[], watched: string[] = [], library: FakeLibrary | null = { ...watching }) {
    const show = new FakeTrackingApi(episodes, watched, library, { id: showId(n), title });
    this.shows.push(show);
    return show;
  }
  home() {
    const saved = this.shows.filter((show) => show.library?.saved);
    const items = saved.filter((show) => show.library!.status === "watching").map((show) => show.libraryItem());
    return {
      upNext: items.filter((item) => item.progress.state === "in_progress"),
      caughtUp: items.filter((item) => item.progress.state !== "in_progress"),
      libraryEmpty: saved.length === 0,
    };
  }
  owner(path: string) {
    const [, kind, id] = path.split("/");
    if (kind === "shows" || kind === "library") return this.shows.find((show) => show.show.id === id);
    if (kind === "actions") return this.shows.find((show) => show.actions.has(id));
    const episode = path.split("/").at(-1)!;
    return this.shows.find((show) => show.byId.has(episode));
  }
  handle(route: Route, request: Request) {
    return serve(this, route, request);
  }
  read(path: string, url: URL) {
    if (path === "/home") return this.home();
    return this.owner(path)?.read(path, url) ?? null;
  }
  mutate(method: string, path: string, body: Record<string, unknown> | undefined): [number, unknown] {
    return this.owner(path)?.mutate(method, path, body) ?? [404, "NOT_FOUND"];
  }
  writes() {
    return this.log.filter((entry) => entry.method !== "GET" && !entry.path.endsWith("/preview"));
  }
  homeReads() {
    return this.log.filter((entry) => entry.method === "GET" && entry.path === "/home");
  }
}

const HOLLOW = 1;
const LANTERN = 2;
const NIGHT = 3;
const PAPER = 4;

/** Hollow: E1 and E3 watched, so E2 is next. Lantern: one released gap. Night: caught up, E4 dated. */
function returning() {
  const fake = new FakeHome();
  const hollow = [ep(HOLLOW, 1, 1), ep(HOLLOW, 1, 2), ep(HOLLOW, 1, 3), ep(HOLLOW, 1, 4)];
  fake.add(HOLLOW, "Hollow Orchard", hollow, [hollow[0].id, hollow[2].id]);
  const lantern = [ep(LANTERN, 1, 1), ep(LANTERN, 1, 2)];
  fake.add(LANTERN, "Lantern Coast", lantern, [lantern[0].id]);
  fake.add(NIGHT, "Night Shift", ...caughtUpNight());
  fake.add(PAPER, "Paper Moons", [ep(PAPER, 1, 1)], [], { saved: true, status: "plan_to_watch", revision: 1 });
  return fake;
}
function caughtUpNight(): [FakeEpisode[], string[]] {
  const night = [ep(NIGHT, 1, 1), ep(NIGHT, 2, 1), ep(NIGHT, 2, 2), ep(NIGHT, 2, 3, "future", "2026-11-12")];
  return [night, [night[0].id, night[1].id, night[2].id]];
}
/** Every Watching show is caught up. */
function allCaught() {
  const fake = new FakeHome();
  fake.add(NIGHT, "Night Shift", ...caughtUpNight());
  const paper = [ep(PAPER, 1, 1), ep(PAPER, 1, 2, "unknown")];
  fake.add(PAPER, "Paper Moons", paper, [paper[0].id]);
  return fake;
}

async function openHome(page: Page, fake: FakeHome) {
  await page.route("**/api/v1/**", (route, request) => fake.handle(route, request));
  await page.goto("/home");
  await expect(page.getByRole("heading", { level: 1, name: "Up next" })).toBeVisible();
  await expect(page.getByText("Loading your shows…")).toHaveCount(0);
}
const featured = (page: Page) => page.getByRole("article");
const upRows = (page: Page) => page.getByRole("list", { name: "More shows up next" }).getByRole("listitem");
const caught = (page: Page) => page.getByRole("region", { name: "Waiting on new episodes" });
const toast = (page: Page, text: string) => page.getByRole("status").filter({ hasText: text });
const failure = (page: Page, text: string) => page.getByRole("alert").filter({ hasText: text });

test.beforeEach(async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
});

test("E1 and E3 watched: the featured next episode is E2, by code only", async ({ page }) => {
  const fake = returning();
  await openHome(page, fake);
  await expect(featured(page).getByRole("heading", { name: "Hollow Orchard" })).toBeVisible();
  await expect(featured(page)).toContainText("S1 E2");
  await expect(featured(page)).toContainText("2 of 4 episodes watched · some out of order");
  await expect(featured(page)).toContainText("Title hidden until you watch");
  await expect(featured(page).getByRole("button", { name: "Mark S1 E2 watched" })).toBeVisible();
  await expect(featured(page).getByRole("link", { name: "View episode" })).toHaveAttribute("href", `/episodes/${ep(HOLLOW, 1, 2).id}`);
  await expect(featured(page).getByRole("progressbar", { name: "Hollow Orchard progress" })).toHaveAttribute("aria-valuenow", "50");

  // The rest follow in Home order; a show that is not Watching never appears.
  await expect(upRows(page)).toHaveCount(1);
  await expect(upRows(page).first()).toContainText("Lantern Coast");
  await expect(upRows(page).first()).toContainText("S1 E2 next");
  await expect(page.getByText("Paper Moons")).toHaveCount(0);
  // No protected episode text reaches Home: only episode reads could carry it, and none ran.
  await expect(page.locator("body")).not.toContainText(SECRET);
  expect(fake.log.filter((entry) => entry.path.startsWith("/episodes/"))).toEqual([]);
});

test("marking the featured episode is keyed, refetches Home and can be undone", async ({ page }) => {
  const fake = returning();
  await openHome(page, fake);
  await featured(page).getByRole("button", { name: "Mark S1 E2 watched" }).click();
  await expect(toast(page, "Marked S1 E2 watched.")).toBeVisible();
  await expect(featured(page).getByRole("button", { name: "Mark S1 E4 watched" })).toBeVisible();
  await expect(featured(page)).toContainText("3 of 4 episodes watched");

  const [write] = fake.writes();
  expect(write).toMatchObject({ method: "PUT", path: `/progress/episodes/${ep(HOLLOW, 1, 2).id}`, body: { watched: true, expectedRevision: 0 } });
  expect(write.key).toBeTruthy();
  expect(write.csrf).toBeTruthy();

  await toast(page, "Marked S1 E2 watched.").getByRole("button", { name: "Undo" }).click();
  await expect(toast(page, "S1 E2 is unwatched again.")).toBeVisible();
  await expect(featured(page).getByRole("button", { name: "Mark S1 E2 watched" })).toBeVisible();
  expect(fake.writes().map((entry) => entry.method + " " + entry.path.split("/")[1])).toEqual(["PUT progress", "POST actions"]);
  await expect(page.locator("body")).not.toContainText(SECRET);
});

test("marking a show's last released episode moves it to Waiting and keeps keyboard position", async ({ page }) => {
  const fake = returning();
  await openHome(page, fake);
  const mark = upRows(page).first().getByRole("button", { name: "Mark S1 E2 of Lantern Coast watched" });
  await mark.focus();
  await page.keyboard.press("Enter");
  await expect(toast(page, "Marked S1 E2 watched.")).toBeVisible();
  await expect(upRows(page)).toHaveCount(0);
  await expect(caught(page).getByRole("listitem").filter({ hasText: "Lantern Coast" })).toContainText("Caught up");
  await expect(page.getByRole("heading", { level: 1, name: "Up next" })).toBeFocused();
});

test("a caught-up show lists what it waits for; a new release moves it back on refetch", async ({ page }) => {
  const fake = allCaught();
  await openHome(page, fake);
  await expect(page.getByRole("heading", { name: "✓ You’re caught up" })).toBeVisible();
  await expect(page.getByRole("link", { name: "Find something new" })).toHaveAttribute("href", "/discover");
  const night = caught(page).getByRole("listitem").filter({ hasText: "Night Shift" });
  await expect(night).toContainText("S2 E3 · Airs Nov 12, 2026 (estimated)");
  await expect(night).toContainText("Caught up");
  await expect(caught(page).getByRole("listitem").filter({ hasText: "Paper Moons" })).toContainText("S1 E2 · Release date unknown");
  await expect(caught(page)).toContainText("use midnight UTC");

  // The episode airs while Home is open; returning to the tab refetches the authoritative sections.
  fake.shows[0].byId.get(ep(NIGHT, 2, 3).id)!.state = "released";
  const reads = fake.homeReads().length;
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect(featured(page).getByRole("heading", { name: "Night Shift" })).toBeVisible();
  await expect(featured(page).getByRole("button", { name: "Mark S2 E3 watched" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "✓ You’re caught up" })).toHaveCount(0);
  expect(fake.homeReads().length).toBeGreaterThan(reads);
});

test("returning to the tab rereads caught-up listings: a failed read retries, a new date shows", async ({ page }) => {
  const fake = allCaught();
  let failNight = true;
  await page.route("**/api/v1/**", (route, request) => {
    if (failNight && new URL(request.url()).pathname.endsWith(`/shows/${showId(NIGHT)}`)) return error(route, 503, "SERVICE_UNAVAILABLE");
    return fake.handle(route, request);
  });
  await page.goto("/home");
  const night = caught(page).getByRole("listitem").filter({ hasText: "Night Shift" });
  const paper = caught(page).getByRole("listitem").filter({ hasText: "Paper Moons" });
  await expect(paper).toContainText("S1 E2 · Release date unknown");
  await expect(night).toContainText("Release info incomplete");

  // Progress is unchanged, so only rereading the listings can pick these up.
  failNight = false;
  Object.assign(fake.shows[1].byId.get(ep(PAPER, 1, 2).id)!, { state: "future", date: "2026-12-03" });
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect(night).toContainText("S2 E3 · Airs Nov 12, 2026 (estimated)");
  await expect(paper).toContainText("S1 E2 · Airs Dec 3, 2026 (estimated)");
});

test("an empty library keeps the solo search journey; no social area is required", async ({ page }) => {
  const fake = new FakeHome();
  await openHome(page, fake);
  await expect(page.getByRole("heading", { name: "Nothing to track yet" })).toBeVisible();
  await expect(page.getByText("From people you follow")).toHaveCount(0);
  await expect(page.getByRole("navigation", { name: "Main navigation" }).getByRole("link", { name: "Home" })).toHaveAttribute("aria-current", "page");
  await page.getByRole("link", { name: "Find a show" }).click();
  await expect(page).toHaveURL(/\/discover$/);
});

test("saved shows but none Watching: Choose a show to watch links Library", async ({ page }) => {
  const fake = new FakeHome();
  fake.add(PAPER, "Paper Moons", [ep(PAPER, 1, 1)], [], { saved: true, status: "plan_to_watch", revision: 1 });
  fake.add(NIGHT, "Night Shift", [ep(NIGHT, 1, 1)], [], { saved: true, status: "on_hold", revision: 1 });
  await openHome(page, fake);
  await expect(page.getByRole("heading", { name: "Choose a show to watch" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Nothing to track yet" })).toHaveCount(0);
  await page.getByRole("link", { name: "Go to your library" }).click();
  await expect(page).toHaveURL(/\/library$/);
});

test("catch-up from the featured show uses the shared sheet and refreshes Home", async ({ page }) => {
  const fake = returning();
  await openHome(page, fake);
  await featured(page).getByRole("button", { name: "Watched more? Catch up" }).click();
  const sheet = page.getByRole("dialog");
  await expect(sheet).toBeVisible();
  await sheet.getByRole("button", { name: "S1 E4", exact: true }).click();
  await expect(sheet).toContainText("Marks 2 episodes watched: S1 E2, E4.");
  await expect(sheet).toContainText("Already watched, left as they are: S1 E1, E3");
  await sheet.getByRole("button", { name: "Mark 2 watched" }).click();
  await expect(sheet).toBeHidden();
  await expect(toast(page, "Marked 2 episodes watched through S1 E4.")).toBeVisible();
  await expect(featured(page).getByRole("heading", { name: "Lantern Coast" })).toBeVisible();
  await expect(caught(page).getByRole("listitem").filter({ hasText: "Hollow Orchard" })).toBeVisible();
  expect(fake.writes().map((entry) => entry.path)).toEqual([`/shows/${showId(HOLLOW)}/catch-up`]);
});

test("a mark with an unknown outcome keeps Home and retries with the same key", async ({ page }) => {
  const fake = returning();
  let fail = true;
  await page.route("**/api/v1/**", (route, request) => {
    if (fail && request.method() === "PUT") {
      fail = false;
      fake.log.push({ method: "PUT", path: "failed", query: {}, body: request.postDataJSON(), key: request.headers()["idempotency-key"] ?? null, csrf: null });
      return error(route, 503, "SERVICE_UNAVAILABLE");
    }
    return fake.handle(route, request);
  });
  await page.goto("/home");
  await featured(page).getByRole("button", { name: "Mark S1 E2 watched" }).click();
  const alert = failure(page, "Couldn’t mark S1 E2 watched.");
  await expect(alert).toBeVisible();
  await expect(featured(page).getByRole("button", { name: "Mark S1 E2 watched" })).toBeVisible();
  await page.getByRole("button", { name: "Retry" }).click();
  await expect(toast(page, "Marked S1 E2 watched.")).toBeVisible();
  const puts = fake.log.filter((entry) => entry.method === "PUT");
  expect(puts).toHaveLength(2);
  expect(puts[1].key).toBe(puts[0].key);
});

test("load failure retries; signed-out and unverified use product copy", async ({ page }) => {
  const fake = returning();
  let status: [number, string] | null = [503, "SERVICE_UNAVAILABLE"];
  await page.route("**/api/v1/**", (route, request) =>
    status && request.url().endsWith("/api/v1/home") ? error(route, ...status) : fake.handle(route, request),
  );
  await page.goto("/home");
  await expect(failure(page, "Your shows didn’t load")).toBeVisible();
  await expect(page.locator("body")).not.toContainText("Safe server message");
  status = null;
  await page.getByRole("button", { name: "Retry" }).click();
  await expect(featured(page)).toBeVisible();

  status = [401, "UNAUTHENTICATED"];
  await page.goto("/home");
  await expect(page.getByRole("heading", { name: "Sign in to see your shows" })).toBeVisible();
  await expect(page.getByRole("link", { name: "Sign in" })).toHaveAttribute("href", "/auth/signin?returnTo=%2Fhome");
  status = [403, "EMAIL_UNVERIFIED"];
  await page.goto("/home");
  await expect(page.getByRole("heading", { name: "Verify your email to track shows" })).toBeVisible();
});

test("Welcome offers account creation and sign-in; a signed-in browser goes to Home", async ({ page }) => {
  const fake = new FakeHome();
  let signedIn = false;
  await page.route("**/api/v1/**", (route, request) =>
    request.url().endsWith("/api/v1/session") && signedIn
      ? route.fulfill({ contentType: "application/json", body: JSON.stringify({ user: { id: showId(9) }, csrfToken: "csrf" }) })
      : fake.handle(route, request),
  );
  await page.goto("/");
  await expect(page.getByRole("heading", { level: 1, name: "Keep track of your shows. Without spoilers." })).toBeVisible();
  await expect(page.getByRole("navigation")).toHaveCount(0);
  await expect(page.getByRole("link", { name: "Create account" })).toHaveAttribute("href", "/auth/signup");
  await page.getByRole("link", { name: "Sign in" }).click();
  await expect(page).toHaveURL(/\/auth\/signin$/);

  signedIn = true;
  await page.goto("/");
  await expect(page).toHaveURL(/\/home$/);
});

const scan = (page: Page) =>
  page.evaluate(async (source) => {
    if (!("axe" in window)) new Function(source)();
    const result = await (window as unknown as { axe: typeof axe }).axe.run(document, { runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21aa"] } });
    return result.violations.map((violation) => `${violation.id}: ${violation.nodes.map((node) => node.target).join(", ")}`);
  }, axe.source);
const overflowAt = (page: Page) => page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
const smallTargets = (page: Page, scope: string) =>
  page.locator(`${scope} button:visible, ${scope} a:visible`).evaluateAll((elements) =>
    elements.filter((element) => element.getBoundingClientRect().height < 44).map((element) => element.textContent),
  );
const shot = (page: Page, width: number, name: string) =>
  width === 390 || width === 1440 ? page.screenshot({ path: `apps/web/src/features/home/verification/${name}-${width}.png`, fullPage: true }) : null;

async function check(page: Page, width: number, name: string, scope = ".sc-home") {
  expect(await overflowAt(page)).toBe(0);
  expect(await smallTargets(page, scope)).toEqual([]);
  expect(await scan(page)).toEqual([]);
  await shot(page, width, name);
}

for (const width of [320, 390, 860, 1440]) {
  test(`layout at ${width}px: no overflow, 44px targets, axe clean`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    const fake = returning();
    await openHome(page, fake);
    await expect(caught(page)).toContainText("Airs Nov 12, 2026");
    await expect(page.locator(width >= 860 ? "header nav" : "nav").first()).toBeVisible();
    await check(page, width, "home");

    const caughtFake = allCaught();
    fake.shows = caughtFake.shows;
    await page.goto("/home");
    await expect(caught(page)).toContainText("Release date unknown");
    await check(page, width, "caught-up");

    fake.shows = [];
    await page.goto("/home");
    await expect(page.getByRole("heading", { name: "Nothing to track yet" })).toBeVisible();
    await check(page, width, "empty");

    await page.goto("/");
    await expect(page.getByRole("heading", { level: 1 })).toBeVisible();
    await check(page, width, "welcome", ".sc-welcome main");
  });
}

test("loading and error states at 390 and 1440 for review", async ({ page }) => {
  const fake = returning();
  // Home reads are held (loading) or rejected (error); every other call reaches the fake.
  let mode: "hold" | "fail" = "hold";
  const held: Route[] = [];
  await page.route("**/api/v1/**", (route, request) => {
    if (!request.url().endsWith("/api/v1/home")) return fake.handle(route, request);
    if (mode === "fail") return error(route, 503, "SERVICE_UNAVAILABLE");
    held.push(route);
  });
  const states = [
    { mode: "hold", name: "loading", visible: () => page.getByText("Loading your shows…") },
    { mode: "fail", name: "error", visible: () => failure(page, "Your shows didn’t load") },
  ] as const;
  for (const state of states) {
    mode = state.mode;
    for (const width of [390, 1440]) {
      await page.setViewportSize({ width, height: 900 });
      await page.goto("/home");
      await expect(state.visible()).toBeVisible();
      expect(await scan(page)).toEqual([]);
      await shot(page, width, state.name);
    }
  }
  await Promise.all(held.map((route) => route.abort().catch(() => undefined)));
});
