import type { components } from "@scenecask/api-client";
import { createApiTransport } from "../catalog/search/api";
import { createTrackingApi } from "../tracking/api";

export type { ApiResult, Episode, Failure, LibraryItem, Release, Show } from "../tracking/api";

export type Home = components["schemas"]["Home"];

export type HomeApi = ReturnType<typeof createHomeApi>;

/**
 * Browser adapter for Home. One per viewer. Marks, catch-up and Undo go through the shared
 * tracking adapter, whose show and season reads also supply caught-up release notes.
 */
export function createHomeApi() {
  const { client, request } = createApiTransport();
  const tracking = createTrackingApi();
  return {
    get: (signal: AbortSignal) => request(() => client.GET("/home", { signal }), signal),
    tracking,
  };
}
