import assert from "node:assert/strict";
import test from "node:test";
import { createSceneCaskClient } from "../src/index.ts";

test("typed client sends mutation body/headers and avoids shared viewer caching", async () => {
  let captured;
  const client = createSceneCaskClient({ baseUrl: "https://example.test/api/v1", fetch: async request => {
    captured = request;
    return new Response(JSON.stringify({ error: { code: "REVISION_CONFLICT", message: "Refresh and try again.", requestId: "synthetic" } }), { status: 409, headers: { "Content-Type": "application/json" } });
  } });
  const result = await client.PUT("/progress/episodes/{episodeId}", {
    params: { path: { episodeId: "10000000-0000-4000-8000-000000000001" }, header: { Origin: "https://example.test", "X-CSRF-Token": "synthetic-csrf", "Idempotency-Key": "20000000-0000-4000-8000-000000000001" } },
    body: { watched: true, expectedRevision: 0 },
  });
  assert.equal(captured.url, "https://example.test/api/v1/progress/episodes/10000000-0000-4000-8000-000000000001");
  assert.equal(captured.credentials, "same-origin");
  assert.equal(captured.cache, "no-store");
  assert.equal(captured.headers.get("X-CSRF-Token"), "synthetic-csrf");
  assert.equal(captured.headers.get("Idempotency-Key"), "20000000-0000-4000-8000-000000000001");
  assert.equal(captured.headers.get("Content-Type"), "application/json");
  assert.deepEqual(await captured.json(), { watched: true, expectedRevision: 0 });
  assert.equal(result.error.error.code, "REVISION_CONFLICT");
  assert.equal(result.data, undefined);
});

test("client instances do not share server-forwarded viewer cookies", async () => {
  const cookies = [];
  const fetch = async request => { cookies.push(request.headers.get("Cookie")); return new Response(JSON.stringify({ user: null, csrfToken: "synthetic" })); };
  const a = createSceneCaskClient({ baseUrl: "https://example.test/api/v1", headers: { Cookie: "scenecask=synthetic-a" }, fetch });
  const b = createSceneCaskClient({ baseUrl: "https://example.test/api/v1", headers: { Cookie: "scenecask=synthetic-b" }, fetch });
  await Promise.all([a.GET("/session"), b.GET("/session")]);
  assert.deepEqual(cookies.sort(), ["scenecask=synthetic-a", "scenecask=synthetic-b"]);
});

test("undefined baseUrl retains the /api/v1 prefix", async () => {
  let captured;
  class BrowserRequest extends Request {
    constructor(input, init) { super(new URL(input, "https://scenecask.test"), init); }
  }
  const client = createSceneCaskClient({ baseUrl: undefined, Request: BrowserRequest, fetch: async request => {
    captured = request;
    return new Response(JSON.stringify({ user: null, csrfToken: "synthetic" }));
  } });
  await client.GET("/session");
  assert.equal(captured.url, "https://scenecask.test/api/v1/session");
});

test("client resolves rotating CSRF tokens and server Origin once per mutation", async () => {
  const captured = [];
  let token = "synthetic-first";
  const client = createSceneCaskClient({ baseUrl: "https://example.test/api/v1", origin: "https://example.test", getCsrfToken: () => token, fetch: async request => {
    captured.push(request);
    return new Response(null, { status: 204 });
  } });
  await client.DELETE("/me/identities/google", { body: {} });
  token = "synthetic-rotated";
  await client.DELETE("/me/identities/google", { body: {} });
  assert.deepEqual(captured.map(r => r.headers.get("X-CSRF-Token")), ["synthetic-first", "synthetic-rotated"]);
  for (const request of captured) {
    assert.equal(request.headers.get("Origin"), "https://example.test");
    assert.equal(request.headers.get("Content-Type"), "application/json");
    assert.deepEqual(await request.json(), {});
  }
});

test("authenticated mutations fail before transport when the CSRF token is absent", async () => {
  let called = false;
  const client = createSceneCaskClient({ baseUrl: "https://example.test/api/v1", fetch: async () => { called = true; return new Response(); } });
  await assert.rejects(client.POST("/reveals", { body: { scope: "discussion", resourceId: "synthetic" } }), /session CSRF token is required/);
  assert.equal(called, false);
});

test("browser mutations leave Origin to the browser even with client defaults", async () => {
  const previous = Object.getOwnPropertyDescriptor(globalThis, "window");
  let captured;
  Object.defineProperty(globalThis, "window", { configurable: true, value: {} });
  try {
    const client = createSceneCaskClient({ baseUrl: "https://example.test/api/v1", origin: "https://wrong-origin.test", headers: { Origin: "https://wrong-origin.test" }, getCsrfToken: () => "synthetic", fetch: async request => {
      captured = request;
      return new Response(null, { status: 204 });
    } });
    await client.DELETE("/me/identities/google", { body: {} });
    assert.equal(captured.headers.has("Origin"), false);
    assert.equal(captured.headers.get("X-CSRF-Token"), "synthetic");
  } finally {
    if (previous) Object.defineProperty(globalThis, "window", previous);
    else delete globalThis.window;
  }
});
