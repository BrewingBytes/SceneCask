import { test } from "node:test";
import assert from "node:assert/strict";
import {
  mkdtempSync,
  mkdirSync,
  writeFileSync,
  existsSync,
  readFileSync,
  rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawn } from "node:child_process";
import {
  createGalleryWorkspace,
  reapGalleryWorkspaces,
} from "./gallery-workspace.mjs";
import { waitForChildExit } from "./child-exit.mjs";
test("copied sources preserve .ts, side-effect and dynamic imports; cleanup preserves source", () => {
  const root = mkdtempSync(join(tmpdir(), "r04-links-"));
  try {
    const source = join(root, "source"),
      output = join(root, "output");
    mkdirSync(join(source, "app"), { recursive: true });
    writeFileSync(
      join(source, "app", "helper.ts"),
      'import "../../styles.css"; export { value } from "../../shared.ts"; export const load = () => import("./content");',
    );
    writeFileSync(join(source, "tsconfig.json"), "{}");
    const workspace = createGalleryWorkspace(source, output);
    const copied = readFileSync(
      join(workspace.directory, "app", "helper.ts"),
      "utf8",
    );
    assert.equal(
      resolve(workspace.directory, "app", copied.match(/import "([^"]+)"/)[1]),
      join(root, "styles.css"),
    );
    assert.equal(
      resolve(workspace.directory, "app", copied.match(/from "([^"]+)"/)[1]),
      join(root, "shared.ts"),
    );
    assert.match(copied, /import\("\.\/content"\)/);
    reapGalleryWorkspaces(output);
    assert.ok(existsSync(workspace.directory));
    workspace.cleanup();
    assert.ok(existsSync(join(source, "app", "helper.ts")));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
test("next startup reaps a SIGKILL-abandoned lease but preserves unowned folders", async () => {
  const root = mkdtempSync(join(tmpdir(), "r04-leases-"));
  try {
    const source = join(root, "source"),
      output = join(root, "output");
    mkdirSync(source);
    mkdirSync(output);
    const unknown = join(output, "r04-gallery-external");
    mkdirSync(unknown);
    const child = spawn(process.execPath, [
      "--input-type=module",
      "-e",
      `import { createGalleryWorkspace } from ${JSON.stringify(new URL("./gallery-workspace.mjs", import.meta.url).href)}; createGalleryWorkspace(${JSON.stringify(source)}, ${JSON.stringify(output)}); process.kill(process.pid, "SIGKILL");`,
    ]);
    assert.equal(await waitForChildExit(child), 137);
    reapGalleryWorkspaces(output);
    assert.deepEqual((await import("node:fs")).readdirSync(output), [
      "r04-gallery-external",
    ]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
