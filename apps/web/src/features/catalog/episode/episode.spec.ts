import axe from "axe-core";
import { expect, test, type Page, type Request, type Route } from "@playwright/test";
import {
  code,
  CSRF,
  ep,
  episodeId,
  error,
  FakeTrackingApi,
  later,
  SECRET,
  UUID,
  type FakeEpisode,
} from "../../tracking/fixture-api";

/**
 * Component suite for R22. The production /episodes/[episodeId] route runs in an isolated fixture
 * app; a stateful fake of the C05/C07 API (progress, undo, catch-up, session-scoped reveals and
 * sign-out) answers at the /api/v1 network boundary. This is not live integration (R24 owns it).
 */

const OVERVIEW = "Protected overview";
const STILL = "/protected-still/";

/** S1: E1–E3 released, E4 future, E5 undated. One special. */
const EPISODES = [ep(1, 1), ep(1, 2), ep(1, 3), ep(1, 4, "future", "2026-11-12"), ep(1, 5, "unknown"), ep(0, 1)];
const byId = new Map(EPISODES.map((episode) => [episode.id, episode]));
const E3 = episodeId(1, 3);
const E4 = episodeId(1, 4);
const E5 = episodeId(1, 5);
const SPECIAL = episodeId(0, 1);

/** The shared C05 fake plus C07 session reveal grants and sign-out. The show is saved and watched. */
class FakeApi extends FakeTrackingApi {
  grants = new Set<string>();
  signedIn = true;

  constructor(watched: string[] = []) {
    super(EPISODES, watched, { saved: true, status: "watching", revision: 1 });
  }
  /** Sign-out revokes the session and every reveal grant. */
  logout() {
    this.signedIn = false;
    this.grants.clear();
  }
  override access(episode: FakeEpisode) {
    return super.access(episode) === "watched" ? "watched" : this.grants.has(episode.id) ? "revealed" : "locked";
  }
  override stillUrl = (episode: FakeEpisode) => `http://127.0.0.1:3123${STILL}${episode.id}.png`;
  override handle(route: Route, request: Request) {
    const path = new URL(request.url()).pathname.replace("/api/v1", "");
    const entry = { method: request.method(), path, query: {}, body: request.postData() ? request.postDataJSON() : undefined, key: null, csrf: request.headers()["x-csrf-token"] ?? null };
    if (path !== "/session" && !this.signedIn) {
      this.log.push(entry);
      return error(route, 401, "AUTH_REQUIRED");
    }
    if (entry.method === "DELETE" && path.startsWith("/reveals/")) {
      // 204 carries no body, so it is answered here rather than by the JSON helper.
      this.log.push(entry);
      this.grants.delete(path.split("/")[3]);
      return route.fulfill({ status: 204 });
    }
    return super.handle(route, request);
  }
  override mutate(method: string, path: string, body: Record<string, unknown> | undefined): [number, unknown] {
    if (method !== "POST" || path !== "/reveals") return super.mutate(method, path, body);
    const id = body!.resourceId as string;
    if (body!.scope !== "episode_details" || !byId.has(id)) return [404, "NOT_FOUND"];
    this.grants.add(id);
    return [201, { scope: "episode_details", resourceId: id, expiresAt: new Date(Date.now() + 12 * 3_600_000).toISOString() }];
  }
}

/** Requests for protected stills, recorded at the network boundary. */
async function openEpisode(page: Page, fake: FakeApi, id = E3) {
  const stills: string[] = [];
  await page.route("**/api/v1/**", (route, request) => fake.handle(route, request));
  await page.route(`**${STILL}**`, (route, request) => {
    stills.push(request.url());
    return route.fulfill({
      status: 200,
      contentType: "image/svg+xml",
      body: '<svg xmlns="http://www.w3.org/2000/svg" width="320" height="180"><rect width="320" height="180" fill="#888"/></svg>',
    });
  });
  await page.goto(`/episodes/${id}`);
  await expect(page.getByRole("heading", { level: 1, name: code(byId.get(id)!) })).toBeVisible();
  await expect(page.getByRole("link", { name: "Hollow Orchard", exact: true })).toBeVisible();
  return stills;
}
/** Holds matching requests until the returned release is called. */
async function hold(page: Page, pattern: string) {
  let release!: () => void;
  const held = new Promise<void>((resolve) => (release = resolve));
  await page.route(pattern, async (route) => {
    await held;
    return route.fallback();
  });
  return release;
}
const toast = (page: Page, text: string) => page.getByRole("status").filter({ hasText: text });
const revealButton = (page: Page) => page.getByRole("button", { name: "Reveal episode details…" });
const revealDialog = (page: Page) => page.getByRole("alertdialog", { name: "Reveal details for S1 E3?" });

/** Everything a viewer or assistive technology could get from the page: markup, attributes and the accessibility tree. */
async function exposed(page: Page) {
  return `${await page.content()}\n${await page.locator("body").ariaSnapshot()}\n${await page.title()}`;
}
async function expectRedacted(page: Page) {
  const text = await exposed(page);
  for (const value of [SECRET, OVERVIEW, STILL]) expect(text).not.toContain(value);
}

test.beforeEach(async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
});

test("a locked episode exposes no protected title, overview, still request or alt text", async ({ page }) => {
  const fake = new FakeApi();
  const stills = await openEpisode(page, fake);
  await expect(page.getByText("Title, summary and still are hidden until you watch this episode.")).toBeVisible();
  await expect(page.getByText("Season 1, episode 3 · Released Jan 1, 2026")).toBeVisible();
  await expect(page.getByText("Not watched", { exact: true })).toBeVisible();
  await expect(page).toHaveTitle("Hollow Orchard S1 E3 · SceneCask");
  await expectRedacted(page);
  expect(stills).toEqual([]);
  expect(await page.locator("img").count()).toBe(0);
  // Alpha reserves the discussion slot without rendering or requesting anything.
  await expect(page.getByText(/discussion/i)).toHaveCount(0);
  expect(fake.log.some((entry) => entry.path.includes("discussion"))).toBe(false);
  expect(fake.writes()).toEqual([]);
});

test("server HTML and RSC payloads carry no episode data", async ({ page }) => {
  const fake = new FakeApi([E3]);
  fake.grants.add(E5);
  await page.route("**/api/v1/**", (route, request) => fake.handle(route, request));
  for (const id of [E3, E5]) {
    for (const headers of [{}, { RSC: "1" }] as Record<string, string>[]) {
      const response = await page.request.get(`/episodes/${id}`, { headers });
      expect(response.ok()).toBe(true);
      const body = await response.text();
      for (const value of [SECRET, OVERVIEW, STILL, "Hollow Orchard"]) expect(body).not.toContain(value);
      if (!("RSC" in headers)) {
        expect(body).toContain("<title>Episode · SceneCask</title>");
        expect(body).toContain('name="referrer" content="no-referrer"');
      }
    }
  }
});

test("reveal asks first, unlocks only from the authorized read and changes no progress", async ({ page }) => {
  const fake = new FakeApi();
  const stills = await openEpisode(page, fake);

  await revealButton(page).click();
  await expect(revealDialog(page)).toContainText("This lasts for this sign-in session. Nothing is marked watched");
  await expect(revealDialog(page).getByRole("button", { name: "Cancel" })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(revealDialog(page)).toBeHidden();
  await expect(revealButton(page)).toBeFocused();
  expect(fake.writes()).toEqual([]);

  await revealButton(page).click();
  await revealDialog(page).getByRole("button", { name: "Reveal details" }).click();
  const heading = page.getByRole("heading", { level: 2, name: `${SECRET} S1 E3` });
  await expect(heading).toBeVisible();
  await expect(heading).toBeFocused();
  await expect(page.getByText(`${OVERVIEW} S1 E3.`)).toBeVisible();
  await expect(page.getByRole("img", { name: "Still from S1 E3" })).toBeVisible();
  expect(stills).toHaveLength(1);
  await expect(page.getByText("Showing details for S1 E3 for this sign-in session.")).toBeVisible();

  expect(fake.writes()).toEqual([
    expect.objectContaining({ method: "POST", path: "/reveals", body: { scope: "episode_details", resourceId: E3 }, csrf: CSRF }),
  ]);
  const read = fake.log.findLastIndex((entry) => entry.method === "GET" && entry.path === `/episodes/${E3}`);
  expect(read).toBeGreaterThan(fake.log.findIndex((entry) => entry.path === "/reveals"));
  expect(fake.watched(E3)).toBe(false);
  await expect(page.getByText("Not watched", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Mark S1 E3 watched" })).toBeVisible();
  // Accessible names and the browser title still use the code only.
  await expect(page).toHaveTitle("Hollow Orchard S1 E3 · SceneCask");
  await expect(page.getByRole("img", { name: new RegExp(SECRET) })).toHaveCount(0);
});

test("Hide again redacts the screen before the request and the refetch stays redacted", async ({ page }) => {
  const fake = new FakeApi();
  fake.grants.add(E3);
  await openEpisode(page, fake);
  await expect(page.getByRole("heading", { level: 2, name: `${SECRET} S1 E3` })).toBeVisible();
  const release = await hold(page, "**/api/v1/reveals/**");
  const reads = () => fake.log.filter((entry) => entry.path === `/episodes/${E3}`).length;
  const before = reads();
  await page.getByRole("button", { name: "Hide again" }).click();
  // Redacted while DELETE is still pending.
  await expectRedacted(page);
  await expect(revealButton(page)).toBeVisible();
  await expect(page.getByRole("region", { name: "S1 E3 details" })).toBeFocused();
  release();

  await expect.poll(() => fake.writes().at(-1)).toMatchObject({ method: "DELETE", path: `/reveals/episode_details/${E3}`, body: {}, csrf: CSRF });
  await expect.poll(reads).toBe(before + 1);
  await expect(page.getByRole("status").filter({ hasText: "Details for S1 E3 are hidden again." }).first()).toBeAttached();
  await expectRedacted(page);
  expect(fake.grants.has(E3)).toBe(false);
});

test("a failed Hide keeps the screen redacted and offers Retry", async ({ page }) => {
  const fake = new FakeApi();
  fake.grants.add(E3);
  await openEpisode(page, fake);
  let fail = true;
  await page.route("**/api/v1/reveals/**", (route) => (fail ? error(route, 503, "SERVICE_UNAVAILABLE") : route.fallback()));
  await page.getByRole("button", { name: "Hide again" }).click();
  const alert = page.getByRole("alert").filter({ hasText: "Couldn’t finish hiding S1 E3 for this sign-in session. It stays hidden here." });
  await expect(alert).toBeVisible();
  await expect(alert).not.toContainText("Safe server message.");
  await expectRedacted(page);
  // A reread while the grant still exists does not bring the details back.
  const reads = () => fake.log.filter((entry) => entry.path === `/episodes/${E3}`).length;
  const before = reads();
  await page.evaluate(() => document.dispatchEvent(new Event("visibilitychange")));
  await expect.poll(reads).toBe(before + 1);
  await expectRedacted(page);

  fail = false;
  await alert.getByRole("button", { name: "Retry" }).click();
  await expect.poll(() => fake.grants.has(E3)).toBe(false);
  await expectRedacted(page);
});

test("signing out evicts revealed details when the page is shown again", async ({ page }) => {
  const fake = new FakeApi();
  fake.grants.add(E3);
  await openEpisode(page, fake);
  await expect(page.getByText(`${OVERVIEW} S1 E3.`)).toBeVisible();

  // Sign-out elsewhere (another tab or the account menu), then the page is restored from the
  // back/forward cache: it redacts before asking the API, then shows the session-ended state.
  fake.logout();
  await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent("pageshow", { persisted: true })));
  await expectRedacted(page);
  await expect(page.getByRole("heading", { name: "Your sign-in session ended" })).toBeVisible();
  await expect(page.getByText("Episode details you revealed are hidden again.")).toBeVisible();
  await expect(page.getByRole("link", { name: "Sign in" })).toHaveAttribute("href", `/auth/signin?returnTo=${encodeURIComponent(`/episodes/${E3}`)}`);
  await expectRedacted(page);
});

test("a session that expires during a write ends on the redacted session state", async ({ page }) => {
  const fake = new FakeApi();
  fake.grants.add(E3);
  await openEpisode(page, fake);
  fake.logout();
  await page.getByRole("button", { name: "Mark S1 E3 watched" }).click();
  await expect(page.getByRole("heading", { name: "Your sign-in session ended" })).toBeVisible();
  await expectRedacted(page);
});

test("an expired reveal is redacted when it lapses", async ({ page }) => {
  await page.clock.install();
  const fake = new FakeApi();
  await openEpisode(page, fake);
  await revealButton(page).click();
  await revealDialog(page).getByRole("button", { name: "Reveal details" }).click();
  await expect(page.getByText(`${OVERVIEW} S1 E3.`)).toBeVisible();
  fake.grants.clear();
  await page.clock.fastForward("12:00:01");
  await expect(page.getByText("Your reveal for S1 E3 ended, so its details are hidden again.").first()).toBeVisible();
  await expectRedacted(page);
});

test("a future episode can be marked; the authorized read unlocks details and the release label stays", async ({ page }) => {
  const fake = new FakeApi();
  const stills = await openEpisode(page, fake, E4);
  const label = "Airs Nov 12, 2026 (estimated)";
  await expect(page.getByRole("note")).toContainText("Not released yet. If you’ve already seen it, you can still mark it watched.");
  await expect(page.locator(".sc-ep-air-strong")).toHaveText(label);
  await expect(page.getByText("Not released", { exact: true })).toBeVisible();
  await expectRedacted(page);

  await page.getByRole("button", { name: "Mark S1 E4 watched" }).click();
  await expect(toast(page, "Marked S1 E4 watched.")).toBeVisible();
  await expect(page.getByRole("heading", { level: 2, name: `${SECRET} S1 E4` })).toBeVisible();
  expect(fake.writes()).toEqual([
    expect.objectContaining({ method: "PUT", path: `/progress/episodes/${E4}`, body: { watched: true, expectedRevision: 0 }, csrf: CSRF }),
  ]);
  expect(fake.writes()[0].key).toMatch(UUID);
  // Details came from a GET after the write, not from the write response.
  const put = fake.log.findIndex((entry) => entry.method === "PUT");
  expect(fake.log.slice(put + 1).some((entry) => entry.method === "GET" && entry.path === `/episodes/${E4}`)).toBe(true);
  expect(stills).toHaveLength(1);
  // The future label and notice are unchanged by the mark.
  await expect(page.getByRole("note")).toContainText("Not released yet.");
  await expect(page.locator(".sc-ep-air")).toHaveText(label);
  await expect(page.getByText("Watched", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Mark unwatched" })).toBeVisible();
});

test("unmarking hides watched details at once; Undo restores them through a reread", async ({ page }) => {
  const fake = new FakeApi([E3]);
  await openEpisode(page, fake);
  await expect(page.getByRole("heading", { level: 2, name: `${SECRET} S1 E3` })).toBeVisible();
  const release = await hold(page, `**/api/v1/progress/episodes/${E3}`);
  await page.getByRole("button", { name: "Mark unwatched" }).click();
  await expectRedacted(page);
  release();
  const done = toast(page, "Marked S1 E3 unwatched.");
  await expect(done).toBeVisible();
  await expectRedacted(page);
  await expect(revealButton(page)).toBeVisible();

  await done.getByRole("button", { name: "Undo" }).click();
  await expect(toast(page, "S1 E3 is watched again.")).toBeVisible();
  await expect(page.getByRole("heading", { level: 2, name: `${SECRET} S1 E3` })).toBeVisible();
  await expect(page.getByRole("button", { name: "Mark unwatched" })).toBeVisible();
});

test("undated and special notices; specials have no catch-up link", async ({ page }) => {
  const fake = new FakeApi();
  await openEpisode(page, fake, E5);
  await expect(page.getByRole("note")).toHaveText(
    "No confirmed release date. This episode isn’t suggested as next or included in catch-up. You can still mark it watched.",
  );
  await expect(page.getByText("Date unknown", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Mark S1 E5 watched" }).click();
  await expect(page.getByText("Watched", { exact: true })).toBeVisible();
  await expect(page.locator(".sc-ep-air")).toHaveText("Release date unknown");

  await page.unrouteAll({ behavior: "ignoreErrors" });
  await openEpisode(page, new FakeApi(), SPECIAL);
  await expect(page.getByRole("note")).toContainText("Specials are tracked separately");
  await expect(page.getByRole("button", { name: /Mark watched through/ })).toHaveCount(0);
});

test("the catch-up link previews through this episode and marks only the confirmed set", async ({ page }) => {
  const fake = new FakeApi([episodeId(1, 1)]);
  await openEpisode(page, fake);
  await page.getByRole("button", { name: "Mark watched through S1 E3…" }).click();
  const sheet = page.getByRole("dialog");
  await expect(sheet).toContainText("Marks 2 episodes watched: S1 E2–E3.");
  await expect(sheet).toContainText("Already watched, left as they are: S1 E1");
  expect(fake.writes()).toEqual([]);
  await sheet.getByRole("button", { name: "Mark 2 watched" }).click();
  await expect(sheet).toBeHidden();
  await expect(toast(page, "Marked 2 episodes watched through S1 E3.")).toBeVisible();
  await expect(page.getByRole("heading", { level: 2, name: `${SECRET} S1 E3` })).toBeVisible();
  expect(fake.watched(episodeId(1, 2))).toBe(true);
  expect(fake.watched(E4) || fake.watched(E5)).toBe(false);
});

test("a rejected reveal stays in the dialog with product copy and reveals nothing", async ({ page }) => {
  const fake = new FakeApi();
  await openEpisode(page, fake);
  await page.route("**/api/v1/reveals", (route) => error(route, 503, "SERVICE_UNAVAILABLE"));
  await revealButton(page).click();
  await revealDialog(page).getByRole("button", { name: "Reveal details" }).click();
  await expect(revealDialog(page).getByRole("alert")).toHaveText("Couldn’t reveal details. Nothing was changed. Try again.");
  await expect(revealDialog(page).getByRole("button", { name: "Try again" })).toBeVisible();
  await expectRedacted(page);
});

test("load failure and missing episode states use product copy", async ({ page }) => {
  let fail = true;
  const fake = new FakeApi();
  await page.route("**/api/v1/**", (route, request) => {
    if (fail && request.url().endsWith(`/episodes/${E3}`)) return error(route, 503, "SERVICE_UNAVAILABLE");
    return fake.handle(route, request);
  });
  await page.goto(`/episodes/${E3}`);
  const alert = page.getByRole("alert").filter({ hasText: "This episode didn’t load" });
  await expect(alert).toBeVisible();
  await expect(alert).not.toContainText("Safe server message.");
  fail = false;
  await page.getByRole("button", { name: "Retry" }).click();
  await expect(page.getByRole("heading", { level: 1, name: "S1 E3" })).toBeVisible();

  await page.goto("/episodes/30000000-0000-4000-8000-00000000dead");
  await expect(page.getByRole("heading", { name: "Episode not found" })).toBeVisible();
});

test("reveal and hide work by keyboard alone", async ({ page }) => {
  const fake = new FakeApi();
  await openEpisode(page, fake);
  await revealButton(page).focus();
  await page.keyboard.press("Enter");
  await expect(revealDialog(page).getByRole("button", { name: "Cancel" })).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(revealDialog(page).getByRole("button", { name: "Reveal details" })).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("heading", { level: 2, name: `${SECRET} S1 E3` })).toBeFocused();
  await page.getByRole("button", { name: "Hide again" }).focus();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("region", { name: "S1 E3 details" })).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(revealButton(page)).toBeFocused();
  await expectRedacted(page);
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
const small = (page: Page) =>
  page
    .locator(".sc-ep button:visible, .sc-ep a:visible, [role=alertdialog] button:visible")
    .evaluateAll((elements) => elements.filter((element) => element.getBoundingClientRect().height < 44).map((element) => element.textContent));

for (const width of [320, 390, 860, 1440]) {
  test(`layout at ${width}px: no overflow, 44px targets, axe clean`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    const shot = (name: string) =>
      width === 390 || width === 1440 ? page.screenshot({ path: `apps/web/src/features/catalog/episode/verification/${name}-${width}.png`, fullPage: true }) : null;
    const fake = new FakeApi();
    await openEpisode(page, fake);
    await expect(page.locator(width >= 860 ? "header nav" : "nav").first()).toBeVisible();
    expect(await overflow(page)).toBe(0);
    expect(await small(page)).toEqual([]);
    expect(await scan(page)).toEqual([]);
    await shot("locked");

    await revealButton(page).click();
    await expect(revealDialog(page)).toBeVisible();
    expect(await overflow(page)).toBe(0);
    expect(await small(page)).toEqual([]);
    expect(await scan(page)).toEqual([]);
    await shot("reveal-confirm");

    await revealDialog(page).getByRole("button", { name: "Reveal details" }).click();
    await expect(page.getByRole("img", { name: "Still from S1 E3" })).toBeVisible();
    expect(await overflow(page)).toBe(0);
    expect(await small(page)).toEqual([]);
    expect(await scan(page)).toEqual([]);
    await shot("revealed");

    await later(0);
    fake.logout();
    await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent("pageshow", { persisted: true })));
    await expect(page.getByRole("heading", { name: "Your sign-in session ended" })).toBeVisible();
    expect(await scan(page)).toEqual([]);
    await shot("session-ended");
  });
}

test("future episode layout at 390 and 1440", async ({ page }) => {
  for (const width of [390, 1440]) {
    await page.setViewportSize({ width, height: 900 });
    await page.unrouteAll({ behavior: "ignoreErrors" });
    await openEpisode(page, new FakeApi(), E4);
    expect(await overflow(page)).toBe(0);
    await page.screenshot({ path: `apps/web/src/features/catalog/episode/verification/future-${width}.png`, fullPage: true });
  }
});
