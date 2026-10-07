import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  writeFileSync,
  rmSync,
} from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";
import ts from "typescript";
function alive(pid) {
  if (!Number.isInteger(pid) || pid <= 0) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return error.code !== "ESRCH";
  }
}
/** Reap only recorded runner-owned leases whose owner and child have exited. */
export function reapGalleryWorkspaces(output) {
  mkdirSync(output, { recursive: true });
  for (const entry of readdirSync(output, { withFileTypes: true })) {
    if (!entry.isDirectory() || !entry.name.startsWith("r04-gallery-"))
      continue;
    const folder = join(output, entry.name);
    try {
      const lease = JSON.parse(
        readFileSync(join(folder, ".r04-owner.json"), "utf8"),
      );
      if (
        lease.kind === "scenecask-r04" &&
        Number.isInteger(lease.owner) &&
        !alive(lease.owner) &&
        !alive(lease.child)
      )
        rmSync(folder, { recursive: true, force: true });
    } catch {
      /* Unknown directories are not ours to remove. */
    }
  }
}
export function rewriteFixtureImports(content, from, to, source, destination) {
  const parsed = ts.createSourceFile(
    from,
    content,
    ts.ScriptTarget.Latest,
    true,
  );
  const edits = [];
  function visit(node) {
    const specifier =
      ts.isImportDeclaration(node) || ts.isExportDeclaration(node)
        ? node.moduleSpecifier
        : ts.isCallExpression(node) &&
            (node.expression.kind === ts.SyntaxKind.ImportKeyword ||
              (ts.isIdentifier(node.expression) &&
                node.expression.text === "require"))
          ? node.arguments[0]
          : undefined;
    if (
      specifier &&
      ts.isStringLiteralLike(specifier) &&
      specifier.text.startsWith(".")
    ) {
      const original = resolve(dirname(from), specifier.text);
      const local = relative(source, original);
      const target =
        local !== ".." && !local.startsWith(`..${sep}`)
          ? resolve(destination, local)
          : original;
      const adjusted = relative(dirname(to), target).split(sep).join("/");
      edits.push({
        start: specifier.getStart(parsed),
        end: specifier.end,
        text: JSON.stringify(
          adjusted.startsWith(".") ? adjusted : `./${adjusted}`,
        ),
      });
    }
    ts.forEachChild(node, visit);
  }
  visit(parsed);
  for (const edit of edits.sort((a, b) => b.start - a.start))
    content =
      content.slice(0, edit.start) + edit.text + content.slice(edit.end);
  return content;
}
function copyFixture(from, to, source, destination) {
  mkdirSync(to, { recursive: true });
  for (const entry of readdirSync(from, { withFileTypes: true })) {
    if (
      [
        ".next",
        "next-env.d.ts",
        "AGENTS.md",
        "CLAUDE.md",
        "package.json",
      ].includes(entry.name)
    )
      continue;
    const original = join(from, entry.name),
      target = join(to, entry.name);
    if (entry.isDirectory()) copyFixture(original, target, source, destination);
    else {
      const bytes = readFileSync(original);
      writeFileSync(
        target,
        /\.[cm]?[jt]sx?$/.test(entry.name)
          ? rewriteFixtureImports(
              bytes.toString("utf8"),
              original,
              target,
              source,
              destination,
            )
          : bytes,
      );
    }
  }
}
export function createGalleryWorkspace(source, output) {
  reapGalleryWorkspaces(output);
  const directory = mkdtempSync(join(output, "r04-gallery-"));
  const recordChild = (child) =>
    writeFileSync(
      join(directory, ".r04-owner.json"),
      JSON.stringify({ kind: "scenecask-r04", owner: process.pid, child }),
    );
  recordChild(null);
  try {
    copyFixture(source, directory, source, directory);
    writeFileSync(
      join(directory, "package.json"),
      JSON.stringify({ private: true, type: "module" }),
    );
    return {
      directory,
      recordChild,
      cleanup: () => rmSync(directory, { recursive: true, force: true }),
    };
  } catch (error) {
    rmSync(directory, { recursive: true, force: true });
    throw error;
  }
}
