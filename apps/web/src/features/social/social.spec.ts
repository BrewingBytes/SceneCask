import { expect, test, type Page, type Request, type Route } from "@playwright/test";
import { axeViolations, error, expectNoPageOverflow, json, later, responsiveTests } from "../tracking/fixture-api";

/**
 * Component suite for R28. The production /friends and /people/[id] routes run in an isolated
 * fixture app with the beta flag on, and /feed renders the exported Home/Discover slots. A
 * stateful fake of the C06 endpoints answers at the /api/v1 network boundary, so the generated
 * client and CSRF bootstrap are exercised. This is not live integration: R38 owns the live
 * relationship matrix.
 */

const id = (n: number) => `20000000-0000-4000-8000-${String(n).padStart(12, "0")}`;
const show = (n: number) => `10000000-0000-4000-8000-${String(n).padStart(12, "0")}`;
type Edge = "none" | "pending" | "approved";
type Visibility = "private" | "public";

interface Account {
  id: string;
  handle: string;
  displayName: string;
  visibility: Visibility;
  /** Viewer → them. */
  outgoing: Edge;
  /** Them → viewer. */
  incoming: Edge;
  blocked: boolean;
}

const account = (n: number, displayName: string, handle: string, visibility: Visibility, outgoing: Edge = "none", incoming: Edge = "none"): Account => ({
  id: id(n),
  handle,
  displayName,
  visibility,
  outgoing,
  incoming,
  blocked: false,
});

const progress = (watched: number, total: number) => ({
  nextEpisode: watched < total ? { id: id(900 + watched), season: 1, number: watched + 1 } : null,
  outOfOrder: false,
  percent: Math.round((watched / total) * 100),
  releaseInfoIncomplete: false,
  state: watched < total ? "in_progress" : "completed",
  total,
  watched,
});

const ID_IN_PATH = /[0-9a-f]{8}-[0-9a-f-]{27}/;

/** One intercepted request, keyed "METHOD /path/{id}". */
interface Seen {
  key: string;
  target: string;
  body: Record<string, unknown> | null;
  csrf: string | null;
  query: URLSearchParams;
}

/** C06 people, follow, request, follower, block, activity and recommendation endpoints. */
class FakeApi {
  user = {
    id: id(0),
    email: "ana@example.test",
    displayName: "Ana Rivera",
    handle: "ana_r" as string | null,
    visibility: "private" as Visibility,
    verified: true,
    role: "member",
  };
  csrf = "csrf-1";
  signedIn = true;
  people = [
    account(1, "Bea Lin", "bea", "private"),
    account(2, "Cal Ortiz", "cal", "public"),
    account(3, "Dee Park", "dee", "private", "approved", "approved"),
    account(4, "Eve Stone", "eve", "private", "none", "pending"),
    account(5, "Fay Moreno", "fay", "public", "none", "approved"),
  ];
  library = [
    { showId: show(1), title: "Hollow Orchard", year: 2024, posterUrl: null, saved: true, status: "watching", progress: progress(3, 8) },
    { showId: show(2), title: "Lantern Coast", year: null, posterUrl: null, saved: true, status: "plan_to_watch", progress: progress(0, 6) },
  ];
  activity = [
    { id: id(700), actor: this.person(this.people[2]), kind: "episode_watched", show: { id: show(1), title: "Hollow Orchard", posterUrl: null }, episode: { id: id(902), season: 1, number: 3 }, createdAt: "2026-10-12T18:30:00Z" },
    { id: id(701), actor: this.person(this.people[2]), kind: "library_added", show: { id: show(2), title: "Lantern Coast", posterUrl: null }, episode: null, createdAt: "2026-10-11T09:00:00Z" },
  ];
  recommendations = [
    { show: { id: show(3), title: "Paper Moons", year: 2023, posterUrl: null }, reason: { kind: "followed_people", people: [this.person(this.people[2]), this.person(this.people[4])] } },
  ];
  pageSize = 50;
  seen: Seen[] = [];
  /** Replaces the normal answer for one key. */
  replies = new Map<string, (route: Route) => unknown>();

  count(key: string) {
    return this.seen.filter((call) => call.key === key);
  }
  mutations() {
    return this.seen.filter((call) => !call.key.startsWith("GET "));
  }
  find(target: string) {
    return this.people.find((item) => item.id === target && !item.blocked);
  }
  person(item: Account) {
    const initials = item.displayName
      .split(" ")
      .map((word) => word[0])
      .join("");
    return {
      id: item.id,
      handle: item.handle,
      displayName: item.displayName,
      initials,
      visibility: item.visibility,
      relationship: { outgoing: item.outgoing, incoming: item.incoming, mutual: item.outgoing === "approved" && item.incoming === "approved" },
    };
  }
  canView(item: Account) {
    return item.visibility === "public" || item.outgoing === "approved";
  }
  page<T>(items: T[], query: URLSearchParams) {
    const start = Number(query.get("cursor") ?? 0);
    const end = start + this.pageSize;
    return { items: items.slice(start, end), nextCursor: end < items.length ? String(end) : null };
  }

  serve = async (route: Route, request: Request) => {
    const url = new URL(request.url());
    const path = url.pathname.slice("/api/v1".length);
    const target = path.match(ID_IN_PATH)?.[0] ?? "";
    const seen: Seen = {
      key: `${request.method()} ${target ? path.replace(target, "{id}") : path}`,
      target,
      body: request.postDataJSON(),
      csrf: await request.headerValue("x-csrf-token"),
      query: url.searchParams,
    };
    this.seen.push(seen);
    const reply = this.replies.get(seen.key);
    return reply ? reply(route) : this.respond(route, seen);
  };

  respond(route: Route, { key, target, body, csrf, query }: Seen) {
    if (key === "GET /session") return json(route, { user: this.signedIn ? this.user : null, csrfToken: this.csrf });
    if (!this.signedIn) return error(route, 401, "AUTH_REQUIRED");
    if (!key.startsWith("GET ") && csrf !== this.csrf) return error(route, 403, "CSRF_FAILED");
    const item = this.find(target);
    const handleRequired = () =>
      json(route, { error: { code: "VALIDATION_ERROR", message: "Safe server message.", fields: { handle: "Choose a handle." } } }, 422);
    switch (key) {
      case "GET /people": {
        const q = query.get("q")!.toLowerCase();
        const found = this.people.filter((p) => !p.blocked && (p.handle.includes(q) || p.displayName.toLowerCase().includes(q)));
        return json(route, this.page(found.map((p) => this.person(p)), query));
      }
      case "GET /people/{id}":
        return item ? json(route, { person: this.person(item), canViewLibrary: this.canView(item) }) : error(route, 404, "NOT_FOUND");
      case "GET /people/{id}/library":
        return item && this.canView(item) ? json(route, this.page(this.library, query)) : error(route, 404, "NOT_FOUND");
      case "POST /people/{id}/follow":
        if (!this.user.handle) return handleRequired();
        if (!item) return error(route, 404, "NOT_FOUND");
        if (item.outgoing === "none") item.outgoing = item.visibility === "public" ? "approved" : "pending";
        return json(route, { state: item.outgoing });
      case "DELETE /people/{id}/follow":
        if (!item) return error(route, 404, "NOT_FOUND");
        item.outgoing = "none";
        return route.fulfill({ status: 204 });
      case "GET /me/follow-requests":
        return json(route, this.page(this.people.filter((p) => !p.blocked && p.incoming === "pending").map((p) => this.person(p)), query));
      case "POST /me/follow-requests/{id}":
        if (!item || item.incoming !== "pending") return error(route, 404, "NOT_FOUND");
        item.incoming = body!.decision === "approve" ? "approved" : "none";
        return route.fulfill({ status: 204 });
      case "GET /me/followers":
        return json(route, this.page(this.people.filter((p) => !p.blocked && p.incoming === "approved").map((p) => this.person(p)), query));
      case "DELETE /me/followers/{id}":
        if (item) item.incoming = "none";
        return route.fulfill({ status: 204 });
      case "PUT /me/blocks/{id}":
        if (!item) return error(route, 404, "NOT_FOUND");
        Object.assign(item, { blocked: true, outgoing: "none", incoming: "none" });
        return route.fulfill({ status: 204 });
      case "GET /activity":
        return json(route, this.page(this.activity, query));
      case "GET /recommendations":
        return json(route, this.page(this.recommendations, query));
    }
    return error(route, 404, "NOT_FOUND");
  }
}

async function open(page: Page, fake = new FakeApi(), path = "/friends") {
  await page.route("**/api/v1/**", fake.serve);
  await page.goto(path);
  return fake;
}

const status = (page: Page, text: string | RegExp) => page.getByRole("status").filter({ hasText: text });
const alert = (page: Page, text: string | RegExp) => page.getByRole("alert").filter({ hasText: text });
const row = (page: Page, name: string) => page.getByRole("listitem").filter({ has: page.getByRole("link", { name, exact: true }) });

async function search(page: Page, text: string) {
  await page.getByRole("searchbox", { name: "Search people" }).fill(text);
}

test.beforeEach(async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
});

test("find people: idle prompt, short-query hint, results with visibility and relationship from the API", async ({ page }) => {
  const fake = await open(page);
  await expect(page.getByRole("heading", { level: 1, name: "Friends" })).toBeVisible();
  await expect(page.getByText(/Your account is private\. People can find your name and @ana_r/)).toBeVisible();
  await expect(page.getByText(/everyone in a thread can see the other participants/)).toBeVisible();
  await expect(page.getByRole("tab", { name: "Find people" })).toHaveAttribute("aria-selected", "true");
  await expect(page.getByRole("heading", { name: "Find people you know" })).toBeVisible();

  await search(page, "b");
  await expect(page.getByText("Type at least 2 characters.")).toBeVisible();
  await later(500);
  expect(fake.count("GET /people")).toHaveLength(0);

  // A typed @ is part of how people write handles; the API searches the handle without it.
  await search(page, "@dee");
  const dee = row(page, "Dee Park");
  await expect(dee).toContainText("@dee");
  await expect(dee).toContainText("Private");
  await expect(dee).toContainText("You follow each other");
  await expect(dee.getByRole("link", { name: "Dee Park" })).toHaveAttribute("href", `/people/${id(3)}`);
  expect(fake.count("GET /people").map((call) => call.query.get("q"))).toEqual(["dee"]);

  await search(page, "zzz");
  await expect(page.getByRole("heading", { name: "No one matches “zzz”" })).toBeVisible();
});

test("private request stays Requested until the server approves; cancel withdraws it", async ({ page }) => {
  const fake = await open(page);
  await search(page, "bea");
  const bea = row(page, "Bea Lin");
  await bea.getByRole("button", { name: "Request to follow Bea Lin" }).click();
  await expect(status(page, "Request sent to Bea Lin.")).toBeVisible();
  await expect(bea).toContainText("Requested");
  await expect(bea.getByRole("button", { name: "Cancel request to follow Bea Lin" })).toBeFocused();
  expect(fake.count("POST /people/{id}/follow").map((call) => [call.body, call.csrf])).toEqual([[{}, "csrf-1"]]);

  // No timer approves anything: after a wait, the row and the server still say pending.
  await later(3000);
  await expect(bea).toContainText("Requested");
  expect(fake.people[0].outgoing).toBe("pending");

  // The owner approves on the server; the profile then shows it from a fresh read.
  fake.people[0].outgoing = "approved";
  await page.goto(`/people/${id(1)}`);
  await expect(page.getByRole("heading", { level: 1, name: "Bea Lin" })).toBeVisible();
  await expect(page.getByText("Following", { exact: true })).toBeVisible();
  await expect(page.getByRole("list", { name: "Bea Lin’s library" })).toContainText("Hollow Orchard");

  fake.people[0].outgoing = "pending";
  await page.goto("/friends");
  await search(page, "bea");
  await row(page, "Bea Lin").getByRole("button", { name: "Cancel request to follow Bea Lin" }).click();
  await expect(status(page, "Your request to follow Bea Lin was canceled.")).toBeVisible();
  await expect(row(page, "Bea Lin").getByRole("button", { name: "Request to follow Bea Lin" })).toBeVisible();
  expect(fake.people[0].outgoing).toBe("none");
});

test("public follow is immediate; unfollow is confirmed and focus returns to Follow", async ({ page }) => {
  const fake = await open(page);
  await search(page, "cal");
  const cal = row(page, "Cal Ortiz");
  await cal.getByRole("button", { name: "Follow Cal Ortiz" }).click();
  await expect(status(page, "You’re following Cal Ortiz.")).toBeVisible();
  await cal.getByRole("button", { name: "Unfollow Cal Ortiz" }).click();
  const dialog = page.getByRole("alertdialog", { name: "Unfollow Cal Ortiz?" });
  await expect(dialog).toContainText("You can follow again at any time.");
  await expect(dialog.getByRole("button", { name: "Cancel" })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(cal.getByRole("button", { name: "Unfollow Cal Ortiz" })).toBeFocused();
  expect(fake.count("DELETE /people/{id}/follow")).toHaveLength(0);

  await cal.getByRole("button", { name: "Unfollow Cal Ortiz" }).click();
  await dialog.getByRole("button", { name: "Unfollow" }).click();
  await expect(status(page, "You unfollowed Cal Ortiz.")).toBeVisible();
  await expect(cal.getByRole("button", { name: "Follow Cal Ortiz" })).toBeFocused();
});

test("a failed follow keeps the server's state; a lost response re-reads the person first", async ({ page }) => {
  const fake = new FakeApi();
  fake.replies.set("POST /people/{id}/follow", (route) => error(route, 429, "RATE_LIMITED"));
  await open(page, fake);
  await search(page, "bea");
  await row(page, "Bea Lin").getByRole("button", { name: "Request to follow Bea Lin" }).click();
  await expect(alert(page, "Couldn’t follow Bea Lin. Too many requests.")).toBeVisible();
  await expect(row(page, "Bea Lin").getByRole("button", { name: "Request to follow Bea Lin" })).toBeVisible();

  // The write commits but the response is lost: the re-read finds the pending request.
  fake.replies.set("POST /people/{id}/follow", async (route: Route) => {
    fake.people[0].outgoing = "pending";
    await route.abort("connectionreset");
  });
  await row(page, "Bea Lin").getByRole("button", { name: "Request to follow Bea Lin" }).click();
  await expect(status(page, "Request sent to Bea Lin.")).toBeVisible();
  await expect(row(page, "Bea Lin")).toContainText("Requested");
  expect(fake.count("GET /people/{id}")).toHaveLength(1);
});

test("without a handle, Friends asks for one and a refused write links to it", async ({ page }) => {
  const fake = new FakeApi();
  fake.user.handle = null;
  await open(page, fake);
  await expect(page.getByText(/People can find your name, but only followers you approve/)).toBeVisible();
  await expect(page.getByRole("link", { name: "Choose a handle" })).toHaveAttribute("href", "/onboarding/profile");
  await search(page, "cal");
  await row(page, "Cal Ortiz").getByRole("button", { name: "Follow Cal Ortiz" }).click();
  const problem = alert(page, "Couldn’t follow Cal Ortiz. Choose a handle before following people");
  await expect(problem.getByRole("link", { name: "Choose a handle" })).toHaveAttribute("href", "/onboarding/profile");
  expect(fake.people[1].outgoing).toBe("none");
});

test("incoming requests: deep link, approve, decline and no-longer-pending only change after the API answers", async ({ page }) => {
  const fake = new FakeApi();
  fake.people.push(account(6, "Gus Hale", "gus", "public", "none", "pending"), account(7, "Hal Ito", "hal", "private", "none", "pending"));
  await open(page, fake, "/friends?tab=requests");
  const tab = page.getByRole("tab", { name: "Incoming requests (3)" });
  await expect(tab).toHaveAttribute("aria-selected", "true");
  const eve = row(page, "Eve Stone");
  await expect(eve).toContainText("Wants to follow you. Approving lets them see your library, progress and activity.");

  fake.replies.set("POST /me/follow-requests/{id}", (route) => error(route, 503, "SERVICE_UNAVAILABLE"));
  await eve.getByRole("button", { name: "Approve Eve Stone" }).click();
  await expect(alert(page, "Couldn’t approve Eve Stone’s request.")).toBeVisible();
  await expect(eve).toBeVisible();
  fake.replies.clear();

  await eve.getByRole("button", { name: "Approve Eve Stone" }).click();
  await expect(status(page, "Eve Stone can now see your library, progress and activity.")).toBeVisible();
  await expect(eve).toHaveCount(0);
  await expect(page.getByRole("heading", { name: "Follow requests" })).toBeFocused();
  expect(fake.count("POST /me/follow-requests/{id}").at(-1)!.body).toEqual({ decision: "approve" });

  await row(page, "Gus Hale").getByRole("button", { name: "Decline Gus Hale" }).click();
  await expect(status(page, "Declined Gus Hale’s request. They aren’t notified.")).toBeVisible();

  // Answered elsewhere (or withdrawn) before this tab acted.
  fake.people[6].incoming = "none";
  await row(page, "Hal Ito").getByRole("button", { name: "Approve Hal Ito" }).click();
  await expect(status(page, "The request from Hal Ito is no longer pending.")).toBeVisible();
  await expect(page.getByRole("heading", { name: "No pending requests" })).toBeVisible();

  // Approval refreshed Followers, and the tab is kept in the URL.
  await page.getByRole("tab", { name: "Followers" }).click();
  await expect(page).toHaveURL("/friends?tab=followers");
  await expect(row(page, "Eve Stone")).toBeVisible();
});

test("followers: remove is confirmed, a failure keeps the row, success removes it", async ({ page }) => {
  const fake = await open(page, new FakeApi(), "/friends?tab=followers");
  const fay = row(page, "Fay Moreno");
  fake.replies.set("DELETE /me/followers/{id}", (route) => error(route, 503, "SERVICE_UNAVAILABLE"));
  await fay.getByRole("button", { name: "Remove Fay Moreno" }).click();
  const dialog = page.getByRole("alertdialog", { name: "Remove Fay Moreno as a follower?" });
  await expect(dialog).toContainText("They won’t be notified");
  await dialog.getByRole("button", { name: "Remove follower" }).click();
  await expect(dialog.getByRole("alert")).toContainText("SceneCask couldn’t be reached.");
  fake.replies.clear();
  await dialog.getByRole("button", { name: "Remove follower" }).click();
  await expect(status(page, "Fay Moreno no longer follows you.")).toBeVisible();
  await expect(fay).toHaveCount(0);
  await expect(page.getByRole("heading", { name: "Followers", level: 2 })).toBeFocused();
});

test("private profile without access: locked explanation, no library request, counts or activity", async ({ page }) => {
  const fake = await open(page, new FakeApi(), `/people/${id(1)}`);
  await expect(page.getByRole("heading", { level: 1, name: "Bea Lin" })).toBeVisible();
  await expect(page.getByText("@bea’s library is private")).toBeVisible();
  await expect(page.getByText("Request to follow to ask for access.")).toBeVisible();
  const main = page.locator(".sc-social");
  await expect(main).not.toContainText(/watched|episode|Hollow Orchard|\d+ of \d+/);
  await expect(main.getByRole("progressbar")).toHaveCount(0);
  expect(fake.count("GET /people/{id}/library")).toHaveLength(0);
  expect(fake.count("GET /activity")).toHaveLength(0);

  await page.getByRole("button", { name: "Request to follow Bea Lin" }).click();
  await expect(page.getByText("Your request is waiting for their approval.")).toBeVisible();
  expect(fake.count("GET /people/{id}/library")).toHaveLength(0);
});

test("visible profile lists the library with codes and counts only; access lost mid-read locks it", async ({ page }) => {
  const fake = await open(page, new FakeApi(), `/people/${id(3)}`);
  const library = page.getByRole("list", { name: "Dee Park’s library" });
  await expect(library).toContainText("Hollow Orchard");
  await expect(library).toContainText("3 of 8 episodes watched · Next S1 E4");
  await expect(library).toContainText("Plan to watch");
  await expect(library).toContainText("(year unknown)");
  await expect(page.getByText("You follow each other")).toBeVisible();

  fake.replies.set("GET /people/{id}/library", (route) => error(route, 404, "NOT_FOUND"));
  await page.reload();
  await expect(page.getByText("@dee’s library is private")).toBeVisible();
  await expect(page.locator(".sc-social")).not.toContainText("Hollow Orchard");
});

test("block is confirmed, has no Undo, and hides the person afterwards", async ({ page }) => {
  const fake = await open(page, new FakeApi(), `/people/${id(2)}`);
  await page.getByRole("button", { name: "Block" }).click();
  const dialog = page.getByRole("alertdialog", { name: "Block Cal Ortiz?" });
  await expect(dialog).toContainText("Follows and requests between you are removed both ways.");
  await expect(dialog).toContainText("You can unblock them in Settings, but follows won’t come back.");
  await dialog.getByRole("button", { name: "Block" }).click();
  await expect(page.getByRole("heading", { level: 1, name: "You blocked Cal Ortiz" })).toBeFocused();
  await expect(page.getByRole("link", { name: "Manage blocked people in Settings" })).toHaveAttribute("href", "/settings#sc-settings-blocked");
  await expect(page.getByRole("button", { name: /Undo/ })).toHaveCount(0);
  expect(fake.count("PUT /me/blocks/{id}").map((call) => call.body)).toEqual([{}]);

  await page.reload();
  await expect(page.getByRole("heading", { name: "This person isn’t available" })).toBeVisible();
  await expect(page.locator(".sc-social")).not.toContainText("Cal");
  await page.goto("/friends");
  await search(page, "cal");
  await expect(page.getByRole("heading", { name: "No one matches “cal”" })).toBeVisible();
});

test("feed slots: activity and suggestions from the API with codes only; no friends shows a search prompt", async ({ page }) => {
  const fake = await open(page, new FakeApi(), "/feed");
  const activity = page.getByRole("list", { name: "Recent activity" });
  await expect(activity.getByRole("listitem").first()).toHaveText(/Dee Park watched Hollow Orchard S1 E3\s*Oct 12/);
  await expect(activity.getByRole("listitem").nth(1)).toContainText("Dee Park saved Lantern Coast");
  await expect(activity.locator("time").first()).toHaveAttribute("title", "Oct 12, 2026, 6:30 PM UTC");
  await expect(page.getByRole("list", { name: "Shows saved by people you follow" })).toContainText("Saved by Dee Park and Fay Moreno");
  expect(fake.mutations()).toEqual([]);

  await page.unrouteAll();
  const empty = new FakeApi();
  empty.activity = [];
  empty.recommendations = [];
  await open(page, empty, "/feed");
  await expect(page.getByRole("heading", { name: "Nothing from people you follow yet" })).toBeVisible();
  await expect(page.getByText("Your tracking works the same without them.")).toBeVisible();
  await expect(page.getByRole("heading", { name: "No suggestions from friends yet" })).toBeVisible();
  await expect(page.getByRole("link", { name: "Find people" })).toHaveCount(2);

  await page.unrouteAll();
  const failing = new FakeApi();
  failing.replies.set("GET /activity", (route) => error(route, 503, "SERVICE_UNAVAILABLE"));
  await open(page, failing, "/feed");
  await expect(alert(page, "Activity didn’t load")).toBeVisible();
  failing.replies.clear();
  await page.getByRole("button", { name: "Retry" }).click();
  await expect(page.getByRole("list", { name: "Recent activity" })).toBeVisible();
});

test("an expired session goes to sign-in with a return path", async ({ page }) => {
  const fake = new FakeApi();
  fake.signedIn = false;
  await open(page, fake, "/friends?tab=requests");
  await expect(page).toHaveURL(/\/auth\/signin\?reason=expired&returnTo=%2Ffriends%3Ftab%3Drequests/);
});

/** D09 states per screen: where it lives, an optional change to the fake, and how to reach it. */
const SCREENS: Record<string, { path: string; setup?: (fake: FakeApi) => void; reach: (page: Page) => Promise<void> }> = {
  "find-people": {
    path: "/friends",
    reach: async (page) => {
      await search(page, "ea");
      await expect(row(page, "Bea Lin")).toBeVisible();
    },
  },
  requests: { path: "/friends?tab=requests", reach: (page) => expect(row(page, "Eve Stone")).toBeVisible() },
  "profile-locked": { path: `/people/${id(1)}`, reach: (page) => expect(page.getByText("@bea’s library is private")).toBeVisible() },
  "profile-library": {
    path: `/people/${id(3)}`,
    reach: (page) => expect(page.getByRole("list", { name: "Dee Park’s library" })).toBeVisible(),
  },
  "block-confirm": {
    path: `/people/${id(2)}`,
    reach: async (page) => {
      await page.getByRole("button", { name: "Block" }).click();
      await expect(page.getByRole("alertdialog")).toBeVisible();
    },
  },
  feed: { path: "/feed", reach: (page) => expect(page.getByRole("list", { name: "Recent activity" })).toBeVisible() },
  "followers-error": {
    path: "/friends?tab=followers",
    setup: (fake) => fake.replies.set("GET /me/followers", (route) => error(route, 503, "SERVICE_UNAVAILABLE")),
    reach: (page) => expect(page.getByRole("heading", { name: "Your followers didn’t load" })).toBeVisible(),
  },
};

/** Buttons, fields and standalone links are 44px targets; inline links in sentences are exempt (WCAG 2.5.8). */
async function expectTargets(page: Page, screen: string) {
  const targets = page
    .locator("main button, .sc-modal button, main input, main a.sc-button, .sc-social-person-name a, .sc-social-show-title, .sc-social-activity a")
    .filter({ visible: true });
  for (const target of await targets.all()) {
    const label = await target.evaluate((node) => node.outerHTML.slice(0, 80));
    expect((await target.boundingBox())!.height, `${screen} ${label}`).toBeGreaterThanOrEqual(44);
  }
}

responsiveTests("responsive social states, targets, axe and screenshots", async (page, width, shots) => {
  for (const [name, screen] of Object.entries(SCREENS)) {
    const fake = new FakeApi();
    screen.setup?.(fake);
    await page.unrouteAll();
    await open(page, fake, screen.path);
    await screen.reach(page);
    await expectNoPageOverflow(page, name);
    // D08: the 760px column inside the gutters; the desktop nav from 860px. /feed has no shell.
    if (screen.path !== "/feed") {
      const column = (await page.locator(".sc-social").boundingBox())!;
      expect(column.width <= 760 && column.x >= 20, `${name} column`).toBe(true);
      if (width >= 860) await expect(page.locator(".sc-nav")).toBeVisible();
    }
    await expectTargets(page, name);
    if (!shots) continue;
    expect(await axeViolations(page), name).toEqual([]);
    await page.evaluate(() => scrollTo(0, 0));
    await page.screenshot({ path: `test-results/r28/${name}-${width}.png`, fullPage: true });
  }
});
