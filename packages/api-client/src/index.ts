import createClient, { type ClientOptions } from "openapi-fetch";
import type { paths } from "./generated/schema.js";
import { csrfRequiredOperations } from "#generated/security";

export type { paths, components, operations } from "./generated/schema.js";

type DefaultHeaders = "Origin" | "X-CSRF-Token";
type WithDefaultHeaders<Header> = Omit<Header, DefaultHeaders> & Partial<Pick<Header, Extract<keyof Header, DefaultHeaders>>>;
type WithClientParameters<Parameters> = Parameters extends { header: infer Header }
  ? Omit<Parameters, "header"> & ({} extends Omit<Header, DefaultHeaders>
    ? { header?: WithDefaultHeaders<Header> }
    : { header: WithDefaultHeaders<Header> })
  : Parameters;
type WithClientOperation<Operation> = Operation extends { parameters: infer Parameters }
  ? Omit<Operation, "parameters"> & { parameters: WithClientParameters<Parameters> }
  : Operation;
type ClientPaths = { [Route in keyof paths]: { [Method in keyof paths[Route]]: WithClientOperation<paths[Route][Method]> } };

export interface SceneCaskClientOptions extends ClientOptions {
  /** Public origin for server-side requests. Browsers supply their own Origin. */
  origin?: string;
  /** Resolve the current viewer/session token on each mutation, including after rotation. */
  getCsrfToken?: () => string | undefined;
}

const requiresCsrf = new Set<string>(csrfRequiredOperations);

/** Create per viewer/request. SSR callers must explicitly forward that viewer's cookie. */
export function createSceneCaskClient(options: SceneCaskClientOptions = {}) {
  const { origin, getCsrfToken, ...clientOptions } = options;
  const client = createClient<ClientPaths>({
    ...clientOptions,
    baseUrl: clientOptions.baseUrl ?? "/api/v1",
    credentials: "same-origin",
    cache: "no-store",
  });
  client.use({ onRequest({ request, schemaPath }) {
    if (["GET", "HEAD", "OPTIONS"].includes(request.method)) return;
    if (typeof window === "undefined") {
      if (!request.headers.has("Origin") && origin) request.headers.set("Origin", origin);
      if (!request.headers.get("Origin")) throw new Error("A public Origin is required for a server-side mutation.");
    }
    // Never send a script-set browser Origin; the browser supplies its authoritative value.
    if (typeof window !== "undefined") request.headers.delete("Origin");
    if (!request.headers.has("X-CSRF-Token")) {
      const csrfToken = getCsrfToken?.();
      if (csrfToken) request.headers.set("X-CSRF-Token", csrfToken);
    }
    if (requiresCsrf.has(`${request.method} ${schemaPath}`) && !request.headers.get("X-CSRF-Token")) {
      throw new Error("A session CSRF token is required for this request.");
    }
  } });
  return client;
}
