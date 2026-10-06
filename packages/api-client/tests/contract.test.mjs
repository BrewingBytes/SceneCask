import assert from "node:assert/strict";
import { readFile, mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import Ajv2020 from "ajv/dist/2020.js";
import addFormats from "ajv-formats";
import YAML from "yaml";
import { buildArtifacts, saveArtifacts } from "../scripts/generate.mjs";

const spec = YAML.parse(await readFile(new URL("../../../contracts/openapi.yaml", import.meta.url), "utf8"));
const ajv = new Ajv2020({ strict: false, allErrors: true });
addFormats(ajv);
ajv.addSchema({ $id: "https://scenecask.test/contract", components: spec.components });
const validate = (schema, value) => ajv.compile({ ...schema, components: spec.components })(value);
const methods = new Set(["get", "post", "put", "patch", "delete"]);
const resolve = value => value?.$ref ? value.$ref.slice(2).split("/").reduce((node, key) => node[key], spec) : value;
const operations = Object.entries(spec.paths).flatMap(([route, item]) => Object.entries(item)
  .filter(([method]) => methods.has(method)).map(([method, operation]) => ({ route, method, operation })));

test("every C04–C08 endpoint is represented once, with implementation ownership", async () => {
  const source = await readFile(new URL("../../../docs/rebuild/contracts.md", import.meta.url), "utf8");
  const required = new Set([...source.matchAll(/\b(GET|POST|PUT|PATCH|DELETE) (\/[\w/{}-]+)/g)].map(m => `${m[1]} ${m[2]}`));
  const actual = new Set(operations.map(({ route, method }) => `${method.toUpperCase()} ${route}`));
  assert.deepEqual([...actual].sort(), [...required].sort());
  assert.equal(new Set(operations.map(o => o.operation.operationId)).size, actual.size);
  const mapping = JSON.parse(await readFile(new URL("../../../contracts/operation-issues.json", import.meta.url), "utf8"));
  assert.equal(mapping.length, actual.size);
  for (const { operation } of operations) {
    assert.match(operation["x-issue"], /^https:\/\/github\.com\/BrewingBytes\/SceneCask\/issues\/\d+$/);
    assert.match(operation["x-rebuild-owner"], /^R\d{2}$/);
  }
});

test("all named DTO and endpoint request/success/error examples validate", () => {
  for (const [name, schema] of Object.entries(spec.components.schemas)) {
    assert.ok(validate({ $ref: `#/components/schemas/${name}` }, schema.example), name);
  }
  for (const { operation } of operations) {
    const payloads = [operation.requestBody, ...Object.values(operation.responses)].filter(Boolean).map(resolve);
    for (const payload of payloads) for (const media of Object.values(payload.content ?? {})) {
      assert.ok(media.schema, `${operation.operationId} requires a typed payload`);
      assert.ok(Object.keys(media.examples ?? {}).length, `${operation.operationId} requires examples`);
      for (const example of Object.values(media.examples)) assert.ok(validate(media.schema, example.value), operation.operationId);
    }
  }
});

test("locked episode accepts the exact canonical redaction and rejects protected/unknown fields", () => {
  const locked = spec.components.schemas.LockedEpisode.example;
  assert.ok(validate({ $ref: "#/components/schemas/Episode" }, locked));
  for (const [key, value] of Object.entries({ details: { title: "synthetic" }, title: "synthetic", overview: "synthetic", stillUrl: "https://example.test/still.png", providerPayload: {} })) {
    assert.equal(validate({ $ref: "#/components/schemas/Episode" }, { ...locked, [key]: value }), false, key);
  }
  const unlocked = spec.components.schemas.WatchedEpisode.example;
  assert.ok(validate({ $ref: "#/components/schemas/Episode" }, unlocked));
  const { details, ...missingDetails } = unlocked;
  assert.equal(validate({ $ref: "#/components/schemas/Episode" }, missingDetails), false);
  assert.equal(validate({ $ref: "#/components/schemas/Episode" }, { ...unlocked, detailsAccess: "locked" }), false);
  assert.equal(validate({ $ref: "#/components/schemas/Episode" }, { ...locked, watched: true }), false);
  assert.equal(validate({ $ref: "#/components/schemas/Episode" }, { ...unlocked, watched: false }), false);
  assert.ok(validate({ $ref: "#/components/schemas/Episode" }, { ...unlocked, detailsAccess: "revealed", watched: false }));
});

test("mutation boundaries reject unknown properties and invalid statuses/revisions", () => {
  for (const { operation } of operations) {
    if (!operation.requestBody) continue;
    const media = operation.requestBody.content["application/json"];
    const example = media.examples.request.value;
    assert.equal(validate(media.schema, { ...example, operator: true }), false, operation.operationId);
  }
  assert.equal(validate({ $ref: "#/components/schemas/ProgressRequest" }, { watched: true, expectedRevision: -1 }), false);
  assert.equal(validate({ $ref: "#/components/schemas/StatusRequest" }, { status: "completed", expectedRevision: 0 }), false);
  assert.equal(validate({ $ref: "#/components/schemas/DeleteRequest" }, { confirmation: "delete" }), false);
});

test("tombstones, visible libraries and errors cannot acquire protected keys", () => {
  const tombstone = spec.components.schemas.RemovedComment.example;
  assert.ok(validate({ $ref: "#/components/schemas/Comment" }, tombstone));
  assert.equal(validate({ $ref: "#/components/schemas/Comment" }, { ...tombstone, body: "removed text" }), false);
  const visible = spec.components.schemas.VisibleLibraryItem.example;
  assert.equal(validate({ $ref: "#/components/schemas/VisibleLibraryItem" }, { ...visible, revision: 1 }), false);
  const error = spec.components.schemas.Error.example;
  assert.equal(validate({ $ref: "#/components/schemas/Error" }, { ...error, details: "protected" }), false);
  assert.equal(validate({ $ref: "#/components/schemas/Error" }, { error: { ...error.error, stillUrl: "https://example.test/still.png" } }), false);
});

test("session, CSRF, idempotency, response caching and pagination conventions", () => {
  for (const { route, method, operation } of operations) {
    const parameters = (operation.parameters ?? []).map(p => p.$ref ? spec.components.parameters[p.$ref.split("/").at(-1)] : p);
    if (method !== "get") {
      assert.ok(parameters.some(p => p.name === "Origin" && p.required), operation.operationId);
      if (operation.security.length) assert.ok(parameters.some(p => p.name === "X-CSRF-Token" && p.required), operation.operationId);
    }
    if (method !== "get" && (/^\/(library|progress|actions)\//.test(route) || /catch-up|\/history$/.test(route))) {
      assert.ok(parameters.some(p => p.name === "Idempotency-Key" && p.required), operation.operationId);
    }
    for (const [status, value] of Object.entries(operation.responses)) {
      const response = resolve(value);
      assert.deepEqual(resolve(response.headers["Cache-Control"]).schema.enum, ["private, no-store"]);
      if (status === "204" || status === "303") assert.equal(response.content, undefined);
      if (status === "429") assert.ok(response.headers["Retry-After"]);
    }
  }
});

test("regeneration is deterministic and changed schemas fail drift check", async () => {
  const temp = await mkdtemp(path.join(tmpdir(), "scenecask-api-drift-"));
  try {
    const baseline = await buildArtifacts(spec);
    await saveArtifacts(baseline, temp, false);
    await saveArtifacts(await buildArtifacts(spec), temp, true);
    const changed = structuredClone(spec);
    changed.components.schemas.User.properties.contractDriftSentinel = { type: "boolean" };
    await assert.rejects(saveArtifacts(await buildArtifacts(changed), temp, true), /Generated contract drift/);
  } finally { await rm(temp, { recursive: true, force: true }); }
});
