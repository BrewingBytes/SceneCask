import {
  mkdtempSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  writeFileSync,
  rmSync,
} from "node:fs";
import { dirname, join, resolve, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";
import { createRequire } from "node:module";

const here = dirname(fileURLToPath(import.meta.url));
const web = resolve(here, "../../../..");
// Keep gallery artifacts separate from the production web build directory.
const output = resolve(web, "../../.next");
mkdirSync(output, { recursive: true });
// This runner cleans only the directory it created. Reusing an external server
// never launches this runner, and no shared gallery-app/.next files are deleted.
const temporary = mkdtempSync(join(output, "r04-gallery-"));
function copyFixture(source, destination) {
  mkdirSync(destination, { recursive: true });
  for (const entry of readdirSync(source, { withFileTypes: true })) {
    if (
      [".next", "next-env.d.ts", "AGENTS.md", "CLAUDE.md"].includes(entry.name)
    )
      continue;
    const from = join(source, entry.name);
    const to = join(destination, entry.name);
    if (entry.isDirectory()) copyFixture(from, to);
    else {
      let content = readFileSync(from);
      if (entry.name.endsWith(".tsx"))
        content = content
          .toString("utf8")
          .replace(/from "(\.[^"]+)"/g, (_, specifier) => {
            const target = relative(
              dirname(to),
              resolve(dirname(from), specifier),
            );
            return `from ${JSON.stringify(target.startsWith(".") ? target : `./${target}`)}`;
          });
      writeFileSync(to, content);
    }
  }
}
try {
  copyFixture(join(here, "gallery-app"), temporary);
  // .next has a CommonJS package boundary; this temporary app uses ESM source.
  writeFileSync(
    join(temporary, "package.json"),
    JSON.stringify({ private: true, type: "module" }),
  );
  const cli = createRequire(import.meta.url).resolve("next/dist/bin/next");
  const args = process.argv.includes("--build")
    ? ["build", temporary]
    : ["dev", temporary, "--hostname", "127.0.0.1", "--port", "3104"];
  const child = spawn(process.execPath, [cli, ...args], { stdio: "inherit" });
  for (const signal of ["SIGINT", "SIGTERM"])
    process.on(signal, () => child.kill(signal));
  process.exitCode = await new Promise((resolveExit, reject) => {
    child.once("error", reject);
    child.once("exit", (code) => resolveExit(code ?? 0));
  });
} finally {
  rmSync(temporary, { recursive: true, force: true });
}
