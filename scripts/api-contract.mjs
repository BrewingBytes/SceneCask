import { existsSync } from "node:fs";
import { spawnSync } from "node:child_process";
// R03 owns the canonical schema and generator configuration. Fail closed once a schema exists.
const mode = process.argv[2];
if (!["generate", "check"].includes(mode)) throw new Error("Expected generate or check");
if (!existsSync("contracts/openapi.yaml")) {
  console.log("R03 pending: no canonical OpenAPI schema; generation/drift check is not yet applicable.");
} else {
  if (!existsSync("packages/api-client/package.json")) throw new Error("R03 must configure packages/api-client before generation can pass");
  const result = spawnSync("pnpm", ["--dir", "packages/api-client", "run", `api:${mode}`], { stdio: "inherit" });
  process.exitCode = result.status ?? 1;
}
