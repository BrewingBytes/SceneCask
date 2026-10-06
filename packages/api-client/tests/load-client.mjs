import assert from "node:assert/strict";
import { mkdtemp, rm } from "node:fs/promises";
import { fileURLToPath, pathToFileURL } from "node:url";
import ts from "typescript";

/** Test emitted JavaScript, including the same .js specifiers consumed by the web app. */
export async function loadClient() {
  const output = await mkdtemp(fileURLToPath(new URL("../.runtime-test-", import.meta.url)));
  try {
    const program = ts.createProgram([fileURLToPath(new URL("../src/index.ts", import.meta.url))], {
      target: ts.ScriptTarget.ES2022,
      module: ts.ModuleKind.NodeNext,
      moduleResolution: ts.ModuleResolutionKind.NodeNext,
      strict: true,
      types: [],
      rootDir: fileURLToPath(new URL("../src", import.meta.url)),
      outDir: output,
    });
    const diagnostics = ts.getPreEmitDiagnostics(program);
    assert.equal(diagnostics.length, 0, ts.formatDiagnosticsWithColorAndContext(diagnostics, {
      getCurrentDirectory: () => process.cwd(), getCanonicalFileName: name => name, getNewLine: () => "\n",
    }));
    assert.equal(program.emit().emitSkipped, false);
    return await import(pathToFileURL(`${output}/index.js`).href);
  } finally { await rm(output, { recursive: true, force: true }); }
}
