import axe from "axe-core";
import { expect, test, type Page, type Request, type Route } from "@playwright/test";
import { safeReturnTo } from "./routes";

/**
 * Component suite for R10 account screens. The production routes run in an isolated fixture
 * app; responses are supplied at the /api/v1 network boundary, so the generated client, CSRF
 * bootstrap and Origin are exercised. This is not live integration (R24 owns that gate).
 */

const ORIGIN = "http://127.0.0.1:3120";
const ANON_CSRF = "csrf-anonymous";
const SIGNED_CSRF = "csrf-signed-in";
const PASSWORD = "correct horse battery";
const REQUEST_ID = "30000000-0000-4000-8000-000000000001";

type User = {
  id: string;
  email: string;
  displayName: string;
  handle: string | null;
  visibility: "private";
  verified: true;
  role: "member";
};
const user = (overrides: Partial<User> = {}): User => ({
  id: "10000000-0000-4000-8000-000000000001",
  email: "ana@example.test",
  displayName: "Ana",
  handle: null,
  visibility: "private",
  verified: true,
  role: "member",
  ...overrides,
});

function json(route: Route, body: unknown, status = 200, headers: Record<string, string> = {}) {
  return route.fulfill({ status, headers: { ...headers, "content-type": "application/json" }, body: JSON.stringify(body) });
}
/** The API's error envelope. Its message is a canary: no screen may ever show it. */
function error(route: Route, status: number, code: string, fields?: Record<string, string>, headers = {}) {
  const envelope = { code, message: "SERVER-MESSAGE", fields, requestId: REQUEST_ID };
  return json(route, { error: envelope }, status, headers);
}
const later = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

type Handler = (route: Route, request: Request) => Promise<unknown> | unknown;
type Endpoint =
  | "session"
  | "register"
  | "verify"
  | "resend"
  | "login"
  | "resetRequest"
  | "reset"
  | "reauth"
  | "profile"
  | "library"
  | "google";
const ENDPOINTS: Record<string, Endpoint> = {
  "GET /session": "session",
  "POST /auth/register": "register",
  "POST /auth/verify": "verify",
  "POST /auth/verification-resend": "resend",
  "POST /auth/login": "login",
  "POST /auth/password-reset-request": "resetRequest",
  "POST /auth/password-reset": "reset",
  "POST /auth/reauth": "reauth",
  "PATCH /me": "profile",
  "GET /library": "library",
  "GET /auth/google/start": "google",
};
interface Call {
  endpoint: Endpoint;
  url: URL;
  body: unknown;
  csrf: string | null;
  origin: string | null;
}

/**
 * A small stateful API: sign-in, verify and reauth start a session; reset ends it. Tests
 * override one endpoint at a time and read the call log.
 */
async function mockApi(page: Page, handlers: Partial<Record<Endpoint, Handler>> = {}) {
  const state: { user: User | null } = { user: null };
  const calls: Call[] = [];
  const signIn = (route: Route, signedIn: User) => {
    state.user = signedIn;
    return json(route, { user: signedIn, csrfToken: SIGNED_CSRF });
  };
  const defaults: Record<Endpoint, Handler> = {
    session: (route) => json(route, { user: state.user, csrfToken: state.user ? SIGNED_CSRF : ANON_CSRF }),
    register: (route) => json(route, { message: "Check your email." }, 202),
    verify: (route) => signIn(route, user()),
    resend: (route) => json(route, { message: "Check your email." }, 202),
    login: (route) => signIn(route, user({ handle: "ana_r" })),
    resetRequest: (route) => json(route, { message: "Check your email." }, 202),
    reset: (route) => {
      state.user = null;
      return route.fulfill({ status: 204 });
    },
    reauth: (route) => route.fulfill({ status: 204 }),
    profile: (route, request) => json(route, { ...state.user!, ...(request.postDataJSON() as object) }),
    library: (route) => json(route, { items: [], nextCursor: null }),
    google: (route) => route.fulfill({ status: 303, headers: { Location: "/home" } }),
  };
  await page.route("**/api/v1/**", async (route, request) => {
    const url = new URL(request.url());
    const endpoint = ENDPOINTS[`${request.method()} ${url.pathname.replace("/api/v1", "")}`];
    if (!endpoint) return error(route, 404, "NOT_FOUND");
    const headers = request.headers();
    calls.push({
      endpoint,
      url,
      body: request.postData() ? request.postDataJSON() : undefined,
      csrf: headers["x-csrf-token"] ?? null,
      origin: headers["origin"] ?? null,
    });
    return (handlers[endpoint] ?? defaults[endpoint])(route, request);
  });
  const of = (endpoint: Endpoint) => calls.filter((call) => call.endpoint === endpoint);
  return { state, calls, of };
}

const field = (page: Page, name: string) => page.getByRole("textbox", { name, exact: true });
const password = (page: Page, name = "Password") => page.getByLabel(name, { exact: true });
const alert = (page: Page, text: string | RegExp) => page.getByRole("alert").filter({ hasText: text });
const status = (page: Page, text: string | RegExp) => page.getByRole("status").filter({ hasText: text });
const heading = (page: Page, name: string) => page.getByRole("heading", { level: 1, name });
const storageIsEmpty = (page: Page) => page.evaluate(() => localStorage.length === 0 && sessionStorage.length === 0);

async function signIn(page: Page, email = "ana@example.test", secret = PASSWORD) {
  await field(page, "Email").fill(email);
  await password(page).fill(secret);
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
}

test.beforeEach(async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
});

test("a new email user goes from sign-up through the real link to onboarding and Discover", async ({ page }) => {
  const api = await mockApi(page);
  await page.goto("/auth/signup");
  await field(page, "Email").fill("ana@example.test");
  await password(page).fill(PASSWORD);
  await page.getByRole("button", { name: "Create account" }).click();

  await expect(heading(page, "Check your inbox")).toBeVisible();
  await expect(page).toHaveURL("/auth/verify");
  await expect(page.getByText("We sent a verification link to ana@example.test.")).toBeVisible();
  // No control may stand in for the link.
  await expect(page.getByRole("button", { name: /verified/i })).toHaveCount(0);
  expect(api.of("register")).toHaveLength(1);
  expect(api.of("register")[0].body).toEqual({ email: "ana@example.test", password: PASSWORD });
  expect(api.of("register")[0].origin).toBe(ORIGIN);
  expect(api.of("session")[0].url.pathname).toBe("/api/v1/session");

  await page.getByRole("button", { name: "Resend link" }).click();
  await expect(status(page, "If that address can receive one, a new link is on its way.")).toBeVisible();
  const cooling = page.getByRole("button", { name: /^Send again in \d+s$/ });
  await expect(cooling).toHaveAttribute("aria-disabled", "true");
  await cooling.click({ force: true });
  expect(api.of("resend")).toHaveLength(1);
  expect(api.of("resend")[0].body).toEqual({ email: "ana@example.test" });

  // The emailed link: token in the fragment, exchanged once, then onboarding.
  await page.goto("/auth/verify#token=verify-token-1");
  await expect(heading(page, "Set up your profile")).toBeVisible();
  await expect(page).toHaveURL("/onboarding/profile");
  expect(api.of("verify").map((call) => call.body)).toEqual([{ token: "verify-token-1" }]);

  await expect(field(page, "Display name")).toHaveValue("Ana");
  await field(page, "Handle").fill("ana_r");
  await expect(page.getByRole("img", { name: "Avatar preview: A" })).toBeVisible();
  await page.getByRole("button", { name: "Save profile" }).click();
  await expect(heading(page, "Discover")).toBeVisible();
  expect(api.of("profile")[0].body).toEqual({ displayName: "Ana", handle: "ana_r" });
  expect(api.of("profile")[0].csrf).toBe(SIGNED_CSRF);
  expect(await storageIsEmpty(page)).toBe(true);
});

test("an expired or replayed verification link offers a new link and leaves the address bar", async ({ page }) => {
  const api = await mockApi(page, { verify: (route) => error(route, 410, "TOKEN_EXPIRED") });
  await page.goto("/auth/verify#token=spent-token");
  await expect(heading(page, "This link has expired")).toBeVisible();
  await expect(page).toHaveURL(/\/auth\/verify$/);
  await expect(page.getByText("SERVER-MESSAGE")).toHaveCount(0);
  await page.getByRole("button", { name: "Send a new link" }).click();
  await expect(field(page, "Email")).toHaveAttribute("aria-invalid", "true");
  await field(page, "Email").fill("ana@example.test");
  await page.getByRole("button", { name: "Send a new link" }).click();
  await expect(status(page, "a new link is on its way")).toBeVisible();
  expect(api.of("resend").map((call) => call.body)).toEqual([{ email: "ana@example.test" }]);
  await expect(page.getByRole("link", { name: "Back to sign in" })).toHaveAttribute("href", "/auth/signin");
});

test("a verification link that fails in transit can be retried without the address bar", async ({ page }) => {
  let fail = true;
  const api = await mockApi(page, {
    verify: (route) => (fail ? route.abort("connectionreset") : json(route, { user: user(), csrfToken: SIGNED_CSRF })),
  });
  await page.goto("/auth/verify#token=verify-token-2");
  await expect(alert(page, "We couldn’t check your link")).toContainText("Check your connection");
  await expect(page).toHaveURL(/\/auth\/verify$/);
  fail = false;
  await page.getByRole("button", { name: "Retry" }).click();
  await expect(page).toHaveURL("/onboarding/profile");
  expect(api.of("verify").map((call) => call.body)).toEqual([{ token: "verify-token-2" }, { token: "verify-token-2" }]);
});

test("sign-in: a wrong password is generic, and an unverified account gets the verify screen without a session", async ({
  page,
}) => {
  const api = await mockApi(page, {
    login: (route, request) => {
      const { email, password: secret } = request.postDataJSON() as { email: string; password: string };
      if (secret !== PASSWORD) return error(route, 401, "INVALID_CREDENTIALS");
      if (email === "pending@example.test") return error(route, 403, "EMAIL_UNVERIFIED");
      return json(route, { user: user({ handle: "ana_r" }), csrfToken: SIGNED_CSRF });
    },
    resend: (route) => error(route, 429, "RATE_LIMITED", undefined, { "Retry-After": "30" }),
  });
  await page.goto("/auth/signin");
  await signIn(page, "ana@example.test", "not the password");
  const mismatch = alert(page, "That email and password don’t match.");
  await expect(mismatch).toBeVisible();
  await expect(page.getByText("SERVER-MESSAGE")).toHaveCount(0);
  await expect(page.getByRole("link", { name: "Reset password" })).toHaveAttribute("href", "/auth/reset");
  await expect(field(page, "Email")).toHaveValue("ana@example.test");

  await field(page, "Email").fill("pending@example.test");
  await password(page).fill(PASSWORD);
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(heading(page, "Verify your email to sign in")).toBeVisible();
  await expect(page.getByText("pending@example.test")).toBeVisible();
  await page.getByRole("button", { name: "Resend link" }).click();
  await expect(alert(page, "Too many attempts. Try again in 30 seconds.")).toBeVisible();
  await expect(page.getByRole("button", { name: /^Send again in (30|29)s$/ })).toHaveAttribute("aria-disabled", "true");
  expect(api.of("login")).toHaveLength(2);
});

test("signing in enters the requested page with a full load; other destinations fall back to /home", async ({ page }) => {
  await mockApi(page, { library: (route) => json(route, { items: [{ title: "Ana’s show" }], nextCursor: null }) });
  await page.goto("/auth/signin?returnTo=%2Flibrary");
  await page.evaluate(() => ((window as { previousPage?: boolean }).previousPage = true));
  await signIn(page);
  await expect(page.getByRole("list", { name: "Saved shows" })).toContainText("Ana’s show");
  await expect(page).toHaveURL("/library");
  expect(await page.evaluate(() => (window as { previousPage?: boolean }).previousPage)).toBeUndefined();

  for (const hostile of ["//evil.example/home", "/auth/verify", "https://evil.example/home", "/home%2f..%2fauth"]) {
    await page.goto(`/auth/signin?returnTo=${encodeURIComponent(hostile)}`);
    await signIn(page);
    await expect(page).toHaveURL("/home");
  }
});

test("returnTo accepts only application paths", () => {
  expect(safeReturnTo("/library")).toBe("/library");
  expect(safeReturnTo("/shows/10000000-0000-4000-8000-000000000001?season=2#e3")).toBe(
    "/shows/10000000-0000-4000-8000-000000000001?season=2",
  );
  for (const hostile of [
    undefined,
    ["/home"],
    "",
    "home",
    "//evil.example",
    "/\\evil.example",
    "/homepage",
    "/auth/signin",
    "/home%0d%0a",
    "/home x",
    "/home\t",
    `/home?${"a".repeat(600)}`,
  ])
    expect(safeReturnTo(hostile), String(hostile)).toBeUndefined();
});

test("field and form errors: focus moves to the first invalid field, and server text never shows", async ({ page }) => {
  let reply: Handler = (route) => error(route, 422, "VALIDATION_ERROR", { password: "SERVER-FIELD" });
  const api = await mockApi(page, { register: (route, request) => reply(route, request) });
  await page.goto("/auth/signup");
  await page.getByRole("button", { name: "Create account" }).click();
  await expect(field(page, "Email")).toBeFocused();
  await expect(page.getByText("Enter a valid email address.")).toBeVisible();
  await expect(page.getByText("Enter a password.")).toBeVisible();
  await field(page, "Email").fill("ana@example.test");
  await password(page).fill("short");
  await page.getByRole("button", { name: "Create account" }).click();
  await expect(password(page)).toBeFocused();
  await expect(password(page)).toHaveAccessibleDescription("12–128 characters. Spaces count. Use at least 12 characters.");
  expect(api.of("register")).toHaveLength(0);

  // A 12-character password made of spaces is valid: passwords are never trimmed.
  await password(page).fill(" ".repeat(12));
  await page.getByRole("button", { name: "Create account" }).click();
  await expect(password(page)).toBeFocused();
  await expect(page.getByText("SERVER-FIELD")).toHaveCount(0);
  expect(api.of("register")[0].body).toEqual({ email: "ana@example.test", password: " ".repeat(12) });

  reply = (route) => error(route, 503, "SERVICE_UNAVAILABLE");
  await page.getByRole("button", { name: "Create account" }).click();
  await expect(alert(page, "SceneCask couldn’t be reached.")).toBeVisible();
  reply = (route) => error(route, 429, "RATE_LIMITED", undefined, { "Retry-After": "12" });
  await page.getByRole("button", { name: "Create account" }).click();
  await expect(alert(page, "Too many attempts. Try again in 12 seconds.")).toBeVisible();
  await expect(page.getByText("SERVER-MESSAGE")).toHaveCount(0);
});

test("repeated activation while busy sends one request", async ({ page }) => {
  const api = await mockApi(page, {
    register: async (route) => {
      await later(600);
      return json(route, { message: "Check your email." }, 202);
    },
  });
  await page.goto("/auth/signup");
  await field(page, "Email").fill("ana@example.test");
  await password(page).fill(PASSWORD);
  const submit = page.getByRole("button", { name: "Create account" });
  await submit.click();
  await expect(submit).toHaveAttribute("aria-busy", "true");
  await submit.click({ force: true });
  await password(page).press("Enter");
  await expect(heading(page, "Check your inbox")).toBeVisible();
  expect(api.of("register")).toHaveLength(1);
});

test("Google: one pending start; a canceled flow returns a recoverable error with Retry and email", async ({ page }) => {
  let respond: Handler = (route) => route.fulfill({ status: 204 });
  const api = await mockApi(page, { google: (route, request) => respond(route, request) });
  await page.goto("/auth/signin?returnTo=%2Flibrary");
  const google = page.getByRole("button", { name: "Continue with Google" });
  // Playwright cannot read a page while its top-level navigation is held, so the start answers
  // 204 (the browser stays put) to observe the pending state. Chromium itself collapses repeats
  // in one task, so this proves the busy guard against later activations, not the ref guard.
  await google.evaluate((button: HTMLElement) => {
    button.click();
    button.click();
  });
  await expect(google).toHaveAttribute("aria-busy", "true");
  await google.click({ force: true });
  await google.press("Enter");
  await expect.poll(() => api.of("google").length).toBe(1);
  expect(Object.fromEntries(api.of("google")[0].url.searchParams)).toEqual({ intent: "signin", returnTo: "/library" });

  respond = (route) => route.fulfill({ status: 303, headers: { Location: "/auth/signin?error=canceled" } });
  await page.reload();
  await page.getByRole("button", { name: "Continue with Google" }).click();
  const canceled = alert(page, "Google sign-in was canceled");
  await expect(canceled).toContainText("Nothing changed.");
  expect(api.of("google")).toHaveLength(2);
  // The code was read once and dropped from the address.
  await expect(page).toHaveURL("/auth/signin");

  await page.getByRole("button", { name: "Use email instead" }).click();
  await expect(field(page, "Email")).toBeFocused();
  await page.getByRole("button", { name: "Try Google again" }).click();
  await expect.poll(() => api.of("google").length).toBe(3);
  await expect(alert(page, "Google sign-in was canceled")).toBeVisible();

  for (const [code, title] of [
    ["unavailable", "Google isn’t responding"],
    ["expired", "That Google sign-in timed out"],
    ["email_unverified", "Google hasn’t verified that email"],
    ["<script>", "Google sign-in didn’t finish"],
  ]) {
    await page.goto(`/auth/signin?error=${encodeURIComponent(code)}`);
    await expect(alert(page, title)).toBeVisible();
    await expect(page.getByText(code, { exact: false })).toHaveCount(0);
  }
});

test("an existing account for the Google email explains linking and never merges", async ({ page }) => {
  await mockApi(page);
  await page.goto("/auth/signin?error=link_required");
  await expect(heading(page, "Sign in to link Google")).toBeVisible();
  await expect(page.getByText("Accounts are never merged automatically.", { exact: false })).toBeVisible();
  // Google would only fail the same way again; the existing method is the way in.
  await expect(page.getByRole("button", { name: /Google/ })).toHaveCount(0);
  await signIn(page);
  await expect(page).toHaveURL("/home");
});

test("password reset: the request is enumeration-safe and the link sets a new password", async ({ page }) => {
  const api = await mockApi(page);
  await page.goto("/auth/reset");
  await field(page, "Email").fill("someone@example.test");
  await page.getByRole("button", { name: "Send reset link" }).click();
  await expect(heading(page, "Check your inbox")).toBeVisible();
  await expect(page.getByText("If there’s an account for someone@example.test, a reset link is on its way.")).toBeVisible();
  expect(api.of("resetRequest").map((call) => call.body)).toEqual([{ email: "someone@example.test" }]);
  await page.getByRole("button", { name: "Use a different email" }).click();
  await expect(heading(page, "Reset your password")).toBeFocused();

  await page.goto("/auth/reset/confirm#token=reset-token-1");
  await expect(heading(page, "Choose a new password")).toBeVisible();
  await expect(page).toHaveURL(/\/auth\/reset\/confirm$/);
  await password(page, "New password").fill(PASSWORD);
  await password(page, "Confirm new password").fill(`${PASSWORD}!`);
  await page.getByRole("button", { name: "Change password" }).click();
  await expect(page.getByText("The passwords don’t match.")).toBeVisible();
  await expect(password(page, "Confirm new password")).toBeFocused();
  expect(api.of("reset")).toHaveLength(0);
  await password(page, "Confirm new password").fill(PASSWORD);
  await page.getByRole("button", { name: "Change password" }).click();
  await expect(heading(page, "Password changed")).toBeFocused();
  await expect(page.getByText("You’ve been signed out everywhere.")).toBeVisible();
  expect(api.of("reset").map((call) => call.body)).toEqual([{ token: "reset-token-1", newPassword: PASSWORD }]);
  await page.getByRole("link", { name: "Sign in" }).click();
  await expect(heading(page, "Sign in")).toBeVisible();
});

test("an expired reset link offers a new link and another way to sign in", async ({ page }) => {
  const api = await mockApi(page, { reset: (route) => error(route, 410, "TOKEN_EXPIRED") });
  await page.goto("/auth/reset/confirm#token=reset-token-old");
  await password(page, "New password").fill(PASSWORD);
  await password(page, "Confirm new password").fill(PASSWORD);
  await page.getByRole("button", { name: "Change password" }).click();
  await expect(heading(page, "This reset link has expired")).toBeFocused();
  await expect(page.getByRole("link", { name: "Send a new link" })).toHaveAttribute("href", "/auth/reset");
  await expect(page.getByRole("link", { name: "Sign in" })).toHaveAttribute("href", "/auth/signin");
  await page.getByRole("button", { name: "Continue with Google" }).click();
  await expect.poll(() => api.of("google").length).toBe(1);

  await page.goto("/auth/reset/confirm");
  await expect(heading(page, "This reset link is incomplete")).toBeVisible();
});

test("onboarding: taken and malformed handles are inline errors, Skip continues, and routing follows the session", async ({
  page,
}) => {
  let taken = true;
  const api = await mockApi(page, {
    profile: (route, request) =>
      taken
        ? error(route, 409, "HANDLE_TAKEN", { handle: "SERVER-FIELD" })
        : json(route, { ...user(), ...(request.postDataJSON() as object) }),
  });
  api.state.user = user({ displayName: "Ana Rivera" });
  await page.goto("/onboarding/profile");
  await expect(page.getByRole("img", { name: "Avatar preview: AR" })).toBeVisible();
  await field(page, "Handle").fill("Ana R");
  await page.getByRole("button", { name: "Save profile" }).click();
  await expect(field(page, "Handle")).toBeFocused();
  await expect(field(page, "Handle")).toHaveAccessibleDescription(/Use 3–30 lowercase letters/);
  expect(api.of("profile")).toHaveLength(0);

  await field(page, "Handle").fill("ana");
  await page.getByRole("button", { name: "Save profile" }).click();
  await expect(page.getByText("That handle is taken. Try another.")).toBeVisible();
  await expect(field(page, "Handle")).toBeFocused();
  await expect(page.getByText("SERVER-FIELD")).toHaveCount(0);
  taken = false;
  await field(page, "Handle").fill("ana_rivera");
  await page.getByRole("button", { name: "Save profile" }).click();
  await expect(heading(page, "Discover")).toBeVisible();
  expect(api.of("profile").map((call) => call.body)).toEqual([
    { displayName: "Ana Rivera", handle: "ana" },
    { displayName: "Ana Rivera", handle: "ana_rivera" },
  ]);

  await page.goto("/onboarding/profile");
  await page.getByRole("link", { name: "Skip for now" }).click();
  await expect(heading(page, "Discover")).toBeVisible();

  api.state.user = user({ handle: "ana_r" });
  await page.goto("/onboarding/profile");
  await expect(page).toHaveURL("/discover");

  api.state.user = null;
  await page.goto("/onboarding/profile");
  await expect(page).toHaveURL("/auth/signin?returnTo=%2Fonboarding%2Fprofile");
});

test("onboarding: a session that fails to load can be retried", async ({ page }) => {
  let fail = true;
  const api = await mockApi(page, {
    session: (route) =>
      fail ? error(route, 503, "SERVICE_UNAVAILABLE") : json(route, { user: user(), csrfToken: SIGNED_CSRF }),
  });
  await page.goto("/onboarding/profile");
  await expect(alert(page, "Your account didn’t load")).toBeVisible();
  fail = false;
  await page.getByRole("button", { name: "Retry" }).click();
  await expect(field(page, "Display name")).toHaveValue("Ana");
  expect(api.of("session").length).toBeGreaterThanOrEqual(2);
});

test("a 401 on a private screen returns to sign-in without keeping the previous viewer's data", async ({ page }) => {
  let viewer: "A" | "B" | null = "A";
  await mockApi(page, {
    library: (route) =>
      viewer ? json(route, { items: [{ title: `${viewer}’s private show` }], nextCursor: null }) : error(route, 401, "AUTH_REQUIRED"),
    login: (route) => {
      viewer = "B";
      return json(route, { user: user({ id: "10000000-0000-4000-8000-00000000000b", handle: "bea" }), csrfToken: SIGNED_CSRF });
    },
  });
  await page.goto("/home");
  await page.goto("/library");
  await expect(page.getByText("A’s private show")).toBeVisible();

  viewer = null;
  await page.getByRole("button", { name: "Refresh" }).click();
  await expect(status(page, "Your session ended. Sign in again to continue.")).toBeVisible();
  await expect(page).toHaveURL("/auth/signin?reason=expired&returnTo=%2Flibrary");
  await expect(page.getByText("A’s private show")).toHaveCount(0);
  expect(await page.evaluate(() => (window as { viewerCache?: unknown }).viewerCache)).toBeUndefined();

  // The private entry was replaced, so Back cannot bring A's screen back.
  await page.goBack();
  await expect(page).toHaveURL("/home");
  await page.goForward();
  await expect(page).toHaveURL(/\/auth\/signin/);

  await signIn(page, "bea@example.test");
  await expect(page).toHaveURL("/library");
  await expect(page.getByText("B’s private show")).toBeVisible();
  await expect(page.getByText("A’s private show")).toHaveCount(0);
  expect(await storageIsEmpty(page)).toBe(true);
});

test("reauth dialog: a wrong password keeps it open; the right one resumes; Google uses the reauth intent", async ({
  page,
}) => {
  const api = await mockApi(page, {
    reauth: (route, request) =>
      (request.postDataJSON() as { password: string }).password === PASSWORD
        ? route.fulfill({ status: 204 })
        : error(route, 401, "INVALID_CREDENTIALS"),
  });
  api.state.user = user({ handle: "ana_r" });
  await page.goto("/reauth");
  const trigger = page.getByRole("button", { name: "Unlink Google" });
  await trigger.click();
  const dialog = page.getByRole("dialog", { name: "Confirm it’s you" });
  await expect(password(page)).toBeFocused();
  await password(page).fill("wrong password");
  await password(page).press("Enter");
  await expect(dialog.getByText("That password isn’t right.")).toBeVisible();
  await password(page).fill(PASSWORD);
  await dialog.getByRole("button", { name: "Confirm" }).click();
  await expect(status(page, "Confirmed. Unlinking Google…")).toBeVisible();
  await expect(dialog).toHaveCount(0);
  await expect(trigger).toBeFocused();
  expect(api.of("reauth").map((call) => call.csrf)).toEqual([SIGNED_CSRF, SIGNED_CSRF]);

  await trigger.click();
  await expect(password(page)).toHaveValue("");
  await dialog.getByRole("button", { name: "Continue with Google" }).click();
  await expect.poll(() => api.of("google").length).toBe(1);
  expect(Object.fromEntries(api.of("google")[0].url.searchParams)).toEqual({ intent: "reauth", returnTo: "/settings" });
});

test("keyboard order on sign-in follows the visual order", async ({ page }) => {
  await mockApi(page);
  await page.goto("/auth/signin");
  const order = [
    page.getByRole("link", { name: "Skip to content" }),
    page.getByRole("link", { name: "SceneCask home" }),
    page.getByRole("button", { name: "Continue with Google" }),
    field(page, "Email"),
    password(page),
    page.getByRole("button", { name: "Sign in", exact: true }),
    page.getByRole("link", { name: "Forgot password?" }),
    page.getByRole("link", { name: "Create account" }),
  ];
  for (const target of order) {
    await page.keyboard.press("Tab");
    await expect(target).toBeFocused();
  }
  await field(page, "Email").focus();
  await page.keyboard.press("Enter");
  await expect(field(page, "Email")).toBeFocused();
  await expect(page.getByText("Enter a valid email address.")).toBeVisible();
});

/** WCAG 2.0 A/AA and 2.1 AA violations on the current screen, as "rule: targets" lines. */
async function axeViolations(page: Page) {
  await page.addScriptTag({ content: axe.source });
  const results = await page.evaluate(() =>
    (window as unknown as { axe: typeof axe }).axe.run({ runOnly: ["wcag2a", "wcag2aa", "wcag21aa"] }),
  );
  return results.violations.map((rule) => `${rule.id}: ${rule.nodes.map((node) => node.target.join(" ")).join(", ")}`);
}

/** Each screen state, reached through the real routes. */
const SCREENS: { name: string; reach: (page: Page) => Promise<unknown> }[] = [
  { name: "signin", reach: (page) => page.goto("/auth/signin") },
  {
    name: "session-expired",
    reach: (page) => page.goto("/auth/signin?reason=expired&returnTo=%2Flibrary"),
  },
  {
    name: "signup-errors",
    reach: async (page) => {
      await page.goto("/auth/signup");
      await field(page, "Email").fill("ana@");
      await password(page).fill("short");
      await page.getByRole("button", { name: "Create account" }).click();
      await expect(page.getByText("Enter a valid email address.")).toBeVisible();
    },
  },
  { name: "google-canceled", reach: (page) => page.goto("/auth/signin?error=canceled") },
  { name: "link-required", reach: (page) => page.goto("/auth/signin?error=link_required") },
  {
    name: "verify-inbox",
    reach: async (page) => {
      await page.goto("/auth/signup");
      await field(page, "Email").fill("ana.with.a.long.address@example.test");
      await password(page).fill(PASSWORD);
      await page.getByRole("button", { name: "Create account" }).click();
      await expect(heading(page, "Check your inbox")).toBeVisible();
    },
  },
  {
    name: "verify-expired",
    reach: async (page) => {
      await page.unroute("**/api/v1/**");
      await mockApi(page, { verify: (route) => error(route, 410, "TOKEN_EXPIRED") });
      await page.goto("/auth/verify#token=spent");
      await expect(heading(page, "This link has expired")).toBeVisible();
    },
  },
  {
    name: "reset-expired",
    reach: async (page) => {
      await page.goto("/auth/reset/confirm");
      await expect(heading(page, "This reset link is incomplete")).toBeVisible();
    },
  },
  {
    name: "onboarding",
    reach: async (page) => {
      await page.unroute("**/api/v1/**");
      const api = await mockApi(page, { profile: (route) => error(route, 409, "HANDLE_TAKEN") });
      api.state.user = user({ displayName: "Ana Rivera" });
      await page.goto("/onboarding/profile");
      await field(page, "Handle").fill("ana");
      await page.getByRole("button", { name: "Save profile" }).click();
      await expect(page.getByText("That handle is taken. Try another.")).toBeVisible();
    },
  },
];

for (const width of [320, 390, 859, 860, 1440])
  test(`responsive account screens, targets, axe and screenshots at ${width}`, async ({ page }) => {
    test.setTimeout(90000);
    await page.setViewportSize({ width, height: 900 });
    const shots = [390, 1440].includes(width);
    for (const screen of SCREENS) {
      await page.unroute("**/api/v1/**");
      await mockApi(page);
      await screen.reach(page);
      await page.evaluate(() => document.fonts.ready);
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), screen.name).toBe(true);
      const column = (await page.locator(".sc-auth-column").boundingBox())!;
      // D03: a 480px column on desktop; full width inside 20px gutters on phones.
      expect(column.width, screen.name).toBeLessThanOrEqual(480);
      expect(column.x, screen.name).toBeGreaterThanOrEqual(20);
      for (const control of await page.locator(".sc-auth-column :is(button, a, input)").all()) {
        const box = (await control.boundingBox())!;
        expect(box.height, `${screen.name} control height`).toBeGreaterThanOrEqual(44);
        expect(box.width, `${screen.name} control width`).toBeGreaterThanOrEqual(44);
      }
      if (shots) {
        expect(await axeViolations(page), screen.name).toEqual([]);
        await page.screenshot({ path: `test-results/r10/${screen.name}-${width}.png`, fullPage: true });
      }
    }
  });
