"use client";
import { useEffect, useRef, useState } from "react";
import { Button } from "../../components/ui/basic";
import { googleStartHref, type GoogleIntent } from "./routes";

export interface GoogleButtonProps {
  intent?: GoogleIntent;
  /** Application path to land on afterwards; the API validates it again. */
  returnTo?: string;
  label?: string;
}

/**
 * Starts C04 Google sign-in, link or reauth as a full navigation. While the redirect is
 * pending the button is busy, so repeated activation cannot start a second flow. Returning
 * with Back (bfcache) re-enables it.
 */
export function GoogleButton({ intent = "signin", returnTo, label = "Continue with Google" }: GoogleButtonProps) {
  const [pending, setPending] = useState(false);
  // Blocks a second activation in the same task, before the busy state has rendered.
  const started = useRef(false);

  useEffect(() => {
    const restored = (event: PageTransitionEvent) => {
      if (!event.persisted) return;
      started.current = false;
      setPending(false);
    };
    window.addEventListener("pageshow", restored);
    return () => window.removeEventListener("pageshow", restored);
  }, []);

  return (
    <Button
      variant="secondary"
      className="sc-auth-google"
      busy={pending}
      onClick={() => {
        if (started.current) return;
        started.current = true;
        setPending(true);
        window.location.assign(googleStartHref(intent, returnTo));
      }}
    >
      <GoogleMark />
      {label}
    </Button>
  );
}

/** Google's multicolour "G", inline so account pages load no third-party resources (C04). */
function GoogleMark() {
  return (
    <svg className="sc-auth-google-mark" viewBox="0 0 18 18" width="18" height="18" aria-hidden="true" focusable="false">
      <path
        fill="#EA4335"
        d="M9 3.48c1.69 0 2.83.73 3.48 1.34l2.54-2.48C13.46.89 11.43 0 9 0 5.48 0 2.44 2.02.96 4.96l2.91 2.26C4.6 5.05 6.62 3.48 9 3.48z"
      />
      <path
        fill="#4285F4"
        d="M17.64 9.2c0-.74-.06-1.28-.19-1.84H9v3.34h4.96c-.1.83-.64 2.08-1.84 2.92l2.84 2.2c1.7-1.57 2.68-3.88 2.68-6.62z"
      />
      <path
        fill="#FBBC05"
        d="M3.88 10.78A5.54 5.54 0 0 1 3.58 9c0-.62.11-1.22.29-1.78L.96 4.96A9 9 0 0 0 0 9c0 1.45.35 2.82.96 4.04l2.92-2.26z"
      />
      <path
        fill="#34A853"
        d="M9 18c2.43 0 4.47-.8 5.96-2.18l-2.84-2.2c-.76.53-1.78.9-3.12.9-2.38 0-4.4-1.57-5.12-3.74L.97 13.04C2.45 15.98 5.48 18 9 18z"
      />
    </svg>
  );
}
