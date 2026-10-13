import type { components } from "@scenecask/api-client";
import { createAccountTransport, type BaseFailure, type Settled } from "../../auth/transport";

export type User = components["schemas"]["User"];
export type Identities = components["schemas"]["Identities"];
export type Person = components["schemas"]["Person"];
export type BlockedPerson = components["schemas"]["BlockedPerson"];
export type Visibility = components["schemas"]["Visibility"];
export interface Page<T> {
  items: T[];
  nextCursor: string | null;
}

/** Safe failure categories for settings; the UI chooses all copy. */
export type Failure = BaseFailure | { kind: "last_method" | "handle_taken" | "not_found" };
export type Result<T> = Settled<T, Failure>;

/**
 * Browser adapter for the C04 profile/identity and C06 privacy, follower and block endpoints over
 * the generated client. Reauthentication rotates the session; the shared transport refreshes the
 * CSRF token and retries once.
 */
export function createSettingsApi() {
  const { client, settle, session, write } = createAccountTransport({
    codes: { LAST_SIGNIN_METHOD: "last_method", HANDLE_TAKEN: "handle_taken", NOT_FOUND: "not_found" },
    statuses: { 404: "not_found" },
  });

  const page = (cursor?: string) => ({ params: { query: cursor ? { cursor } : {} } });
  const target = (id: string) => ({ params: { path: { id } }, body: {} });

  return {
    session,
    identities: () => settle(() => client.GET("/me/identities")),
    updateProfile: (displayName: string, handle: string) =>
      write(() => client.PATCH("/me", { body: { displayName, handle } })),
    unlinkGoogle: () => write(() => client.DELETE("/me/identities/google", { body: {} })),
    /** C04: an SSO-only account establishes a password by proving mailbox control. */
    requestPassword: (email: string) => write(() => client.POST("/auth/password-reset-request", { body: { email } })),
    logout: () => write(() => client.POST("/auth/logout", { body: {} })),
    setVisibility: (visibility: Visibility) => write(() => client.PATCH("/me/privacy", { body: { visibility } })),
    followers: (cursor?: string) => settle(() => client.GET("/me/followers", page(cursor))),
    removeFollower: (id: string) => write(() => client.DELETE("/me/followers/{id}", target(id))),
    blocks: (cursor?: string) => settle(() => client.GET("/me/blocks", page(cursor))),
    unblock: (id: string) => write(() => client.DELETE("/me/blocks/{id}", target(id))),
  };
}

export type SettingsApi = ReturnType<typeof createSettingsApi>;
