/** Where a session lands when nothing more specific was asked for (C04's Google default too). */
export const HOME = "/home";
/** First stop for a newly verified account: handles are chosen there (D03). */
export const ONBOARDING = "/onboarding/profile";
/** Where onboarding (saved or skipped) hands over: search works without friends (C09 #11). */
export const AFTER_ONBOARDING = "/discover";

/**
 * Application routes a sign-in may return to, each matching itself and any path below it.
 * Mirrors the API's Google `returnTo` allowlist, so auth pages never become a destination.
 */
const RETURN_ROUTES = [
  "/home",
  "/discover",
  "/library",
  "/shows",
  "/episodes",
  "/settings",
  "/onboarding/profile",
  "/friends",
  "/notifications",
];
const MAX_RETURN_TO = 512;

/** A same-origin application path from untrusted input, or undefined. Fragments are dropped. */
export function safeReturnTo(value: string | string[] | null | undefined): string | undefined {
  if (typeof value !== "string" || value.length > MAX_RETURN_TO) return undefined;
  // Encoded, escaped, spaced or protocol-relative forms are refused rather than interpreted.
  if (!value.startsWith("/") || value.startsWith("//") || /[\\%\s\p{Cc}]/u.test(value)) return undefined;
  const url = new URL(value, "https://scenecask.invalid");
  if (url.origin !== "https://scenecask.invalid") return undefined;
  const allowed = RETURN_ROUTES.some((route) => url.pathname === route || url.pathname.startsWith(`${route}/`));
  return allowed ? url.pathname + url.search : undefined;
}

export type GoogleIntent = "signin" | "link" | "reauth";

/**
 * C04 Google start. A browser navigation, never fetch: the API answers 303 to Google, and Google
 * returns through the API callback to an application route.
 */
export function googleStartHref(intent: GoogleIntent, returnTo?: string) {
  const params = new URLSearchParams({ intent });
  if (returnTo) params.set("returnTo", returnTo);
  return `/api/v1/auth/google/start?${params}`;
}

export function signinHref({ returnTo, expired = false }: { returnTo?: string; expired?: boolean } = {}) {
  const params = new URLSearchParams();
  if (expired) params.set("reason", "expired");
  if (returnTo) params.set("returnTo", returnTo);
  const query = params.toString();
  return query ? `/auth/signin?${query}` : "/auth/signin";
}

/**
 * Enters a signed-in area with a full document load. Nothing held in memory by the previous
 * page (another viewer's data, the router cache, the CSRF token) carries across a session change.
 */
export function enterSession(path: string) {
  window.location.assign(path);
}

/**
 * Call on a 401 while showing private data. The current entry is replaced by a full load of
 * sign-in, so neither in-memory state nor a Back entry keeps the previous viewer's screen.
 * After signing in, the person returns to the same path.
 */
export function endExpiredSession(returnTo = window.location.pathname + window.location.search) {
  window.location.replace(signinHref({ returnTo: safeReturnTo(returnTo), expired: true }));
}
