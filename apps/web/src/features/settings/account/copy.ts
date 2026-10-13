import type { Failure } from "./api";

/** Product copy for settings failures. Never derived from server messages. */
export function settingsFailureCopy(failure: Failure): string {
  switch (failure.kind) {
    case "rate_limited":
      return failure.retryAfter
        ? `Too many changes. Try again in ${failure.retryAfter} seconds.`
        : "Too many changes. Wait a moment and try again.";
    case "unavailable":
      return "SceneCask couldn’t be reached. Check your connection and try again.";
    case "invalid":
      return "That change wasn’t accepted.";
    default:
      return "That didn’t go through. Refresh the page and try again.";
  }
}

/** D04 public-switch warning, shown before the write. */
export const PUBLIC_WARNING =
  "Any signed-in SceneCask member can see your library and activity. Discussions still require mutual follows.";

export const LAST_METHOD_EXPLANATION =
  "Google is the only way you sign in, so it can’t be unlinked. Set a password first, then unlink Google.";

/** Where a Google flow started from Settings returns, and which pending action it resumes. */
export type GoogleReturn = "link" | "unlink";
export const googleReturnTo = (action: GoogleReturn) => `/settings?google=${action}`;
