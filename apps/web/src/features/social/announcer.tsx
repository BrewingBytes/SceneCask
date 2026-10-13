"use client";
import Link from "next/link";
import { useCallback, useState, type ReactNode } from "react";
import { endExpiredSession } from "../auth";
import { needsHandle, type Failure } from "./api";
import { HANDLE_NEEDED, socialFailureCopy } from "./copy";

export interface Announce {
  /** A polite success message; clears any earlier problem. */
  done: (message: string) => void;
  /** A failure as an alert that stays until the next attempt. A 401 ends the session instead. */
  failed: (lead: string, failure: Failure) => void;
  clear: () => void;
}

/** A failure's copy; a missing handle links to where it's chosen (D03). */
export function FailureCopy({ failure }: { failure: Failure }) {
  if (!needsHandle(failure)) return <>{socialFailureCopy(failure)}</>;
  return (
    <>
      {HANDLE_NEEDED} <Link href="/onboarding/profile">Choose a handle</Link>
    </>
  );
}

/**
 * One view's announcements. Both regions are mounted up front so later messages are read:
 * successes politely, failures as alerts.
 */
export function useAnnouncer() {
  const [status, setStatus] = useState("");
  const [problem, setProblem] = useState<ReactNode>(null);

  const announce: Announce = {
    done: useCallback((message: string) => {
      setProblem(null);
      setStatus(message);
    }, []),
    failed: useCallback((lead: string, failure: Failure) => {
      if (failure.kind === "auth") return endExpiredSession();
      setStatus("");
      setProblem(
        <>
          {lead} <FailureCopy failure={failure} />
        </>,
      );
    }, []),
    clear: useCallback(() => {
      setStatus("");
      setProblem(null);
    }, []),
  };

  const regions = (
    <>
      <p className="sc-social-status" role="status">
        {status}
      </p>
      <div className="sc-social-problem" role="alert">
        {problem}
      </div>
    </>
  );
  return { announce, regions };
}
