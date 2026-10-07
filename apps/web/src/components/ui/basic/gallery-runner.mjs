import { resolve } from "node:path";
import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import { waitForChildExit } from "./child-exit.mjs";
import { createGalleryWorkspace } from "./gallery-workspace.mjs";
const output = resolve(import.meta.dirname, "../../../../../../.next");
/** Run a verification fixture app in an isolated workspace: `next dev` on port, or `next build` with --build. */
export async function runGallery(fixture, port) {
  // Parse all source module imports while preserving isolated writable build files.
  const workspace = createGalleryWorkspace(fixture, output);
  try {
    const cli = createRequire(import.meta.url).resolve("next/dist/bin/next");
    const args = process.argv.includes("--build")
      ? ["build", workspace.directory]
      : ["dev", workspace.directory, "--hostname", "127.0.0.1", "--port", `${port}`];
    const child = spawn(process.execPath, [cli, ...args], { stdio: "inherit" });
    workspace.recordChild(child.pid);
    for (const signal of ["SIGINT", "SIGTERM"])
      process.on(signal, () => child.kill(signal));
    process.exitCode = await waitForChildExit(child);
  } finally {
    workspace.cleanup();
  }
}
