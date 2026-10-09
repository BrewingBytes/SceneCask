"use client";
import { useState, type Dispatch, type SetStateAction } from "react";
import type { useToastQueue } from "../../../components/ui/overlays";
import { newIdempotencyKey, outcomeUnknown, type ApiResult, type DiscoverApi, type SaveRequest, type SearchShow } from "./api";
import { failureCopy } from "./copy";

export type ToastControls = Pick<ReturnType<typeof useToastQueue>, "show" | "update" | "dismiss">;

export interface RowState {
  busy?: "open" | "add";
  /** Known from this session's import or save; search results carry no library state. */
  inLibrary?: boolean;
}
type Rows = Record<number, RowState>;

interface PendingSave {
  showId: string;
  body: SaveRequest;
  key: string;
}

const errorId = (providerId: number) => `search-error-${providerId}`;
const successId = (providerId: number) => `search-added-${providerId}`;

/**
 * Open (import, then navigate by application ID) and Add to Plan to watch. Imports are cached
 * per provider ID. A save whose outcome is unknown is retried with the same Idempotency-Key and
 * body, so a retry can never create a second library action; a known rejection starts fresh
 * from the authoritative show/library revision.
 */
function createLibraryActions(
  api: DiscoverApi,
  toasts: ToastControls,
  navigate: (href: string) => void,
  setRows: Dispatch<SetStateAction<Rows>>,
) {
  const imported = new Map<number, string>();
  const pendingSaves = new Map<number, PendingSave>();
  const undoKeys = new Map<string, string>();
  const running = new Set<number>();

  const setRow = (providerId: number, patch: RowState) =>
    setRows((current) => ({ ...current, [providerId]: { ...current[providerId], ...patch } }));

  async function ensureImported(providerId: number): Promise<ApiResult<string>> {
    const known = imported.get(providerId);
    if (known) return { ok: true, data: known };
    const result = await api.importShow(providerId);
    if (!result.ok) return result;
    imported.set(providerId, result.data.showId);
    return { ok: true, data: result.data.showId };
  }

  /** One in-flight action per result; a second activation is ignored rather than queued. */
  async function exclusive(show: SearchShow, busy: "open" | "add", action: () => Promise<void>) {
    if (running.has(show.providerId)) return;
    running.add(show.providerId);
    toasts.dismiss(errorId(show.providerId));
    setRow(show.providerId, { busy });
    try {
      await action();
    } finally {
      running.delete(show.providerId);
      setRow(show.providerId, { busy: undefined });
    }
  }

  function open(show: SearchShow) {
    return exclusive(show, "open", async () => {
      const showId = await ensureImported(show.providerId);
      if (showId.ok) return navigate(`/shows/${showId.data}`);
      toasts.show({
        id: errorId(show.providerId),
        tone: "error",
        message: `Couldn’t open ${show.title}. ${failureCopy(showId.failure)}`,
        onRetry: () => void open(show),
      });
    });
  }

  async function undo(show: SearchShow, actionId: string, toastId: string) {
    const key = undoKeys.get(actionId) ?? newIdempotencyKey();
    undoKeys.set(actionId, key);
    toasts.update(toastId, { busy: "undo" });
    const result = await api.undo(actionId, key);
    if (result.ok) {
      undoKeys.delete(actionId);
      setRow(show.providerId, { inLibrary: result.data.library.saved });
      toasts.show({
        id: toastId,
        tone: "success",
        message: result.data.library.saved
          ? `${show.title} stays in your library because it changed after you added it.`
          : `Removed ${show.title} from your library.`,
      });
      return;
    }
    if (!outcomeUnknown(result.failure)) undoKeys.delete(actionId);
    const expired = result.failure.kind === "expired";
    toasts.show({
      id: toastId,
      tone: "error",
      message: expired
        ? `Undo is no longer available. ${show.title} is still in your library.`
        : `Couldn’t undo. ${show.title} is still in your library. ${failureCopy(result.failure)}`,
      onRetry: expired ? undefined : () => void undo(show, actionId, toastId),
    });
  }

  function add(show: SearchShow) {
    return exclusive(show, "add", async () => {
      const fail = (message: string) => {
        toasts.show({
          id: errorId(show.providerId),
          tone: "error",
          message: `Couldn’t add ${show.title}. ${message}`,
          onRetry: () => void add(show),
        });
      };
      let pending = pendingSaves.get(show.providerId);
      let hadHistory = false;
      if (!pending) {
        const showId = await ensureImported(show.providerId);
        if (!showId.ok) return fail(failureCopy(showId.failure));
        const detail = await api.getShow(showId.data);
        if (!detail.ok) return fail(failureCopy(detail.failure));
        const entry = detail.data.library;
        if (entry?.saved) {
          setRow(show.providerId, { inLibrary: true });
          toasts.show({ id: successId(show.providerId), tone: "info", message: `${show.title} is already in your library.` });
          return;
        }
        hadHistory = (entry?.progress.watched ?? 0) > 0;
        pending = {
          showId: showId.data,
          key: newIdempotencyKey(),
          // An existing entry keeps its manual status; a new one starts as Plan to watch.
          body: entry
            ? { saved: true, expectedRevision: entry.revision }
            : { saved: true, status: "plan_to_watch", expectedRevision: 0 },
        };
      }
      const result = await api.saveToLibrary(pending.showId, pending.body, pending.key);
      if (!result.ok) {
        if (outcomeUnknown(result.failure)) pendingSaves.set(show.providerId, pending);
        else pendingSaves.delete(show.providerId);
        return fail(failureCopy(result.failure));
      }
      pendingSaves.delete(show.providerId);
      const { library } = result.data;
      // The generated success type keeps only the changed branch's actionId; a no-op has none.
      const actionId = "actionId" in result.data ? result.data.actionId : null;
      setRow(show.providerId, { inLibrary: library.saved });
      const toastId = successId(show.providerId);
      toasts.show({
        id: toastId,
        tone: "success",
        message:
          hadHistory || library.progress.watched > 0
            ? `${show.title} is back in your library with your progress.`
            : `Added ${show.title} to Plan to watch.`,
        onUndo: actionId ? () => void undo(show, actionId, toastId) : undefined,
      });
    });
  }

  return { open, add };
}

/** Inputs must be stable for the component's lifetime (created once, useToastQueue, router). */
export function useLibraryActions(api: DiscoverApi, toasts: ToastControls, navigate: (href: string) => void) {
  const [rows, setRows] = useState<Rows>({});
  const [actions] = useState(() => createLibraryActions(api, toasts, navigate, setRows));
  return { rows, ...actions };
}
