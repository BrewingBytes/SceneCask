import { releaseLabel, type EpisodeView } from "../../tracking";

const dateFormat = new Intl.DateTimeFormat("en", { month: "short", day: "numeric", year: "numeric", timeZone: "UTC" });

/** Release line from the catalog. Marking an episode watched never changes it. */
export function airText(episode: EpisodeView) {
  const future = releaseLabel(episode.release);
  if (future) return future;
  const { date, estimated } = episode.release;
  const released = date ? `Released ${dateFormat.format(new Date(`${date}T00:00:00Z`))}${estimated ? " (estimated)" : ""}` : "Released";
  return episode.season === 0 ? `Special · ${released}` : `Season ${episode.season}, episode ${episode.number} · ${released}`;
}

export function badgeText(episode: EpisodeView, watched: boolean) {
  if (watched) return "Watched";
  if (episode.release.state === "future") return "Not released";
  if (episode.release.state === "unknown") return "Date unknown";
  return "Not watched";
}

export const FUTURE_NOTE =
  "Not released yet. If you’ve already seen it, you can still mark it watched. It isn’t suggested as next or included in catch-up.";
export const UNDATED_NOTE =
  "No confirmed release date. This episode isn’t suggested as next or included in catch-up. You can still mark it watched.";
export const SPECIAL_NOTE = "Specials are tracked separately and don’t count toward progress, next episode or catch-up.";

export const HIDDEN_TEXT = "Title, summary and still are hidden until you watch this episode.";

export const revealTitle = (code: string) => `Reveal details for ${code}?`;
export const revealBody = (code: string, show: string) =>
  `You’ll see the title, summary and still for ${code} of ${show}. This lasts for this sign-in session. Nothing is marked watched, and later episodes stay hidden.`;
export const revealedNotice = (code: string) =>
  `Showing details for ${code} for this sign-in session. Nothing is marked watched, and later episodes stay hidden.`;
export const hiddenNotice = (code: string) => `Details for ${code} are hidden again.`;
export const expiredNotice = (code: string) => `Your reveal for ${code} ended, so its details are hidden again.`;
