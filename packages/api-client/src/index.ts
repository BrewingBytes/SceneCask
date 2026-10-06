import createClient, { type ClientOptions } from "openapi-fetch";
import type { paths } from "./generated/schema.js";

export type { paths, components, operations } from "./generated/schema.js";

/** Create per viewer/request. SSR callers must explicitly forward that viewer's cookie. */
export function createSceneCaskClient(options: ClientOptions = {}) {
  return createClient<paths>({
    baseUrl: "/api/v1",
    ...options,
    credentials: "same-origin",
    cache: "no-store",
  });
}
