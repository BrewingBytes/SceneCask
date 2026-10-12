import type { ToastControls } from "../catalog/search/use-library-actions";
import { keyedUndo, newIdempotencyKey, outcomeUnknown, type ApiResult, type UndoResult } from "./api";
import { plural, trackingFailureCopy } from "./copy";

/**
 * Idempotency-Keys per write scope. The same body after an unknown outcome (network/503) reuses
 * its key, so a retry is never applied twice; a known outcome discards it.
 */
export function createIdempotencyKeys() {
  const pending = new Map<string, { key: string; body: string }>();
  return {
    keyFor(scope: string, body: unknown) {
      const text = JSON.stringify(body);
      const known = pending.get(scope);
      if (known && known.body === text) return known.key;
      const key = newIdempotencyKey();
      pending.set(scope, { key, body: text });
      return key;
    },
    settle(scope: string, result: ApiResult<unknown>) {
      if (result.ok || !outcomeUnknown(result.failure)) pending.delete(scope);
    },
  };
}

/**
 * Undo from a success toast: reports skipped later edits, keeps Retry for failures other than
 * expiry, and hands a successful result to `onUndone` for the authoritative refresh.
 */
export function createToastUndo(
  undoAction: (actionId: string, key: string) => Promise<ApiResult<UndoResult>>,
  toasts: ToastControls,
  onUndone: (result: UndoResult) => unknown,
) {
  const runUndo = keyedUndo(undoAction);
  async function undo(actionId: string, toastId: string, undone: string): Promise<void> {
    toasts.update(toastId, { busy: "undo" });
    const result = await runUndo(actionId);
    if (result.ok) {
      const { skipped } = result.data;
      toasts.show({
        id: toastId,
        tone: "success",
        message: skipped ? `Undone. ${plural(skipped, "change")} you made afterwards ${skipped === 1 ? "was" : "were"} kept.` : undone,
      });
      await onUndone(result.data);
      return;
    }
    const expired = result.failure.kind === "expired";
    toasts.show({
      id: toastId,
      tone: "error",
      message: expired ? "Undo is no longer available." : `Couldn’t undo. ${trackingFailureCopy(result.failure)}`,
      onRetry: expired ? undefined : () => void undo(actionId, toastId, undone),
    });
  }
  return undo;
}
