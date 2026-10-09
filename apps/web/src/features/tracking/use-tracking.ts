"use client";
import { useEffect, useState, useSyncExternalStore } from "react";
import { createTrackingApi } from "./api";
import { createTrackingClient, type ToastControls } from "./tracking-client";

/** One tracking client per mounted show; `toasts` must be stable (useToastQueue). */
export function useTracking(showId: string, toasts: ToastControls) {
  const [client] = useState(() => createTrackingClient(createTrackingApi(), showId, toasts));
  const snapshot = useSyncExternalStore(client.subscribe, client.getSnapshot, client.getSnapshot);
  useEffect(() => {
    void client.loadShow();
  }, [client]);
  return { client, snapshot };
}
