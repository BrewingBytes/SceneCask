import { randomUUID } from "node:crypto";
import axe from "axe-core";
import { expect, type APIRequestContext, type BrowserContext, type Page } from "@playwright/test";
import { OIDC_ISSUER, PROTECTED_TITLES, TMDB_BASE } from "./data.mjs";

export const ORIGIN = "http://127.0.0.1:3000";
const MAILPIT = process.env.MAILPIT_API ?? "http://127.0.0.1:8029";
export const PASSWORD = "correct horse battery staple";

export const freshEmail = (label: string) => `${label}-${randomUUID()}@alpha.example.test`;

/** The newest emailed link to `path` for `email`, read from the real SMTP capture (Mailpit). */
export async function emailedLink(request: APIRequestContext, email: string, path: string) {
  let link: string | undefined;
  await expect
    .poll(
      async () => {
        const search = await request.get(`${MAILPIT}/api/v1/search?query=${encodeURIComponent(`to:"${email}"`)}`);
        const { messages = [] } = (await search.json()) as { messages?: { ID: string }[] };
        for (const { ID } of messages) {
          const { Text = "" } = (await (await request.get(`${MAILPIT}/api/v1/message/${ID}`)).json()) as { Text?: string };
          const match = Text.match(new RegExp(`${ORIGIN}${path}#token=[A-Za-z0-9_-]{43}`));
          if (match) link = match[0];
          if (link) return true;
        }
        return false;
      },
      { timeout: 30_000, message: `email to ${email}` },
    )
    .toBe(true);
  return link!;
}

/** Signs up through the API (C04 register), then verifies through the emailed link in the browser. */
export async function verifiedAccount(page: Page, label: string) {
  const email = freshEmail(label);
  const response = await page.request.post("/api/v1/auth/register", {
    headers: { Origin: ORIGIN },
    data: { email, password: PASSWORD },
  });
  expect(response.status()).toBe(202);
  await page.goto(await emailedLink(page.request, email, "/auth/verify"));
  await expect(page).toHaveURL(/\/onboarding\/profile$/);
  await page.getByRole("link", { name: "Skip for now" }).click();
  await expect(page).toHaveURL(/\/discover$/);
  return email;
}

export async function signIn(page: Page, email: string, returnTo?: string) {
  await page.goto(returnTo ? `/auth/signin?returnTo=${encodeURIComponent(returnTo)}` : "/auth/signin");
  await page.getByLabel("Email").fill(email);
  await page.getByLabel("Password", { exact: true }).fill(PASSWORD);
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
}

export async function signOut(page: Page) {
  await page.getByRole("button", { name: /^Account/ }).click();
  await page.getByRole("menuitem", { name: "Sign out" }).click();
  await expect(page).toHaveURL(/\/auth\/signin$/);
}

/**
 * Records every same-origin response body (JSON, HTML, RSC/prefetch payloads) so tests can prove
 * protected strings never reached this viewer, not merely that they were not rendered.
 */
export function recordNetwork(page: Page) {
  const bodies: Promise<{ url: string; body: string }>[] = [];
  page.on("response", (response) => {
    if (!response.url().startsWith(ORIGIN)) return;
    bodies.push(
      response
        .text()
        .then((body) => ({ url: response.url(), body }))
        .catch(() => ({ url: response.url(), body: "" })),
    );
  });
  return {
    /** Responses since the last reset that contain any of `titles`. */
    async leaks(titles: readonly string[]) {
      const settled = await Promise.all(bodies.splice(0));
      return settled.flatMap(({ url, body }) => titles.filter((title) => body.includes(title)).map((title) => `${title} @ ${url}`));
    },
    async count() {
      return (await Promise.all(bodies)).length;
    },
  };
}

export const titlesExcept = (...allowed: string[]) => PROTECTED_TITLES.filter((title) => !allowed.includes(title));

/** A signed-in API client on `context`'s cookies: Origin, CSRF and Idempotency-Key as the web app sends them. */
export async function apiAs(context: BrowserContext) {
  const session = (await (await context.request.get("/api/v1/session")).json()) as { csrfToken: string };
  const headers = () => ({ Origin: ORIGIN, "X-CSRF-Token": session.csrfToken, "Idempotency-Key": randomUUID() });
  return {
    get: async (path: string) => context.request.get(`/api/v1${path}`),
    put: async (path: string, data: unknown) => context.request.put(`/api/v1${path}`, { headers: headers(), data }),
  };
}

export async function episodeIds(context: BrowserContext, showId: string) {
  const response = await context.request.get(`/api/v1/shows/${showId}/episodes?season=1`);
  expect(response.status()).toBe(200);
  const { items } = (await response.json()) as { items: { id: string; number: number }[] };
  return Object.fromEntries(items.map((item) => [item.number, item.id])) as Record<number, string>;
}

/** Fixture-only switches at the provider boundary. */
export async function setTmdbDown(request: APIRequestContext, down: boolean) {
  const response = await request.post(new URL("/__control", TMDB_BASE).toString(), { data: { down } });
  expect(response.ok()).toBe(true);
}

export async function setGoogleIdentity(request: APIRequestContext, identity: { sub: string; email: string; name: string } | null) {
  const response = await request.post(`${OIDC_ISSUER}/__identity`, { data: identity });
  expect(response.ok()).toBe(true);
}

export const axeViolations = (page: Page) =>
  page.evaluate(async (source) => {
    if (!("axe" in window)) new Function(source)();
    const result = await (window as unknown as { axe: typeof axe }).axe.run(document, {
      runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21aa"] },
    });
    return result.violations.map((violation) => `${violation.id}: ${violation.nodes.map((node) => node.target).join(", ")}`);
  }, axe.source);

export const horizontalOverflow = (page: Page) =>
  page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
