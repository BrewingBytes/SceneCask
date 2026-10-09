import type { CatchupEpisode, Failure, LibraryStatus, Release } from "./api";

/** Product copy for tracking failures. Never derived from server messages or provider payloads. */
export function trackingFailureCopy(failure: Failure): string {
  switch (failure.kind) {
    case "auth":
      return "Your session has ended. Sign in again to continue.";
    case "unverified":
      return "Verify your email to track shows.";
    case "rate_limited":
      return failure.retryAfter
        ? `Too many changes. Try again in ${failure.retryAfter} seconds.`
        : "Too many changes. Wait a moment and try again.";
    case "conflict":
      return "Your progress changed somewhere else. The latest version is shown.";
    case "expired":
      return "That’s no longer available.";
    case "not_found":
      return "That show or episode is no longer available.";
    case "invalid":
      return "That change wasn’t accepted.";
    case "network":
    case "unavailable":
      return "SceneCask couldn’t be reached. Try again; the change won’t be applied twice.";
    default:
      return "SceneCask couldn’t be reached. Check your connection and try again.";
  }
}

export const STATUS_LABELS: Record<LibraryStatus, string> = {
  plan_to_watch: "Plan to watch",
  watching: "Watching",
  on_hold: "On hold",
  dropped: "Dropped",
};
export const STATUSES = Object.keys(STATUS_LABELS) as LibraryStatus[];

/** Episode codes never include titles: "S2 E7", or "Special 3" for season 0. */
export function episodeCode(season: number, number: number) {
  return season === 0 ? `Special ${number}` : `S${season} E${number}`;
}

/** "S1 E2–E4, E6; S2 E1". Input is in season/episode order. */
export function rangeText(episodes: readonly CatchupEpisode[]) {
  const bySeason = new Map<number, number[]>();
  for (const { season, episode } of episodes) bySeason.set(season, [...(bySeason.get(season) ?? []), episode]);
  return [...bySeason]
    .map(([season, numbers]) => {
      if (season === 0) return numbers.map((number) => episodeCode(0, number)).join(", ");
      const runs: string[] = [];
      let start = numbers[0];
      for (let index = 1; index <= numbers.length; index += 1) {
        if (numbers[index] === numbers[index - 1] + 1) continue;
        const end = numbers[index - 1];
        runs.push(start === end ? `E${start}` : `E${start}–E${end}`);
        start = numbers[index];
      }
      return `S${season} ${runs.join(", ")}`;
    })
    .join("; ");
}

const dateFormat = new Intl.DateTimeFormat("en", { month: "short", day: "numeric", year: "numeric", timeZone: "UTC" });

/** Release label for a row. Future and unknown labels are kept after an episode is marked watched. */
export function releaseLabel(release: Release): string | null {
  if (release.state === "unknown") return "Release date unknown";
  if (release.state === "future") {
    if (!release.date) return "Not released yet · date not announced";
    const date = dateFormat.format(new Date(`${release.date}T00:00:00Z`));
    return `Airs ${date}${release.estimated ? " (estimated)" : ""}`;
  }
  return null;
}

export const plural = (count: number, one: string, many = `${one}s`) => `${count} ${count === 1 ? one : many}`;
