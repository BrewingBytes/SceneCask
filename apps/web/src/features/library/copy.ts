import { episodeCode, plural, STATUS_LABELS, STATUSES } from "../tracking/copy";
import type { LibraryItem, LibraryQuery, LibraryStatus } from "./api";

type Progress = LibraryItem["progress"];

/**
 * Derived state as the backend computed it. Completed is never inferred from the show's
 * ended/canceled status here: an incomplete catalog stays Caught up.
 */
export function derivedState(progress: Progress): { label: string; tone: "accent" | "neutral" } | null {
  switch (progress.state) {
    case "completed":
      return { label: "Completed", tone: "accent" };
    case "caught_up":
      return { label: "Caught up", tone: "neutral" };
    case "not_yet_available":
      return { label: "Not yet available", tone: "neutral" };
    case "release_unknown":
      return { label: "Release dates unknown", tone: "neutral" };
    default:
      return null;
  }
}

/** "2 of 6 watched · Next S1 E2 · some out of order". Codes only, never episode titles. */
export function progressText(progress: Progress) {
  // With no regular episodes listed yet, the derived state alone says it.
  const parts = progress.total > 0 ? [`${progress.watched} of ${plural(progress.total, "episode")} watched`] : [];
  if (progress.state === "in_progress" && progress.nextEpisode)
    parts.push(`Next ${episodeCode(progress.nextEpisode.season, progress.nextEpisode.number)}`);
  if (progress.outOfOrder) parts.push("some out of order");
  if (progress.releaseInfoIncomplete) parts.push("release info incomplete");
  return parts.join(" · ");
}

export const showCount = (count: number) => plural(count, "show");

export function noMatchText(query: LibraryQuery) {
  const term = query.q.trim();
  const status = query.status ? STATUS_LABELS[query.status] : null;
  if (term && status) return `No ${status} shows match “${term}”.`;
  if (term) return `No shows in your library match “${term}”.`;
  if (status) return `No shows are ${status}.`;
  return "No saved shows are loaded.";
}

/** A `?status=` value, or null for All when absent or not a manual status. */
export const parseStatus = (value: string | null | undefined): LibraryStatus | null =>
  STATUSES.includes(value as LibraryStatus) ? (value as LibraryStatus) : null;
