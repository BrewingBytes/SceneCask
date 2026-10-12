import { episodeCode, plural, releaseLabel } from "../tracking/copy";
import type { Episode, LibraryItem } from "./api";

type Progress = LibraryItem["progress"];

/** "2 of 6 episodes watched · some out of order". Codes and counts only, never episode titles. */
export function watchedText(progress: Progress) {
  const parts = progress.total > 0 ? [`${progress.watched} of ${plural(progress.total, "episode")} watched`] : [];
  if (progress.outOfOrder) parts.push("some out of order");
  if (progress.releaseInfoIncomplete) parts.push("release info incomplete");
  return parts.join(" · ");
}

export const nextCode = (item: LibraryItem) => {
  const next = item.progress.nextEpisode;
  return next ? episodeCode(next.season, next.number) : null;
};

export interface UpcomingNote {
  text: string;
  /** True when the note shows a release date, which may be a UTC-midnight estimate. */
  dated: boolean;
}

/**
 * What a caught-up show is waiting for: the first unwatched regular episode that is not released
 * yet, by code and release label only. Without listings (still loading or unavailable) the note
 * falls back to what the derived state alone says.
 */
export function upcomingNote(item: LibraryItem, episodes: readonly Episode[] | null): UpcomingNote {
  const { progress } = item;
  if (progress.state === "completed") return { text: "Every episode watched", dated: false };
  const waiting = episodes?.find((episode) => episode.season > 0 && !episode.watched && episode.release.state !== "released");
  const label = waiting && releaseLabel(waiting.release);
  if (waiting && label) return { text: `${episodeCode(waiting.season, waiting.number)} · ${label}`, dated: !!waiting.release.date };
  if (progress.releaseInfoIncomplete) return { text: "Release info incomplete", dated: false };
  if (progress.state === "release_unknown") return { text: "Release information unknown", dated: false };
  if (progress.state === "not_yet_available") return { text: "Not released yet", dated: false };
  return { text: "No upcoming episodes announced", dated: false };
}

const kickerFormat = new Intl.DateTimeFormat("en-GB", { weekday: "short", day: "numeric", month: "short" });

/** "Mon 12 Oct · Your shows" (shown uppercase) in the viewer's own time zone. */
export const todayKicker = (now: Date) => `${kickerFormat.format(now).replace(",", "")} · Your shows`;
