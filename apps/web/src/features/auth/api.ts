import type { components } from "@scenecask/api-client";
import { createAccountTransport, type BaseFailure, type Settled } from "./transport";

export type User = components["schemas"]["User"];
export type Session = components["schemas"]["Session"];
/**
 * Safe failure categories for auth screens. The UI chooses all copy: server messages are never
 * shown, and only the names of rejected fields are kept, never their messages or values.
 */
export type AuthFailure = BaseFailure | { kind: "credentials" | "unverified" | "expired" | "handle_taken" };
export type AuthResult<T> = Settled<T, AuthFailure>;

export type AuthApi = ReturnType<typeof createAuthApi>;

/**
 * Browser adapter for C04 over the generated client. It keeps only the session-paired CSRF
 * token, in memory: no credential, token or session value is ever written to storage.
 */
export function createAuthApi() {
  const { client, session, write } = createAccountTransport({
    codes: {
      INVALID_CREDENTIALS: "credentials",
      EMAIL_UNVERIFIED: "unverified",
      TOKEN_EXPIRED: "expired",
      HANDLE_TAKEN: "handle_taken",
    },
    statuses: { 410: "expired" },
  });

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
