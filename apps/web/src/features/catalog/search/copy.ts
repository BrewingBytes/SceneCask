import type { Failure } from "./api";

/** Product copy for failures. Never derived from server messages or provider payloads. */
export function failureCopy(failure: Failure): string {
  switch (failure.kind) {
    case "auth":
      return "Your session has ended. Sign in again to continue.";
    case "unverified":
      return "Verify your email to search and save shows.";
    case "rate_limited":
      return failure.retryAfter
        ? `Too many requests. Try again in ${failure.retryAfter} seconds.`
        : "Too many requests. Wait a moment and try again.";
    case "provider":
      return "Our show listings provider didn’t answer. Try again in a moment.";
    case "conflict":
      return "Your library changed somewhere else. Try again to use the latest version.";
    case "not_found":
      return "That show is no longer available.";
    case "invalid":
      return "That request wasn’t accepted. Check the search and try again.";
    default:
      return "SceneCask couldn’t be reached. Check your connection and try again.";
  }
}

export function resultsHint(count: number, hasMore: boolean) {
  if (count === 1 && !hasMore) return "1 show found";
  return `${count} shows${hasMore ? " so far" : ""} · check the year and poster to pick the right one`;
}
