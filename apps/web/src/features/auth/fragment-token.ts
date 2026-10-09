"use client";
import { useEffect, useRef } from "react";

/**
 * Reads the `#token=` an email link carries and scrubs the fragment from the address bar and
 * the history entry. Fragments never reach a server or a Referer (C04); scrubbing keeps the
 * token out of later screenshots, bookmarks and Back.
 */
function takeFragmentToken(): string | null {
  const { hash, pathname, search } = window.location;
  if (!hash) return null;
  window.history.replaceState(window.history.state, "", pathname + search);
  return new URLSearchParams(hash.slice(1)).get("token") || null;
}

/**
 * Calls `receive` once on mount with the link's token (or null), then again for every link
 * opened in this same tab later: that is a fragment-only navigation, which does not reload or
 * remount the page.
 */
export function useFragmentToken(receive: (token: string | null) => void) {
  const latest = useRef(receive);
  const started = useRef(false);

  useEffect(() => {
    latest.current = receive;
  });

  useEffect(() => {
    // Once per mount, including React's development re-run: the first read scrubs the token.
    if (!started.current) {
      started.current = true;
      latest.current(takeFragmentToken());
    }
    const changed = () => {
      const token = takeFragmentToken();
      if (token) latest.current(token);
    };
    window.addEventListener("hashchange", changed);
    return () => window.removeEventListener("hashchange", changed);
  }, []);
}
