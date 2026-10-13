"use client";
import { useCallback, useEffect, useState } from "react";
import { endExpiredSession } from "../auth";
import type { Failure, Load } from "./api";

export type Paged<T> =
  | { state: "idle" }
  | { state: "loading" }
  | { state: "error"; failure: Failure }
  | { state: "ready"; items: T[]; nextCursor: string | null; more: "idle" | "loading" | Failure };

/**
 * A cursor-paginated C06 list. `load` identifies the list: a new function starts over, and a
 * response for a superseded `load`, Retry or unmounted view never settles. Null is idle (for
 * example a search that is too short). A 401 ends the session.
 */
export function usePaged<T>(load: Load<T> | null, key: (item: T) => string) {
  const [attempt, setAttempt] = useState(0);
  const [settled, setSettled] = useState<{ load: Load<T>; attempt: number; list: Paged<T> } | null>(null);

  useEffect(() => {
    if (!load) return;
    let live = true;
    void load().then((result) => {
      if (!live) return;
      if (!result.ok && result.failure.kind === "auth") return endExpiredSession();
      setSettled({
        load,
        attempt,
        list: result.ok
          ? { state: "ready", items: result.data.items, nextCursor: result.data.nextCursor, more: "idle" }
          : { state: "error", failure: result.failure },
      });
    });
    return () => {
      live = false;
    };
  }, [load, attempt]);

  const current = settled && settled.load === load && settled.attempt === attempt;
  const list: Paged<T> = !load ? { state: "idle" } : current ? settled.list : { state: "loading" };

  const update = useCallback(
    (change: (items: T[]) => T[]) =>
      setSettled((previous) =>
        previous && previous.list.state === "ready"
          ? { ...previous, list: { ...previous.list, items: change(previous.list.items) } }
          : previous,
      ),
    [],
  );

  const more = useCallback(async () => {
    if (!settled || settled.list.state !== "ready" || !settled.list.nextCursor || settled.list.more === "loading") return;
    const { load: owner, list } = settled;
    const next = { ...settled, list: { ...list, more: "loading" as const } };
    setSettled(next);
    const result = await owner(list.nextCursor!);
    if (!result.ok && result.failure.kind === "auth") return endExpiredSession();
    setSettled((previous) => {
      if (previous?.load !== owner || previous.list.state !== "ready") return previous;
      if (!result.ok) return { ...previous, list: { ...previous.list, more: result.failure } };
      const known = new Set(previous.list.items.map(key));
      const items = [...previous.list.items, ...result.data.items.filter((item) => !known.has(key(item)))];
      return { ...previous, list: { state: "ready", items, nextCursor: result.data.nextCursor, more: "idle" } };
    });
  }, [settled, key]);

  const retry = useCallback(() => setAttempt((count) => count + 1), []);
  return { list, retry, more, update };
}
