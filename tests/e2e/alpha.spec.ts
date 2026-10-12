// R24 alpha gate: browser journeys on the real Next.js + Axum + PostgreSQL + SMTP-capture stack.
// Only TMDB and Google are replaced, by fixture servers at the provider boundary
// (alpha-fixtures/server.mjs). Network bodies are inspected for protected episode titles.
import { randomUUID } from "node:crypto";
import { expect, test, type Page } from "@playwright/test";
import {
  PASSWORD,
  apiAs,
  axeViolations,
  emailedLink,
  episodeIds,
  freshEmail,
  horizontalOverflow,
  recordNetwork,
  setGoogleIdentity,
  setTmdbDown,
  signIn,
  signOut,
  titlesExcept,
  verifiedAccount,
} from "./alpha-fixtures/journey";

// The provider switches are process-wide, so this file runs in one worker, in order.
test.describe.configure({ mode: "serial" });

const episodes = (page: Page) => page.getByRole("list", { name: "Season 1 episodes" });
const toggle = (page: Page, code: string) => page.getByRole("button", { name: `${code} watched`, exact: true });
const toast = (page: Page, text: string | RegExp) => page.getByRole("status").filter({ hasText: text });
const sheet = (page: Page) => page.getByRole("dialog");

async function addHollowOrchard(page: Page) {
  await page.goto("/discover");
  await page.getByRole("searchbox").fill("hollow");
  await expect(page.getByRole("button", { name: /^Open Hollow Orchard \(2010\)/ })).toBeVisible();
  await page.getByRole("button", { name: "Add Hollow Orchard (2024) to Plan to watch" }).click();
  await expect(page.getByRole("button", { name: "Hollow Orchard (2024) is in your library. Open it" })).toBeVisible();
  await page.getByRole("button", { name: /^Open Hollow Orchard \(2024\)/ }).click();
  await expect(page).toHaveURL(/\/shows\/[0-9a-f-]{36}/);
  await expect(page.getByRole("heading", { level: 1, name: "Hollow Orchard" })).toBeVisible();
  return page.url().match(/\/shows\/([0-9a-f-]{36})/)![1];
}

async function expectWatched(page: Page, expected: Record<string, boolean>) {
  for (const [code, watched] of Object.entries(expected)) {
    await expect(toggle(page, code)).toHaveAttribute("aria-pressed", String(watched));
  }
}

test("fresh account with no friends: signup → verify → search → save → out-of-order → catch-up → Undo → reload → logout/login", async ({ page, browser }) => {
  const network = recordNetwork(page);
  const email = freshEmail("journey");

  // Sign up and verify through the real emailed link.
  await page.goto("/auth/signup");
  await page.getByLabel("Email").fill(email);
  await page.getByLabel("Password", { exact: true }).fill(PASSWORD);
  await page.getByRole("button", { name: "Create account" }).click();
  await expect(page).toHaveURL(/\/auth\/verify/);
  await page.goto(await emailedLink(page.request, email, "/auth/verify"));
  await expect(page).toHaveURL(/\/onboarding\/profile$/);
  await page.getByRole("link", { name: "Skip for now" }).click();
  await expect(page).toHaveURL(/\/discover$/);

  // Search with disambiguation, save, open. Nothing protected has reached the browser.
  const showId = await addHollowOrchard(page);
  await expect(page.getByRole("button", { name: "Library status: Plan to watch" })).toBeVisible();
  expect(await network.leaks(titlesExcept())).toEqual([]);

  // Mark E3 alone: only E3 unlocks; E1 stays next and the show moves to Watching.
  await toggle(page, "S1 E3").click();
  await expectWatched(page, { "S1 E1": false, "S1 E2": false, "S1 E3": true });
  await expect(episodes(page).getByText("The Cellar Door")).toBeVisible();
  await expect(page.getByText("Some episodes are marked out of order.")).toBeVisible();
  await expect(page.getByRole("button", { name: "Library status: Watching" })).toBeVisible();

  // Home: E1 is next, never by title; marking it makes E2 next.
  await page.getByRole("link", { name: "Home" }).first().click();
  await expect(page).toHaveURL(/\/home$/);
  await expect(page.getByText("S1 E1", { exact: false }).first()).toBeVisible();
  await page.getByRole("button", { name: /Mark S1 E1 watched/ }).first().click();
  await expect(page.getByText(/S1 E2/).first()).toBeVisible();
  expect(await network.leaks(titlesExcept("The Cellar Door", "Quiet Harvest"))).toEqual([]);

  // Explicit catch-up through E4 marks exactly the released, unwatched E2 and E4.
  await page.goto(`/shows/${showId}`);
  await page.getByRole("button", { name: "Catch up…" }).click();
  await sheet(page).getByRole("button", { name: "S1 E4" }).click();
  await expect(sheet(page).getByText("Marks 2 episodes watched")).toBeVisible();
  await sheet(page).getByRole("button", { name: "Mark 2 watched" }).click();
  await expectWatched(page, { "S1 E1": true, "S1 E2": true, "S1 E3": true, "S1 E4": true, "S1 E5": false });

  // A concurrent edit in another signed-in browser unmarks E4 before Undo.
  const undo = toast(page, /watched/).getByRole("button", { name: "Undo" });
  await undo.focus(); // Holds the 6s toast open while the other session writes.
  const other = await browser.newContext({ baseURL: "http://127.0.0.1:3000" });
  const otherPage = await other.newPage();
  await signIn(otherPage, email);
  await expect(otherPage).toHaveURL(/\/home$/);
  const ids = await episodeIds(other, showId);
  const otherApi = await apiAs(other);
  const e4 = (await (await otherApi.get(`/episodes/${ids[4]}`)).json()) as { revision: number };
  expect((await otherApi.put(`/progress/episodes/${ids[4]}`, { watched: false, expectedRevision: e4.revision })).status()).toBe(200);

  // Undo reverts only what is still the catch-up's own: E2. E4 keeps the newer edit.
  await undo.click();
  await expect(toast(page, "Undone. 1 change you made afterwards was kept.")).toBeVisible();
  await page.reload();
  await expectWatched(page, { "S1 E1": true, "S1 E2": false, "S1 E3": true, "S1 E4": false, "S1 E5": false });
  await expect(episodes(page).getByText("Lantern Debt")).toHaveCount(0);

  await other.close();

  // Sign out: this browser's other tabs leave private screens too; Back does not restore data.
  const tab = await page.context().newPage();
  await tab.goto("/library");
  await expect(tab.getByText("Hollow Orchard").first()).toBeVisible();
  await signOut(page);
  await expect(tab).toHaveURL(/\/auth\/signin\?reason=expired/);
  await tab.close();
  await page.goBack();
  await expect(page).toHaveURL(/\/auth\/signin/);
  await expect(page.getByText("The Cellar Door")).toHaveCount(0);

  // Sign in again: the same authoritative state.
  await signIn(page, email, `/shows/${showId}`);
  await expect(page).toHaveURL(new RegExp(`/shows/${showId}`));
  await expectWatched(page, { "S1 E1": true, "S1 E2": false, "S1 E3": true, "S1 E4": false, "S1 E5": false });
  await page.goto("/library");
  await expect(page.getByText("2 of 5 episodes watched · Next S1 E2")).toBeVisible();
  // Start a clean capture: E2 and E4 were unlocked only while watched. In this new session, only
  // the watched E1/E3 may arrive; E2, E4, E5 and the special never do.
  await network.leaks([]);
  await page.goto(`/shows/${showId}`);
  await expectWatched(page, { "S1 E2": false });
  expect(await network.leaks(titlesExcept("The Cellar Door", "Quiet Harvest"))).toEqual([]);
});

test("provider outage keeps saved progress and catalog; unverified and other sessions never get protected payloads", async ({ page, browser }) => {
  const ana = await verifiedAccount(page, "ana");
  const showId = await addHollowOrchard(page);
  await toggle(page, "S1 E3").click();
  await expectWatched(page, { "S1 E3": true });
  const ids = await episodeIds(page.context(), showId);

  // Reveal E2 in this session only (scoped, marks nothing).
  await page.goto(`/episodes/${ids[2]}`);
  await page.getByRole("button", { name: /^Reveal/ }).click();
  await page.getByRole("alertdialog").getByRole("button", { name: /^Reveal/ }).click();
  await expect(page.getByRole("heading", { name: "Lantern Debt" })).toBeVisible();

  try {
    await setTmdbDown(page.request, true);
    // Search fails with Retry, no provider text; saved catalog and progress keep working.
    await page.goto("/discover?q=orchard");
    await expect(page.getByText("Show search isn’t responding")).toBeVisible();
    await expect(page.getByRole("button", { name: "Retry" })).toBeVisible();
    await page.goto(`/shows/${showId}`);
    await expect(page.getByRole("heading", { level: 1, name: "Hollow Orchard" })).toBeVisible();
    await toggle(page, "S1 E1").click();
    await expectWatched(page, { "S1 E1": true, "S1 E2": false, "S1 E3": true });
    await page.reload();
    await expectWatched(page, { "S1 E1": true, "S1 E3": true });
  } finally {
    await setTmdbDown(page.request, false);
  }

  // Ana's second session: the reveal does not carry over; nothing protected beyond E1/E3.
  const second = await browser.newContext({ baseURL: "http://127.0.0.1:3000" });
  const secondPage = await second.newPage();
  const secondNet = recordNetwork(secondPage);
  await signIn(secondPage, ana, `/episodes/${ids[2]}`);
  await expect(secondPage.getByRole("heading", { name: /S1 E2/ })).toBeVisible();
  await expect(secondPage.getByText("Lantern Debt")).toHaveCount(0);
  await secondPage.goto(`/shows/${showId}`);
  await expectWatched(secondPage, { "S1 E1": true, "S1 E3": true });
  expect(await secondNet.leaks(titlesExcept("Quiet Harvest", "The Cellar Door"))).toEqual([]);
  await second.close();

  // Another member (no friends): catalog is readable, every detail stays locked, in JSON,
  // server-rendered HTML and route prefetches alike.
  const bob = await browser.newContext({ baseURL: "http://127.0.0.1:3000" });
  const bobPage = await bob.newPage();
  const bobNet = recordNetwork(bobPage);
  await verifiedAccount(bobPage, "bob");
  await bobPage.goto(`/shows/${showId}`);
  await expectWatched(bobPage, { "S1 E1": false, "S1 E3": false });
  await bobPage.goto(`/episodes/${ids[3]}`);
  await expect(bobPage.getByRole("heading", { name: /S1 E3/ })).toBeVisible();
  for (const path of [`/episodes/${ids[3]}`, `/shows/${showId}`, "/home", "/library"]) {
    const html = await (await bob.request.get(path)).text();
    expect(titlesExcept().filter((title) => html.includes(title)), path).toEqual([]);
  }
  const bobApi = await apiAs(bob);
  const library = (await (await bobApi.get("/library")).json()) as { items: unknown[] };
  expect(library.items).toEqual([]);
  expect(await bobNet.leaks(titlesExcept())).toEqual([]);
  await bob.close();

  // An unverified account gets 403 with no payload, and no application session from sign-in.
  const pending = await browser.newContext({ baseURL: "http://127.0.0.1:3000" });
  const pendingEmail = freshEmail("pending");
  const register = await pending.request.post("/api/v1/auth/register", {
    headers: { Origin: "http://127.0.0.1:3000" },
    data: { email: pendingEmail, password: PASSWORD },
  });
  expect(register.status()).toBe(202);
  const pendingPage = await pending.newPage();
  await signIn(pendingPage, pendingEmail);
  await expect(pendingPage).toHaveURL(/\/auth\/verify/);
  for (const path of ["/home", `/episodes/${ids[3]}`, `/shows/${showId}`, "/library"]) {
    const response = await pending.request.get(`/api/v1${path}`);
    expect([401, 403], path).toContain(response.status());
    const body = await response.text();
    expect(body).not.toContain("Hollow Orchard");
    expect(titlesExcept().filter((title) => body.includes(title))).toEqual([]);
  }
  await pending.close();
});

test("Google sign-in through the OIDC boundary creates a separate private account", async ({ page }) => {
  const email = freshEmail("google");
  await setGoogleIdentity(page.request, { sub: `google-${randomUUID()}`, email, name: "Gia Lane" });
  try {
    await page.goto("/auth/signin");
    await page.getByRole("button", { name: "Continue with Google" }).click();
    await expect(page).toHaveURL(/\/onboarding\/profile$/);
    await page.getByLabel("Handle").fill(`gia_${randomUUID().slice(0, 8)}`);
    await page.getByRole("button", { name: "Save profile" }).click();
    await expect(page).toHaveURL(/\/discover$/);
    await page.getByRole("button", { name: "Account" }).click();
    await expect(page.getByRole("menu")).toContainText("Gia Lane");
    await page.keyboard.press("Escape");
  } finally {
    await setGoogleIdentity(page.request, null);
  }
});

test("signed-out visits to private routes go to sign-in with the return path; About is public", async ({ page }) => {
  await page.goto("/library?status=watching");
  await expect(page).toHaveURL(/\/auth\/signin\?returnTo=%2Flibrary%3Fstatus%3Dwatching$/);
  const about = await page.request.get("/about");
  expect(await about.text()).toContain("This product uses the TMDB API but is not endorsed or certified by TMDB.");
});

for (const width of [320, 390, 860, 1440]) {
  test(`alpha screens at ${width}px: no overflow, nav switch, axe clean`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await verifiedAccount(page, `layout${width}`);
    const showId = await addHollowOrchard(page);
    await toggle(page, "S1 E3").click();
    await expectWatched(page, { "S1 E3": true });
    const ids = await episodeIds(page.context(), showId);
    for (const [name, path] of [
      ["home", "/home"],
      ["discover", "/discover?q=hollow"],
      ["library", "/library"],
      ["show", `/shows/${showId}`],
      ["episode", `/episodes/${ids[3]}`],
      ["about", "/about"],
    ] as const) {
      await page.goto(path);
      await expect(page.locator("main h1").first()).toBeVisible();
      await page.waitForLoadState("networkidle");
      expect(await horizontalOverflow(page), `${name} overflow`).toBeLessThanOrEqual(0);
      const nav = page.getByRole("navigation", { name: "Main navigation" });
      const box = await nav.boundingBox();
      // Below 860px the nav is the fixed bottom bar; from 860px it sits in the header.
      expect(box!.y > 400, `${name} nav position`).toBe(width < 860);
      expect(await axeViolations(page), `${name} axe`).toEqual([]);
      if (width === 390 || width === 1440) {
        await page.screenshot({ path: `test-results/alpha/${name}-${width}.png`, fullPage: true });
      }
    }
  });
}

test("keyboard: account menu opens, moves and closes with focus restored", async ({ page }) => {
  await verifiedAccount(page, "keys");
  await page.goto("/home");
  const trigger = page.getByRole("button", { name: "Account" });
  await trigger.focus();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("menuitem", { name: "About SceneCask" })).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await expect(page.getByRole("menuitem", { name: "Sign out" })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(trigger).toBeFocused();
  await page.keyboard.press("Enter");
  await page.keyboard.press("ArrowUp");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/auth\/signin$/);
});
