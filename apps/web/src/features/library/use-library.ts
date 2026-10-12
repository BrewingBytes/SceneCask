"use client";
import { useEffect, useState, useSyncExternalStore } from "react";
import { createLibraryApi, type LibraryQuery } from "./api";
import { createLibraryClient, type ToastControls } from "./library-client";

/** One library client per mounted page; `toasts` must be stable (useToastQueue). */
export function useLibrary(initialQuery: LibraryQuery, toasts: ToastControls) {
  const [client] = useState(() => createLibraryClient(createLibraryApi(), toasts, initialQuery));
  const snapshot = useSyncExternalStore(client.subscribe, client.getSnapshot, client.getSnapshot);
  useEffect(() => {
    void client.load();
    return client.dispose;
  }, [client]);
  return { client, snapshot };
}
