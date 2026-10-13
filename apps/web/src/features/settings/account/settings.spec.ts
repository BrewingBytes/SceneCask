import { expect, test, type Page, type Request, type Route } from "@playwright/test";
import {
  axeViolations,
  error,
  expectNoPageOverflow,
  json,
  later,
  responsiveTests,
} from "../../tracking/fixture-api";

/**
 * Component suite for R29. The production /settings route runs in an isolated fixture app with
 * the beta flag on (/alpha/settings renders it off); a stateful fake of the C04/C06 endpoints
 * answers at the /api/v1 network boundary, so the generated client, CSRF bootstrap and token
 * rotation are exercised. This is not live integration (R38 owns linking and live privacy).
 */

const PASSWORD = "correct horse battery";
const id = (n: number) => `20000000-0000-4000-8000-${String(n).padStart(12, "0")}`;
const person = (n: number, displayName: string, handle: string) => ({
  id: id(n),
  handle,
  displayName,
  initials: displayName
    .split(" ")
    .map((word) => word[0])
    .join(""),
  visibility: "private" as const,
  relationship: { outgoing: "none" as const, incoming: "approved" as const, mutual: false },
});

interface Call {
  key: string;
  body: unknown;
  csrf: string | null;
  url: URL;
}
type Override = (route: Route, request: Request) => Promise<unknown> | unknown;

/** C04/C06 settings endpoints. Reauth rotates the session's CSRF token, as the API does. */
class FakeApi {
  user = {
    id: id(0),
    email: "ana@example.test",
    displayName: "Ana Rivera",
    handle: "ana_r" as string | null,
    visibility: "private" as "private" | "public",
    verified: true,
    role: "member",
  };
  password = true;
  google = true;
  fresh = false;
  signedIn = true;
  csrf = "csrf-1";
  followers = [person(1, "Bea Lin", "bea"), person(2, "Cal Ortiz", "cal")];
  blocks = [{ id: id(3), handle: "dee", displayName: "Dee Park" }];
  pageSize = 50;
  calls: Call[] = [];
  overrides = new Map<string, Override>();

  of(key: string) {
    return this.calls.filter((call) => call.key === key);
  }
  writes() {
    return this.calls.filter((call) => !call.key.startsWith("GET "));
  }
  page<T extends { id: string }>(items: T[], url: URL) {
    const start = Number(url.searchParams.get("cursor") ?? 0);
    const end = start + this.pageSize;
    return { items: items.slice(start, end), nextCursor: end < items.length ? String(end) : null };
  }

  async handle(route: Route, request: Request) {
    const url = new URL(request.url());
    const path = url.pathname.replace("/api/v1", "");
    const method = request.method();
    const key = `${method} ${path.replace(/[0-9a-f]{8}-[0-9a-f-]{27}$/, "{id}")}`;
    const body = request.postData() ? request.postDataJSON() : undefined;
    const csrf = request.headers()["x-csrf-token"] ?? null;
    this.calls.push({ key, body, csrf, url });
    const override = this.overrides.get(key);
    if (override) return override(route, request);
    if (key === "GET /session") return json(route, { user: this.signedIn ? this.user : null, csrfToken: this.csrf });
    if (!this.signedIn) return error(route, 401, "AUTH_REQUIRED");
    if (method !== "GET" && key !== "GET /auth/google/start" && csrf !== this.csrf) return error(route, 403, "CSRF_FAILED");
    return this.answer(route, key, path, body as Record<string, unknown> | undefined, url);
  }

  answer(route: Route, key: string, path: string, body: Record<string, unknown> | undefined, url: URL) {
    const target = path.split("/").pop()!;
    switch (key) {
      case "GET /me/identities":
        return json(route, { passwordEnabled: this.password, googleLinked: this.google });
      case "PATCH /me":
        if (this.user.handle && body!.handle !== this.user.handle) return error(route, 422, "VALIDATION_ERROR");
        Object.assign(this.user, { displayName: body!.displayName, handle: body!.handle });
        return json(route, this.user);
      case "POST /auth/reauth":
        if (body!.password !== PASSWORD) return error(route, 401, "INVALID_CREDENTIALS");
        this.fresh = true;
        this.csrf = "csrf-2";
        return route.fulfill({ status: 204 });
      case "DELETE /me/identities/google":
        if (!this.fresh) return error(route, 401, "AUTH_REQUIRED");
        if (!this.password) return error(route, 409, "LAST_SIGNIN_METHOD");
        this.google = false;
        return route.fulfill({ status: 204 });
      case "POST /auth/password-reset-request":
        return json(route, { message: "Check your email." }, 202);
      case "POST /auth/logout":
        this.signedIn = false;
        return route.fulfill({ status: 204 });
      case "PATCH /me/privacy":
        this.user.visibility = body!.visibility as "private" | "public";
        return json(route, { visibility: this.user.visibility });
      case "GET /me/followers":
        return json(route, this.page(this.followers, url));
      case "DELETE /me/followers/{id}":
        this.followers = this.followers.filter((item) => item.id !== target);
        return route.fulfill({ status: 204 });
      case "GET /me/blocks":
        return json(route, this.page(this.blocks, url));
      case "DELETE /me/blocks/{id}":
        this.blocks = this.blocks.filter((item) => item.id !== target);
        return route.fulfill({ status: 204 });
      case "GET /auth/google/start": {
        // Google is skipped: a fresh session links at once and the callback redirects back.
        if (url.searchParams.get("intent") === "link" && this.fresh) this.google = true;
        return route.fulfill({ status: 303, headers: { Location: url.searchParams.get("returnTo")! } });
      }
    }
    return error(route, 404, "NOT_FOUND");
  }
}

async function open(page: Page, fake = new FakeApi(), path = "/settings") {
  await page.route("**/api/v1/**", (route, request) => fake.handle(route, request));
  await page.goto(path);
  await expect(page.getByRole("heading", { level: 2, name: "Sign-in methods" })).toBeVisible();
  return fake;
}

const section = (page: Page, name: string) => page.getByRole("region", { name });
const status = (page: Page, text: string | RegExp) => page.getByRole("status").filter({ hasText: text });
const alert = (page: Page, text: string | RegExp) => page.getByRole("alert").filter({ hasText: text });
const radio = (page: Page, name: RegExp) => page.getByRole("radio", { name });

test.beforeEach(async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
});

test("beta shows every D04 section from API data; with beta off the social sections and requests are absent", async ({ page }) => {
  const fake = await open(page);
  for (const name of ["Profile", "Sign-in methods", "Privacy", "Followers", "Blocked people", "Your data", "About"])
    await expect(page.getByRole("heading", { level: 2, name })).toBeVisible();
  await expect(section(page, "Followers").getByText("Bea Lin")).toBeVisible();
  await expect(section(page, "Blocked people").getByText("@dee")).toBeVisible();
  await expect(radio(page, /^Private/)).toBeChecked();
  await expect(page.getByRole("link", { name: "Manage your data" })).toHaveAttribute("href", "/settings/data");
  const about = section(page, "About");
  await expect(about).toContainText("This product uses the TMDB API but is not endorsed or certified by TMDB.");
  await expect(about.getByRole("link", { name: "TMDB website" })).toHaveAttribute("href", "https://www.themoviedb.org/");
  await expect(about.getByRole("link", { name: "About SceneCask" })).toHaveAttribute("href", "/about");

  await page.unrouteAll();
  const alpha = await open(page, new FakeApi(), "/alpha/settings");
  for (const name of ["Privacy", "Followers", "Blocked people", "Your data"])
    await expect(page.getByRole("heading", { level: 2, name })).toHaveCount(0);
  await expect(page.getByRole("heading", { level: 2, name: "About" })).toBeVisible();
  // Development Strict Mode runs the load effect twice; only which endpoints were read matters.
  expect([...new Set(alpha.calls.map((call) => call.key))].sort()).toEqual(["GET /me/identities", "GET /session"]);
  expect(fake.writes()).toEqual([]);
});

test("profile: the display name saves with the fixed handle, which is read-only", async ({ page }) => {
  const fake = await open(page);
  const profile = section(page, "Profile");
  await expect(profile.getByRole("textbox", { name: "Handle" })).toHaveAttribute("readonly", "");
  await expect(profile.getByRole("textbox", { name: "Handle" })).toHaveValue("@ana_r");
  await profile.getByRole("textbox", { name: "Display name" }).fill("   ");
  await profile.getByRole("button", { name: "Save profile" }).click();
  await expect(profile.getByText("Enter the name people will see.")).toBeVisible();
  await expect(profile.getByRole("textbox", { name: "Display name" })).toBeFocused();
  expect(fake.of("PATCH /me")).toHaveLength(0);

  await profile.getByRole("textbox", { name: "Display name" }).fill("  Ana R  ");
  await profile.getByRole("button", { name: "Save profile" }).click();
  await expect(status(page, "Profile saved.")).toBeVisible();
  expect(fake.of("PATCH /me").map((call) => call.body)).toEqual([{ displayName: "Ana R", handle: "ana_r" }]);
  await expect(profile.getByRole("textbox", { name: "Display name" })).toHaveValue("Ana R");
});

test("profile: an account without a handle chooses one here; a taken handle is an inline error", async ({ page }) => {
  const fake = new FakeApi();
  fake.user.handle = null;
  fake.overrides.set("PATCH /me", (route, request) =>
    (request.postDataJSON() as { handle: string }).handle === "taken"
      ? error(route, 409, "HANDLE_TAKEN")
      : fake.answer(route, "PATCH /me", "/me", request.postDataJSON(), new URL(request.url())),
  );
  await open(page, fake);
  const profile = section(page, "Profile");
  const handle = profile.getByRole("textbox", { name: "Handle" });
  await expect(handle).not.toHaveAttribute("readonly", "");
  await handle.fill("taken");
  await profile.getByRole("button", { name: "Save profile" }).click();
  await expect(profile.getByText("That handle is taken. Try another.")).toBeVisible();
  await handle.fill("ana_new");
  await profile.getByRole("button", { name: "Save profile" }).click();
  await expect(status(page, "Profile saved. Your handle is @ana_new.")).toBeVisible();
  await expect(profile.getByRole("textbox", { name: "Handle" })).toHaveValue("@ana_new");
  await expect(profile.getByRole("textbox", { name: "Handle" })).toHaveAttribute("readonly", "");
});

test("last sign-in method: Unlink Google is blocked with an explanation and sends nothing", async ({ page }) => {
  const fake = new FakeApi();
  fake.password = false;
  await open(page, fake);
  const signin = section(page, "Sign-in methods");
  const unlink = signin.getByRole("button", { name: "Unlink Google" });
  await expect(unlink).toHaveAttribute("aria-disabled", "true");
  await expect(unlink).toHaveAccessibleDescription(/Google is the only way you sign in, so it can’t be unlinked/);
  // aria-disabled keeps it focusable and announced, so it can still be activated.
  await unlink.click({ force: true });
  await expect(page.getByRole("alertdialog")).toHaveCount(0);
  expect(fake.writes()).toEqual([]);
});

test("last sign-in method: the API's 409 is explained in the dialog and the methods are re-read", async ({ page }) => {
  const fake = new FakeApi();
  fake.fresh = true;
  await open(page, fake);
  // Another device removed the password after this page loaded.
  fake.password = false;
  const reads = fake.of("GET /me/identities").length;
  await section(page, "Sign-in methods").getByRole("button", { name: "Unlink Google" }).click();
  const dialog = page.getByRole("alertdialog", { name: "Unlink Google?" });
  await dialog.getByRole("button", { name: "Unlink Google" }).click();
  await expect(dialog.getByRole("alert")).toHaveText(/Set a password first, then unlink Google/);
  await expect.poll(() => fake.of("GET /me/identities").length).toBe(reads + 1);
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await expect(section(page, "Sign-in methods").getByRole("button", { name: "Unlink Google" })).toHaveAttribute("aria-disabled", "true");
  expect(fake.google).toBe(true);
});

test("unlink with a stale sign-in: reauth first, then the pending unlink resumes on the rotated token", async ({ page }) => {
  const fake = await open(page);
  const signin = section(page, "Sign-in methods");
  await signin.getByRole("button", { name: "Unlink Google" }).click();
  const confirm = page.getByRole("alertdialog", { name: "Unlink Google?" });
  await expect(confirm.getByRole("button", { name: "Cancel" })).toBeFocused();
  await confirm.getByRole("button", { name: "Unlink Google" }).click();

  const reauth = page.getByRole("dialog", { name: "Confirm it’s you" });
  await expect(reauth.getByRole("button", { name: "Continue with Google" })).toBeVisible();
  await reauth.getByLabel("Password", { exact: true }).fill("wrong password");
  await reauth.getByRole("button", { name: "Confirm" }).click();
  await expect(reauth.getByText("That password isn’t right.")).toBeVisible();
  expect(fake.google).toBe(true);
  await reauth.getByLabel("Password", { exact: true }).fill(PASSWORD);
  await reauth.getByRole("button", { name: "Confirm" }).click();

  await expect(status(page, "Google is unlinked.")).toBeVisible();
  await expect(confirm).toHaveCount(0);
  await expect(signin.getByText("Not linked. Link it to sign in with Google too.")).toBeVisible();
  // The first attempt was refused as stale; the resumed one hit the old token, refreshed and retried once.
  expect(fake.of("DELETE /me/identities/google").map((call) => call.csrf)).toEqual(["csrf-1", "csrf-1", "csrf-2"]);
  expect(fake.google).toBe(false);
});

test("Google reauth returns to Settings and resumes the unlink confirmation; failures are explained", async ({ page }) => {
  const fake = await open(page);
  await section(page, "Sign-in methods").getByRole("button", { name: "Unlink Google" }).click();
  await page.getByRole("alertdialog").getByRole("button", { name: "Unlink Google" }).click();
  await page.getByRole("dialog", { name: "Confirm it’s you" }).getByRole("button", { name: "Continue with Google" }).click();
  await expect.poll(() => fake.of("GET /auth/google/start").length).toBe(1);
  expect(Object.fromEntries(fake.of("GET /auth/google/start")[0].url.searchParams)).toEqual({
    intent: "reauth",
    returnTo: "/settings?google=unlink",
  });
  // The fake skips Google, so the session is not fresh: the resumed confirmation shows and the URL is clean.
  await expect(page.getByRole("alertdialog", { name: "Unlink Google?" })).toBeVisible();
  await expect(page).toHaveURL("/settings");

  await page.goto("/settings?google=unlink&error=reauth_mismatch");
  await expect(alert(page, "That’s a different Google account")).toBeVisible();
  await expect(page.getByRole("alertdialog")).toHaveCount(0);
  await expect(page).toHaveURL("/settings");
});

test("link Google: confirm with the password, then Google's link flow returns with the result", async ({ page }) => {
  const fake = new FakeApi();
  fake.google = false;
  await open(page, fake);
  await section(page, "Sign-in methods").getByRole("button", { name: "Link Google" }).click();
  const reauth = page.getByRole("dialog", { name: "Confirm it’s you" });
  // Google can't confirm an identity that isn't linked yet.
  await expect(reauth.getByRole("button", { name: "Continue with Google" })).toHaveCount(0);
  await reauth.getByLabel("Password", { exact: true }).fill(PASSWORD);
  await reauth.getByRole("button", { name: "Confirm" }).click();
  await expect(status(page, "Google is linked. You can now sign in with Google.")).toBeVisible();
  await expect(page).toHaveURL("/settings");
  expect(Object.fromEntries(fake.of("GET /auth/google/start")[0].url.searchParams)).toEqual({
    intent: "link",
    returnTo: "/settings?google=link",
  });

  // A link that comes back with an error says why, and the identities still decide what is shown.
  fake.google = false;
  await page.goto("/settings?google=link&error=identity_in_use");
  await expect(alert(page, "That Google account is linked elsewhere")).toBeVisible();
  await expect(section(page, "Sign-in methods").getByRole("button", { name: "Link Google" })).toBeVisible();
  await expect(status(page, "Google is linked")).toHaveCount(0);
});

test("Google-only account sets a password through a verified email link", async ({ page }) => {
  const fake = new FakeApi();
  fake.password = false;
  await open(page, fake);
  await section(page, "Sign-in methods").getByRole("button", { name: "Set a password" }).click();
  await expect(status(page, "We sent a link to ana@example.test. Open it to set a password.")).toBeVisible();
  expect(fake.of("POST /auth/password-reset-request").map((call) => call.body)).toEqual([{ email: "ana@example.test" }]);

  fake.overrides.set("POST /auth/password-reset-request", (route) =>
    route.fulfill({ status: 429, headers: { "Retry-After": "30" }, contentType: "application/json", body: "{}" }),
  );
  await section(page, "Sign-in methods").getByRole("button", { name: "Set a password" }).click();
  await expect(alert(page, "Too many changes. Try again in 30 seconds.")).toBeVisible();
});

test("public switch: the warning comes before any write; cancel keeps Private", async ({ page }) => {
  const fake = await open(page);
  await radio(page, /^Public/).check();
  const dialog = page.getByRole("alertdialog", { name: "Make your account public?" });
  await expect(dialog).toContainText(
    "Any signed-in SceneCask member can see your library and activity. Discussions still require mutual follows.",
  );
  expect(fake.of("PATCH /me/privacy")).toHaveLength(0);
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expect(radio(page, /^Private/)).toBeChecked();
  expect(fake.writes()).toEqual([]);

  await radio(page, /^Public/).check();
  fake.overrides.set("PATCH /me/privacy", async (route, request) => {
    await later(300);
    return fake.answer(route, "PATCH /me/privacy", "/me/privacy", request.postDataJSON(), new URL(request.url()));
  });
  await dialog.getByRole("button", { name: "Make public" }).click();
  await expect(status(page, "Saving your privacy setting…")).toBeVisible();
  await expect(page.locator(".sc-settings-privacy")).toHaveAttribute("aria-busy", "true");
  await expect(radio(page, /^Public/)).toBeFocused();
  // A change while saving is ignored rather than queued.
  await radio(page, /^Private/).click();
  await expect(radio(page, /^Public/)).toBeChecked();
  await expect(status(page, "Your account is now public.")).toBeVisible();
  await expect(radio(page, /^Public/)).toBeChecked();
  expect(fake.of("PATCH /me/privacy").map((call) => call.body)).toEqual([{ visibility: "public" }]);

  // Going private needs no warning; approved followers keep access.
  await radio(page, /^Private/).check();
  await expect(status(page, "Your account is now private. Followers you’ve approved keep access.")).toBeVisible();
  expect(fake.of("PATCH /me/privacy").map((call) => call.body)).toEqual([{ visibility: "public" }, { visibility: "private" }]);
});

test("public switch: a failed save keeps the authoritative visibility and offers Try again", async ({ page }) => {
  const fake = await open(page);
  fake.overrides.set("PATCH /me/privacy", (route) => error(route, 503, "SERVICE_UNAVAILABLE"));
  await radio(page, /^Public/).check();
  await page.getByRole("alertdialog").getByRole("button", { name: "Make public" }).click();
  await expect(alert(page, "Your account is still private.")).toBeVisible();
  await expect(radio(page, /^Private/)).toBeChecked();
  expect(fake.user.visibility).toBe("private");

  fake.overrides.delete("PATCH /me/privacy");
  await section(page, "Privacy").getByRole("button", { name: "Try again" }).click();
  // Retrying a public switch shows the warning again before writing.
  await page.getByRole("alertdialog").getByRole("button", { name: "Make public" }).click();
  await expect(status(page, "Your account is now public.")).toBeVisible();
  await expect(alert(page, "Your account is still private.")).toHaveCount(0);
});

test("followers: removal is confirmed (no notification), changes only after the API accepts", async ({ page }) => {
  const fake = await open(page);
  const followers = section(page, "Followers");
  await followers.getByRole("button", { name: "Remove Bea Lin" }).click();
  const dialog = page.getByRole("alertdialog", { name: "Remove Bea Lin as a follower?" });
  await expect(dialog).toContainText("loses access to your library and activity");
  await expect(dialog).toContainText("They won’t be notified");

  fake.overrides.set("DELETE /me/followers/{id}", (route) => error(route, 503, "SERVICE_UNAVAILABLE"));
  await dialog.getByRole("button", { name: "Remove follower" }).click();
  await expect(dialog.getByRole("alert")).toHaveText(/couldn’t be reached/);
  await expect(followers.getByText("Bea Lin")).toBeVisible();

  fake.overrides.delete("DELETE /me/followers/{id}");
  await dialog.getByRole("button", { name: "Remove follower" }).click();
  await expect(status(page, "Bea Lin no longer follows you.")).toBeVisible();
  await expect(followers.getByRole("listitem").filter({ hasText: "Bea Lin" })).toHaveCount(0);
  await expect(followers.getByRole("heading", { name: "Followers" })).toBeFocused();
  expect(fake.of("DELETE /me/followers/{id}").map((call) => call.url.pathname)).toEqual([
    `/api/v1/me/followers/${id(1)}`,
    `/api/v1/me/followers/${id(1)}`,
  ]);
});

test("followers: loading, error with Retry, empty and Show more states", async ({ page }) => {
  const fake = new FakeApi();
  let fail = true;
  fake.overrides.set("GET /me/followers", async (route, request) => {
    await later(200);
    if (fail) return error(route, 503, "SERVICE_UNAVAILABLE");
    return fake.answer(route, "GET /me/followers", "/me/followers", undefined, new URL(request.url()));
  });
  fake.pageSize = 1;
  await open(page, fake);
  const followers = section(page, "Followers");
  await expect(status(page, "Loading your followers…")).toBeAttached();
  await expect(followers.getByRole("heading", { name: "Your followers didn’t load" })).toBeVisible();
  fail = false;
  await followers.getByRole("button", { name: "Retry" }).click();
  await expect(followers.getByText("Bea Lin")).toBeVisible();
  await followers.getByRole("button", { name: "Show more" }).click();
  await expect(followers.getByText("Cal Ortiz")).toBeVisible();
  await expect(followers.getByRole("button", { name: "Show more" })).toHaveCount(0);

  await page.unrouteAll();
  const none = new FakeApi();
  none.followers = [];
  none.blocks = [];
  await open(page, none);
  await expect(section(page, "Followers").getByRole("heading", { name: "No followers yet" })).toBeVisible();
  await expect(section(page, "Blocked people").getByRole("heading", { name: "You haven’t blocked anyone" })).toBeVisible();
});

test("unblock: the confirmation says follows won't come back; only the block is removed", async ({ page }) => {
  const fake = await open(page);
  await section(page, "Blocked people").getByRole("button", { name: "Unblock Dee Park" }).click();
  const dialog = page.getByRole("alertdialog", { name: "Unblock Dee Park?" });
  await expect(dialog).toContainText("Follows between you were removed when you blocked them and won’t come back.");
  await dialog.getByRole("button", { name: "Unblock" }).click();
  await expect(status(page, "Dee Park is unblocked. Previous follows weren’t restored.")).toBeVisible();
  await expect(section(page, "Blocked people").getByRole("heading", { name: "You haven’t blocked anyone" })).toBeVisible();
  expect(fake.writes().map((call) => call.key)).toEqual(["DELETE /me/blocks/{id}"]);
  expect(fake.followers).toHaveLength(2);
});

test("an expired session leaves for sign-in with a return path; Sign out ends the session", async ({ page }) => {
  const expired = new FakeApi();
  expired.signedIn = false;
  await page.route("**/api/v1/**", (route, request) => expired.handle(route, request));
  await page.goto("/settings");
  await expect(page).toHaveURL("/auth/signin?reason=expired&returnTo=%2Fsettings");

  await page.unrouteAll();
  const fake = await open(page);
  await section(page, "Sign-in methods").getByRole("button", { name: "Sign out" }).click();
  await expect(page).toHaveURL("/auth/signin");
  expect(fake.of("POST /auth/logout").map((call) => call.csrf)).toEqual(["csrf-1"]);
});

test("a failed load offers Retry and shows no account data", async ({ page }) => {
  const fake = new FakeApi();
  fake.overrides.set("GET /me/identities", (route) => error(route, 503, "SERVICE_UNAVAILABLE"));
  await page.route("**/api/v1/**", (route, request) => fake.handle(route, request));
  await page.goto("/settings");
  await expect(page.getByRole("heading", { name: "Your settings didn’t load" })).toBeVisible();
  await expect(page.getByText("ana@example.test")).toHaveCount(0);
  fake.overrides.delete("GET /me/identities");
  await page.getByRole("button", { name: "Retry" }).click();
  await expect(page.getByRole("heading", { level: 2, name: "Sign-in methods" })).toBeVisible();
});

test("keyboard: dialogs trap focus, Escape cancels, and focus returns to the trigger", async ({ page }) => {
  await open(page);
  const trigger = section(page, "Blocked people").getByRole("button", { name: "Unblock Dee Park" });
  await trigger.focus();
  await page.keyboard.press("Enter");
  const dialog = page.getByRole("alertdialog", { name: "Unblock Dee Park?" });
  await expect(dialog.getByRole("button", { name: "Cancel" })).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(dialog.getByRole("button", { name: "Unblock" })).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(dialog.getByRole("button", { name: "Cancel" })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expect(trigger).toBeFocused();
});

/** Each state with its fake, reached through the real route. */
const SCREENS: { name: string; fake: () => FakeApi; reach: (page: Page) => Promise<unknown> }[] = [
  { name: "settings", fake: () => new FakeApi(), reach: async () => {} },
  {
    name: "last-method",
    fake: () => Object.assign(new FakeApi(), { password: false }),
    reach: async (page) => page.getByRole("heading", { level: 2, name: "Sign-in methods" }).scrollIntoViewIfNeeded(),
  },
  {
    name: "public-confirm",
    fake: () => new FakeApi(),
    reach: async (page) => {
      await radio(page, /^Public/).check();
      await expect(page.getByRole("alertdialog")).toBeVisible();
    },
  },
  {
    name: "privacy-failed",
    fake: () => {
      const fake = new FakeApi();
      fake.overrides.set("PATCH /me/privacy", (route) => error(route, 503, "SERVICE_UNAVAILABLE"));
      return fake;
    },
    reach: async (page) => {
      await radio(page, /^Public/).check();
      await page.getByRole("alertdialog").getByRole("button", { name: "Make public" }).click();
      await expect(alert(page, "Your account is still private.")).toBeVisible();
    },
  },
  {
    name: "unblock-confirm",
    fake: () => new FakeApi(),
    reach: async (page) => {
      await section(page, "Blocked people").getByRole("button", { name: "Unblock Dee Park" }).click();
      await expect(page.getByRole("alertdialog")).toBeVisible();
    },
  },
  {
    name: "empty-error",
    fake: () => {
      const fake = new FakeApi();
      fake.blocks = [];
      fake.overrides.set("GET /me/followers", (route) => error(route, 503, "SERVICE_UNAVAILABLE"));
      return fake;
    },
    reach: (page) => expect(page.getByRole("heading", { name: "Your followers didn’t load" })).toBeVisible(),
  },
];

responsiveTests("responsive settings states, targets, axe and screenshots", async (page, width, shots) => {
  for (const screen of SCREENS) {
    await page.unrouteAll();
    await open(page, screen.fake());
    await screen.reach(page);
    await expectNoPageOverflow(page, screen.name);
    const column = (await page.locator(".sc-settings").boundingBox())!;
    // D04: a 760px column on desktop; full width inside 20px gutters on phones.
    expect(column.width, screen.name).toBeLessThanOrEqual(760);
    expect(column.x, screen.name).toBeGreaterThanOrEqual(20);
    const nav = page.locator(".sc-nav");
    if (width >= 860) await expect(nav).toBeVisible();
    for (const control of await page.locator(".sc-settings button:visible, .sc-modal button:visible, .sc-settings input:not([type=radio]):visible, .sc-settings .sc-choice").all()) {
      const box = (await control.boundingBox())!;
      const name = `${screen.name} ${await control.evaluate((node) => node.outerHTML.slice(0, 80))}`;
      expect(box.height, name).toBeGreaterThanOrEqual(44);
    }
    if (shots) {
      expect(await axeViolations(page), screen.name).toEqual([]);
      // Full-page captures draw fixed layers at the scroll offset; capture from the top.
      await page.evaluate(() => scrollTo(0, 0));
      await page.screenshot({ path: `test-results/r29/${screen.name}-${width}.png`, fullPage: true });
    }
  }
});
