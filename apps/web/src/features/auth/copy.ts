import type { AuthFailure } from "./api";

/**
 * Product copy for failures. Never derived from server messages, and never confirms whether
 * an email address has an account.
 */
export function failureCopy(failure: AuthFailure): string {
  switch (failure.kind) {
    case "rate_limited":
      return failure.retryAfter
        ? `Too many attempts. Try again in ${failure.retryAfter} seconds.`
        : "Too many attempts. Wait a minute and try again.";
    case "unavailable":
      return "SceneCask couldn’t be reached. Check your connection and try again.";
    case "invalid":
      return "Check the highlighted fields.";
    default:
      return "That didn’t go through. Refresh the page and try again.";
  }
}

export interface GoogleProblem {
  title: string;
  body: string;
}

/**
 * Copy for the fixed `?error=` codes the Google callback redirects with (C04). Anything
 * unrecognised reads as a failed attempt; the code itself is never displayed.
 */
export function googleProblem(code: string): GoogleProblem {
  switch (code) {
    case "canceled":
      return {
        title: "Google sign-in was canceled",
        body: "Nothing changed. Try Google again, or use your email and password.",
      };
    case "expired":
      return {
        title: "That Google sign-in timed out",
        body: "Start again, or use your email and password.",
      };
    case "unavailable":
      return {
        title: "Google isn’t responding",
        body: "Try again in a moment, or use your email and password.",
      };
    case "email_unverified":
      return {
        title: "Google hasn’t verified that email",
        body: "Verify it with Google first, or use an email and password instead.",
      };
    case "identity_in_use":
      return {
        title: "That Google account is linked elsewhere",
        body: "It already signs in to another SceneCask account. Sign in to that account, or use your email and password.",
      };
    case "reauth_mismatch":
      return {
        title: "That’s a different Google account",
        body: "Use the Google account linked to this SceneCask account, or confirm with your password.",
      };
    default:
      return {
        title: "Google sign-in didn’t finish",
        body: "Try Google again, or use your email and password.",
      };
  }
}
