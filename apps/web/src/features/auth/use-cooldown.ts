"use client";
import { useCallback, useEffect, useState } from "react";

/** C04 resend cooldown (60s) or a 429's Retry-After: seconds left, and a starter. */
export function useCooldown() {
  const [until, setUntil] = useState(0);
  const [now, setNow] = useState(0);

  useEffect(() => {
    if (until <= now) return;
    const timer = setTimeout(() => setNow(Date.now()), Math.min(1000, until - now));
    return () => clearTimeout(timer);
  }, [until, now]);

  const start = useCallback((seconds: number) => {
    const started = Date.now();
    setNow(started);
    setUntil(started + seconds * 1000);
  }, []);

  return [Math.max(0, Math.ceil((until - now) / 1000)), start] as const;
}

export const RESEND_COOLDOWN_SECONDS = 60;
