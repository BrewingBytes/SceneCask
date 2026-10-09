import { createSceneCaskClient, type components } from "@scenecask/api-client";

export type SearchShow = components["schemas"]["SearchShow"];
export type SearchResults = components["schemas"]["SearchResults"];
export type SaveRequest = components["schemas"]["SaveRequest"];
type ErrorCode = components["schemas"]["Error"]["error"]["code"];

/**
 * Safe failure categories. Copy is chosen by the UI; server messages, provider payloads and
 * image paths are never surfaced. `network` and `unavailable` are unknown outcomes: a write
 * retried after them must reuse its Idempotency-Key and body.
 */
export type FailureKind =
  | "aborted"
  | "auth"
  | "unverified"
  | "csrf"
  | "not_found"
  | "conflict"
  | "expired"
  | "invalid"
  | "rate_limited"
  | "provider"
  | "unavailable"
  | "network";
export interface Failure {
  kind: FailureKind;
  /** Seconds from Retry-After on 429. */
  retryAfter?: number;
}
export type ApiResult<T> = { ok: true; data: T } | { ok: false; failure: Failure };

export const outcomeUnknown = (failure: Failure) =>
  failure.kind === "network" || failure.kind === "unavailable";

export type DiscoverApi = ReturnType<typeof createDiscoverApi>;

function classify(response: Response, error: unknown): Failure {
  const code = (error as { error?: { code?: ErrorCode } } | undefined)?.error?.code;
  switch (response.status) {
    case 401:
      return { kind: "auth" };
    case 403:
      return { kind: code === "EMAIL_UNVERIFIED" ? "unverified" : code === "CSRF_FAILED" ? "csrf" : "auth" };
    case 404:
      return { kind: "not_found" };
    case 409:
      return { kind: "conflict" };
    case 410:
      return { kind: "expired" };
    case 400:
    case 422:
      return { kind: "invalid" };
    case 429: {
      const seconds = Number(response.headers.get("Retry-After"));
      return { kind: "rate_limited", retryAfter: Number.isFinite(seconds) && seconds > 0 ? seconds : undefined };
    }
    case 502:
      return { kind: "provider" };
    default:
      return { kind: "unavailable" };
  }
}

type Call<T> = () => Promise<{ data?: T; error?: unknown; response: Response }>;

/** Browser adapter over the generated client. Create one per viewer; it caches only the CSRF token. */
export function createDiscoverApi() {
  let csrfToken: string | undefined;
  const client = createSceneCaskClient({ getCsrfToken: () => csrfToken });

  async function request<T>(run: Call<T>, signal?: AbortSignal, mutation = false): Promise<ApiResult<T>> {
    try {
      if (mutation && !csrfToken) {
        const session = await client.GET("/session");
        csrfToken = session.data?.csrfToken;
        if (!csrfToken) return { ok: false, failure: { kind: "network" } };
      }
      const { data, error, response } = await run();
      if (response.ok && data !== undefined) return { ok: true, data };
      const failure = classify(response, error);
      // A rotated or expired session token is refetched before the next mutation.
      if (failure.kind === "csrf" || failure.kind === "auth") csrfToken = undefined;
      return { ok: false, failure };
    } catch {
      return { ok: false, failure: { kind: signal?.aborted ? "aborted" : "network" } };
    }
  }

  return {
    search: (q: string, page: number, signal: AbortSignal) =>
      request(() => client.GET("/shows/search", { params: { query: { q, page } }, signal }), signal),
    importShow: (providerId: number) =>
      request(() => client.POST("/shows/import", { body: { providerId } }), undefined, true),
    getShow: (showId: string) => request(() => client.GET("/shows/{id}", { params: { path: { id: showId } } })),
    saveToLibrary: (showId: string, body: SaveRequest, idempotencyKey: string) =>
      request(
        () =>
          client.PUT("/library/{showId}", {
            params: { path: { showId }, header: { "Idempotency-Key": idempotencyKey } },
            body,
          }),
        undefined,
        true,
      ),
    undo: (actionId: string, idempotencyKey: string) =>
      request(
        () =>
          client.POST("/actions/{id}/undo", {
            params: { path: { id: actionId }, header: { "Idempotency-Key": idempotencyKey } },
            body: {},
          }),
        undefined,
        true,
      ),
  };
}

export function newIdempotencyKey() {
  return crypto.randomUUID();
}
