"use client";
import { useCallback, useEffect, useState } from "react";
import { endExpiredSession } from "../auth";
import type { Failure, SocialApi, User } from "./api";

export type Viewer = { state: "loading" } | { state: "error"; failure: Failure } | { state: "ready"; user: User };

/** The signed-in viewer from `GET /session`, which also primes the CSRF token for writes. */
export function useViewer(api: SocialApi) {
  const [viewer, setViewer] = useState<Viewer>({ state: "loading" });
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    let live = true;
    void api.session().then((result) => {
      if (!live) return;
      if (result.ok && result.data.user) return setViewer({ state: "ready", user: result.data.user });
      if (result.ok || result.failure.kind === "auth") return endExpiredSession();
      setViewer({ state: "error", failure: result.failure });
    });
    return () => {
      live = false;
    };
  }, [api, attempt]);

  const retry = useCallback(() => {
    setViewer({ state: "loading" });
    setAttempt((count) => count + 1);
  }, []);
  return { viewer, retry };
}
