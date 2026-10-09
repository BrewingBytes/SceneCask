import { createSceneCaskClient, type components } from "@scenecask/api-client";

export type User = components["schemas"]["User"];
export type Session = components["schemas"]["Session"];
type ErrorBody = components["schemas"]["Error"]["error"];

/**
 * Safe failure categories for auth screens. The UI chooses all copy: server messages are never
 * shown, and only the names of rejected fields are kept, never their messages or values.
 */
export type AuthFailure =
  | { kind: "invalid"; fields: string[] }
  | { kind: "rate_limited"; retryAfter?: number }
  | {
      kind:
        | "credentials"
        | "auth"
        | "unverified"
        | "csrf"
        | "expired"
        | "handle_taken"
        | "rejected"
        | "unavailable";
    };
export type AuthResult<T> = { ok: true; data: T } | { ok: false; failure: AuthFailure };

export type AuthApi = ReturnType<typeof createAuthApi>;

function failureFrom(response: Response, body: unknown): AuthFailure {
  const error = (body as { error?: Partial<ErrorBody> } | undefined)?.error;
  switch (error?.code) {
    case "VALIDATION_ERROR":
      return { kind: "invalid", fields: Object.keys(error.fields ?? {}) };
    case "INVALID_CREDENTIALS":
      return { kind: "credentials" };
    case "AUTH_REQUIRED":
      return { kind: "auth" };
    case "EMAIL_UNVERIFIED":
      return { kind: "unverified" };
    case "CSRF_FAILED":
      return { kind: "csrf" };
    case "TOKEN_EXPIRED":
      return { kind: "expired" };
    case "HANDLE_TAKEN":
      return { kind: "handle_taken" };
  }
  // No recognised code (for example a proxy's error page): the status still decides.
  if (response.status === 429) {
    const seconds = Number(response.headers.get("Retry-After"));
    return { kind: "rate_limited", retryAfter: Number.isFinite(seconds) && seconds > 0 ? Math.ceil(seconds) : undefined };
  }
  if (response.status === 410) return { kind: "expired" };
  if (response.status === 401) return { kind: "auth" };
  if (response.status === 400 || response.status === 422) return { kind: "invalid", fields: [] };
  return response.status >= 500 ? { kind: "unavailable" } : { kind: "rejected" };
}

type Run<T> = () => Promise<{ data?: T; error?: unknown; response: Response }>;

async function settle<T>(run: Run<T>): Promise<AuthResult<T>> {
  try {
    const { data, error, response } = await run();
    // 202/204 carry no body the screens need; the status alone is the success.
    if (response.ok) return { ok: true, data: data as T };
    return { ok: false, failure: failureFrom(response, error) };
  } catch {
    return { ok: false, failure: { kind: "unavailable" } };
  }
}

/**
 * Browser adapter for C04 over the generated client. It keeps only the session-paired CSRF
 * token, in memory: no credential, token or session value is ever written to storage.
 */
export function createAuthApi() {
  let csrfToken: string | undefined;
  const client = createSceneCaskClient({ getCsrfToken: () => csrfToken });

  async function session() {
    const result = await settle(() => client.GET("/session"));
    csrfToken = result.ok ? result.data.csrfToken : undefined;
    return result;
  }

  /**
   * Public auth writes rely on Origin, but a signed-in browser must also send its session's
   * token. A CSRF rejection runs nothing, so it is retried once with a fresh token.
   */
  async function write<T>(run: Run<T>): Promise<AuthResult<T>> {
    let result = await withToken(run);
    if (!result.ok && result.failure.kind === "csrf") {
      csrfToken = undefined;
      result = await withToken(run);
    }
    if (result.ok) {
      // Sign-in returns the rotated token; reset and reauth rotate the session without one.
      const next = (result.data as { csrfToken?: unknown } | undefined)?.csrfToken;
      csrfToken = typeof next === "string" ? next : undefined;
    }
    return result;
  }

  async function withToken<T>(run: Run<T>): Promise<AuthResult<T>> {
    if (!csrfToken) {
      const current = await session();
      if (!current.ok) return current;
    }
    return settle(run);
  }

  return {
    session,
    register: (email: string, password: string) =>
      write(() => client.POST("/auth/register", { body: { email, password } })),
    verify: (token: string) => write(() => client.POST("/auth/verify", { body: { token } })),
    resendVerification: (email: string) =>
      write(() => client.POST("/auth/verification-resend", { body: { email } })),
    login: (email: string, password: string) =>
      write(() => client.POST("/auth/login", { body: { email, password } })),
    requestReset: (email: string) =>
      write(() => client.POST("/auth/password-reset-request", { body: { email } })),
    resetPassword: (token: string, newPassword: string) =>
      write(() => client.POST("/auth/password-reset", { body: { token, newPassword } })),
    reauth: (password: string) => write(() => client.POST("/auth/reauth", { body: { password } })),
    updateProfile: (displayName: string, handle: string) =>
      write(() => client.PATCH("/me", { body: { displayName, handle } })),
  };
}
