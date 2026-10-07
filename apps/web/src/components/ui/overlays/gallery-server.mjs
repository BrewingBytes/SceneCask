import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";
import { createRequire } from "node:module";
// Reuse R04's isolated workspace runner; it copies only this module's fixture app.
import { waitForChildExit } from "../basic/child-exit.mjs";
import { createGalleryWorkspace } from "../basic/gallery-workspace.mjs";
const here = dirname(fileURLToPath(import.meta.url));
const output = resolve(here, "../../../../../../.next");
const workspace = createGalleryWorkspace(resolve(here, "gallery-app"), output);
try {
  const cli = createRequire(import.meta.url).resolve("next/dist/bin/next");
  const args = process.argv.includes("--build")
    ? ["build", workspace.directory]
    : ["dev", workspace.directory, "--hostname", "127.0.0.1", "--port", "3105"];
  const child = spawn(process.execPath, [cli, ...args], { stdio: "inherit" });
  workspace.recordChild(child.pid);
  for (const signal of ["SIGINT", "SIGTERM"])
    process.on(signal, () => child.kill(signal));
  process.exitCode = await waitForChildExit(child);
} finally {
  workspace.cleanup();
}
