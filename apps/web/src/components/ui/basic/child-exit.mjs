import { constants } from "node:os";
/** Preserve failure when a child is terminated instead of exiting normally. */
export function waitForChildExit(child) {
  return new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("exit", (code, signal) =>
      resolve(code ?? (signal ? 128 + (constants.signals[signal] ?? 1) : 1)),
    );
  });
}
