import type { components } from "@scenecask/api-client";
import { createAccountTransport, type BaseFailure, type Settled } from "../auth/transport";

export type User = components["schemas"]["User"];
export type Person = components["schemas"]["Person"];
export type Visibility = components["schemas"]["Visibility"];
export type PersonResult = components["schemas"]["PersonResult"];
export type VisibleLibraryItem = components["schemas"]["VisibleLibraryItem"];
export type ActivityEvent = components["schemas"]["ActivityEvent"];
export type Recommendation = components["schemas"]["Recommendation"];
export type FollowState = components["schemas"]["FollowResult"]["state"];
export interface Page<T> {
  items: T[];
  nextCursor: string | null;
}

/**
 * Safe failure categories for social screens; the UI chooses all copy. `not_found` covers
 * someone who doesn't exist, blocks either way, an inaccessible library and a request that is
 * no longer pending: the API answers all of them alike.
 */
export type Failure = BaseFailure | { kind: "not_found" };
export type Result<T> = Settled<T, Failure>;
export type Load<T> = (cursor?: string) => Promise<Result<Page<T>>>;

/** A social write the API refused because the caller hasn't chosen a handle yet (C06). */
export const needsHandle = (failure: Failure) => failure.kind === "invalid" && failure.fields.includes("handle");

/**
 * Browser adapter for the C06 people, follow, request, follower, block, activity and
 * recommendation endpoints over the generated client. Writes carry the session's CSRF token.
 */
export function createSocialApi() {
  const { client, settle, session, write } = createAccountTransport({
    codes: { NOT_FOUND: "not_found" },
    statuses: { 404: "not_found" },
  });

  const cursor = (value?: string) => (value ? { cursor: value } : {});
  const page = (value?: string) => ({ params: { query: cursor(value) } });
  const target = (id: string) => ({ params: { path: { id } }, body: {} });

  return {
    session,
    search: (q: string, after?: string) => settle(() => client.GET("/people", { params: { query: { q, ...cursor(after) } } })),
    person: (id: string) => settle(() => client.GET("/people/{id}", { params: { path: { id } } })),
    library: (id: string, after?: string) =>
      settle(() => client.GET("/people/{id}/library", { params: { path: { id }, query: cursor(after) } })),
    follow: (id: string) => write(() => client.POST("/people/{id}/follow", target(id))),
    unfollow: (id: string) => write(() => client.DELETE("/people/{id}/follow", target(id))),
    requests: (after?: string) => settle(() => client.GET("/me/follow-requests", page(after))),
    decide: (requesterId: string, decision: "approve" | "decline") =>
      write(() =>
        client.POST("/me/follow-requests/{requesterId}", { params: { path: { requesterId } }, body: { decision } }),
      ),
    followers: (after?: string) => settle(() => client.GET("/me/followers", page(after))),
    removeFollower: (id: string) => write(() => client.DELETE("/me/followers/{id}", target(id))),
    block: (id: string) => write(() => client.PUT("/me/blocks/{id}", target(id))),
    activity: (after?: string) => settle(() => client.GET("/activity", page(after))),
    recommendations: (after?: string) => settle(() => client.GET("/recommendations", page(after))),
  };
}

export type SocialApi = ReturnType<typeof createSocialApi>;
