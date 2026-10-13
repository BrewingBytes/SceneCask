import type { Failure, Person, Visibility } from "./api";

/** Product copy for social failures. Never derived from server messages. */
export function socialFailureCopy(failure: Failure): string {
  switch (failure.kind) {
    case "rate_limited":
      return failure.retryAfter
        ? `Too many requests. Try again in ${failure.retryAfter} seconds.`
        : "Too many requests. Wait a moment and try again.";
    case "unavailable":
      return "SceneCask couldn’t be reached. Check your connection and try again.";
    case "invalid":
      return failure.fields.includes("handle") ? HANDLE_NEEDED : "That change wasn’t accepted.";
    case "not_found":
      return "This person isn’t available.";
    default:
      return "That didn’t go through. Refresh the page and try again.";
  }
}

export const HANDLE_NEEDED = "Choose a handle before following people or answering requests.";

/** D08: said before anyone relies on following for discussions. */
export const PARTICIPANTS_NOTE =
  "Discussions are separate: only people who follow each other with a thread’s host can join it, and everyone in a thread can see the other participants.";

export function privacyLede(visibility: Visibility, handle: string | null) {
  const name = handle ? `your name and @${handle}` : "your name";
  return visibility === "private"
    ? `Your account is private. People can find ${name}, but only followers you approve see your library, progress and activity. Following is one-way.`
    : `Your account is public. Any signed-in SceneCask member can see your library and activity. Following is one-way.`;
}

export const visibilityLabel = (visibility: Visibility) => (visibility === "private" ? "Private" : "Public");

/** How the viewer and this person are connected, or null when they aren't. */
export function relationshipText({ relationship: { outgoing, incoming } }: Person): string | null {
  if (outgoing === "approved" && incoming === "approved") return "You follow each other";
  const parts = [
    outgoing === "approved" && "Following",
    outgoing === "pending" && "Requested",
    incoming === "approved" && "Follows you",
    incoming === "pending" && "Asked to follow you",
  ].filter(Boolean);
  return parts.length ? parts.join(" · ") : null;
}

const DATE = new Intl.DateTimeFormat("en", { dateStyle: "medium", timeStyle: "short", timeZone: "UTC" });
const SHORT = new Intl.DateTimeFormat("en", { month: "short", day: "numeric", timeZone: "UTC" });

/** Visible short date plus the full UTC timestamp for assistive technology and tooltips. */
export function when(iso: string) {
  const date = new Date(iso);
  return { short: SHORT.format(date), full: `${DATE.format(date)} UTC` };
}

/** "Saved by Bea", "Saved by Bea and Cal", "Saved by Bea and 2 others". */
export function savedBy(people: Person[]) {
  const [first, second] = people;
  if (!first) return "Saved by people you follow";
  if (people.length === 1) return `Saved by ${first.displayName}`;
  if (people.length === 2) return `Saved by ${first.displayName} and ${second.displayName}`;
  return `Saved by ${first.displayName} and ${people.length - 1} others`;
}

/** Friends tabs, kept in `?tab=` (a notification opens `requests`). Shared with the server route. */
export const FRIENDS_TABS = ["find", "requests", "followers"] as const;
export type FriendsTab = (typeof FRIENDS_TABS)[number];
export const parseFriendsTab = (value: unknown): FriendsTab =>
  FRIENDS_TABS.includes(value as FriendsTab) ? (value as FriendsTab) : "find";
