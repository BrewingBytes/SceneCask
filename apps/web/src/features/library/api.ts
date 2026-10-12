import type { components } from "@scenecask/api-client";
import { createApiTransport } from "../catalog/search/api";
import { createTrackingApi } from "../tracking/api";

export { actionIdOf, outcomeUnknown, type ApiResult, type Failure, type MutationResult } from "../tracking/api";

type Schemas = components["schemas"];
export type LibraryItem = Schemas["LibraryItem"];
export type LibraryStatus = Schemas["LibraryStatus"];
export type LibraryPage = Schemas["LibraryPage"];
export type LibraryCounts = NonNullable<LibraryPage["counts"]>;

export type LibraryApi = ReturnType<typeof createLibraryApi>;

export interface LibraryQuery {
  status: LibraryStatus | null;
  q: string;
}

/**
 * Browser adapter for the personal library list and its row mutations. One per viewer. Writes
 * and the single-show recovery read are the shared tracking adapter's.
 */
export function createLibraryApi() {
  const { client, request } = createApiTransport();
  const tracking = createTrackingApi();

  return {
    list: (query: LibraryQuery, cursor: string | undefined, signal: AbortSignal) =>
      request(
        () =>
          client.GET("/library", {
            params: { query: { status: query.status ?? undefined, q: query.q || undefined, cursor } },
            signal,
          }),
        signal,
      ),
    /** The authoritative entry for one show, used to recover after a rejected write. */
    getShow: tracking.getShow,
    setStatus: tracking.setStatus,
    remove: (showId: string, expectedRevision: number, key: string) =>
      tracking.saveToLibrary(showId, { saved: false, expectedRevision }, key),
    undo: tracking.undo,
  };
}
