import { createSceneCaskClient, type components } from "@scenecask/api-client";

type ErrorBody = components["schemas"]["Error"]["error"];

/** Failure kinds every account adapter shares; each adds its own error codes. */
export type BaseFailure =
  | { kind: "invalid"; fields: string[] }
  | { kind: "rate_limited"; retryAfter?: number }
  | { kind: "auth" | "csrf" | "rejected" | "unavailable" };
export type Settled<T, Failure> = { ok: true; data: T } | { ok: false; failure: Failure };
export type Run<T> = () => Promise<{ data?: T; error?: unknown; response: Response }>;

/** Adapter-specific kinds: by error code first, then by status when no code was recognised. */
export interface FailureKinds<Kind extends string> {
  codes: Record<string, Kind>;
  statuses: Record<number, Kind>;
}

/**
 * The safe category for a failed response. Only the names of rejected fields are kept, never
 * server messages or values, so the UI chooses all copy.
 */
function failureFrom<Kind extends string>(
  { codes, statuses }: FailureKinds<Kind>,
  response: Response,
  body: unknown,
): BaseFailure | { kind: Kind } {
  const error = (body as { error?: Partial<ErrorBody> } | undefined)?.error;
  const code = error?.code;
  if (code === "VALIDATION_ERROR") return { kind: "invalid", fields: Object.keys(error?.fields ?? {}) };
  if (code === "AUTH_REQUIRED") return { kind: "auth" };
  if (code === "CSRF_FAILED") return { kind: "csrf" };
  if (code && Object.hasOwn(codes, code)) return { kind: codes[code] };
  // No recognised code (for example a proxy's error page): the status still decides.
  const { status } = response;
  if (status === 429) {
    const seconds = Number(response.headers.get("Retry-After"));
    return { kind: "rate_limited", retryAfter: Number.isFinite(seconds) && seconds > 0 ? Math.ceil(seconds) : undefined };
  }
  if (Object.hasOwn(statuses, status)) return { kind: statuses[status] };
  if (status === 401) return { kind: "auth" };
  if (status === 400 || status === 422) return { kind: "invalid", fields: [] };
  return status >= 500 ? { kind: "unavailable" } : { kind: "rejected" };
}

/**
 * Generated-client transport for account adapters. It keeps only the session-paired CSRF token,
 * in memory: no credential, token or session value is ever written to storage.
 */
export function createAccountTransport<Kind extends string>(kinds: FailureKinds<Kind>) {
  type Failure = BaseFailure | { kind: Kind };
  let csrfToken: string | undefined;
  const client = createSceneCaskClient({ getCsrfToken: () => csrfToken });

  async function settle<T>(run: Run<T>): Promise<Settled<T, Failure>> {
    try {
      const { data, error, response } = await run();
      // 202/204 carry no body the screens need; the status alone is the success.
      if (response.ok) return { ok: true, data: data as T };
      return { ok: false, failure: failureFrom(kinds, response, error) };
    } catch {
      return { ok: false, failure: { kind: "unavailable" } };
    }
  }

  async function session() {
    const result = await settle(() => client.GET("/session"));
    csrfToken = result.ok ? result.data.csrfToken : undefined;
    return result;
  }

  /**
   * A write sends the session's token. Sign-in, reset and reauth rotate the session, so a CSRF
   * rejection (which runs nothing) is retried once with a fresh token.
   */
  async function write<T>(run: Run<T>): Promise<Settled<T, Failure>> {
    for (let attempt = 0; ; attempt += 1) {
      if (!csrfToken) {
        const current = await session();
        if (!current.ok) return current;
      }
      const result = await settle(run);
      if (!result.ok && result.failure.kind === "csrf" && attempt === 0) {
        csrfToken = undefined;
        continue;
      }
      if (result.ok) {
        // Sign-in returns the rotated token; other writes leave the next one to re-read it.
        const next = (result.data as { csrfToken?: unknown } | undefined)?.csrfToken;
        csrfToken = typeof next === "string" ? next : undefined;
      }
      return result;
    }
  }

  return { client, settle, session, write };
}
