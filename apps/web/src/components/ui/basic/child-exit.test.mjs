import { test } from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { waitForChildExit } from "./child-exit.mjs";
for (const code of [0, 7]) {
  test(`preserves normal child exit ${code}`, async () => {
    assert.equal(
      await waitForChildExit(
        spawn(process.execPath, ["-e", `process.exit(${code})`]),
      ),
      code,
    );
  });
}
test("a child killed by SIGKILL cannot report build success", async () => {
  assert.equal(
    await waitForChildExit(
      spawn(process.execPath, ["-e", 'process.kill(process.pid, "SIGKILL")']),
    ),
    137,
  );
});
