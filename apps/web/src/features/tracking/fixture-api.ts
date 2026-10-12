import type { Request, Route } from "@playwright/test";

/**
 * Shared plumbing for the stateful C05 fakes in component suites. Test-only: imported by
 * Playwright specs, never by application code.
 */
export const CSRF = "csrf-fixture-token";
export const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

export interface Logged {
  method: string;
  path: string;
  query: Record<string, string>;
  body: unknown;
  key: string | null;
  csrf: string | null;
}

export interface FakeBackend {
  log: Logged[];
  idempotent: Map<string, { body: string; response: unknown; status: number }>;
  /** Body for a GET, or null for 404. */
  read(path: string, url: URL): unknown;
  /** Status and body for a write; a non-2xx body is the error code. */
  mutate(method: string, path: string, body: Record<string, unknown> | undefined): [number, unknown];
}

export const json = (route: Route, body: unknown, status = 200) =>
  route.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) });
export const error = (route: Route, status: number, code: string) =>
  json(route, { error: { code, message: "Safe server message.", requestId: "60000000-0000-4000-8000-000000000001" } }, status);
export const later = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/**
 * Answers /session with the CSRF token and logs every other call. The same Idempotency-Key
 * and body replays the stored response; a changed body is a conflict.
 */
export function serve(fake: FakeBackend, route: Route, request: Request) {
  const url = new URL(request.url());
  const path = url.pathname.replace("/api/v1", "");
  const method = request.method();
  const body = request.postData() ? request.postDataJSON() : undefined;
  const key = request.headers()["idempotency-key"] ?? null;
  if (path === "/session") return json(route, { user: null, csrfToken: CSRF });
  fake.log.push({ method, path, query: Object.fromEntries(url.searchParams), body, key, csrf: request.headers()["x-csrf-token"] ?? null });
  if (method === "GET") {
    const found = fake.read(path, url);
    return found === null ? error(route, 404, "NOT_FOUND") : json(route, found);
  }
  if (key) {
    const stored = fake.idempotent.get(key);
    if (stored) return stored.body === JSON.stringify(body) ? json(route, stored.response, stored.status) : error(route, 409, "REVISION_CONFLICT");
  }
  const [status, response] = fake.mutate(method, path, body);
  if (key && status < 300) fake.idempotent.set(key, { body: JSON.stringify(body), response, status });
  return status < 300 ? json(route, response, status) : error(route, status, response as string);
}
