import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

// Ratchet: lower these when a change removes clones. The percentage threshold in .jscpd.json
// loosens as the codebase grows, so absolute counts gate new copy-paste.
const baseline = { clones: 10, duplicatedLines: 91 };

const output = mkdtempSync(join(tmpdir(), "jscpd-"));
try {
  const run = spawnSync("jscpd", ["--config", ".jscpd.json", "--reporters", "console,json", "--output", output], { stdio: "inherit" });
  // jscpd reports its own percentage-threshold failure.
  if (run.status !== 0) process.exitCode = run.status ?? 1;
  else {
    const { total } = JSON.parse(readFileSync(join(output, "jscpd-report.json"), "utf8")).statistics;
    const over = Object.entries(baseline).filter(([key, max]) => total[key] > max);
    if (over.length > 0) {
      for (const [key, max] of over) console.error(`Duplication: ${key} ${total[key]} exceeds baseline ${max}.`);
      console.error("Move the shared code into a helper (API tests: services/api/tests/common).");
      process.exitCode = 1;
    }
  }
} finally {
  rmSync(output, { recursive: true, force: true });
}
