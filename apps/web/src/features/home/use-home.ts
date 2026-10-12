"use client";
import { useEffect, useState, useSyncExternalStore } from "react";
import { createHomeApi } from "./api";
import { createHomeClient, type ToastControls } from "./home-client";

/**
 * One Home client per mounted page; `toasts` must be stable (useToastQueue). Returning to the
 * tab refetches, so a release that happened meanwhile moves a caught-up show back to Up next.
 */
export function useHome(toasts: ToastControls) {
  const [client] = useState(() => createHomeClient(createHomeApi(), toasts));
  const snapshot = useSyncExternalStore(client.subscribe, client.getSnapshot, client.getSnapshot);
  useEffect(() => {
    void client.load();
    const refetch = () => {
      if (document.visibilityState === "visible") void client.load();
    };
    document.addEventListener("visibilitychange", refetch);
    window.addEventListener("focus", refetch);
    return () => {
      document.removeEventListener("visibilitychange", refetch);
      window.removeEventListener("focus", refetch);
      client.dispose();
    };
  }, [client]);
  return { client, snapshot };
}
