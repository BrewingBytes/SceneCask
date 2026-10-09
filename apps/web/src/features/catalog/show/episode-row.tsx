"use client";
import Link from "next/link";
import { useState } from "react";
import { Menu } from "../../../components/ui/overlays";
import {
  episodeCode,
  isWatched,
  releaseLabel,
  visibleDetails,
  type EpisodeView,
  type Overlay,
} from "../../tracking";

export interface EpisodeRowProps {
  episode: EpisodeView;
  overlay: Overlay;
  busy: boolean;
  onToggle: () => void;
  /** Regular episodes only: opens the catch-up sheet with this endpoint. */
  onThrough?: () => void;
}

/**
 * One episode. The toggle marks only this episode, including future and undated ones; the
 * release label stays after marking. Titles appear only when the backend returned details, and
 * accessible names use the episode code, never protected text.
 */
export function EpisodeRow({ episode, overlay, busy, onToggle, onThrough }: EpisodeRowProps) {
  const [menuOpen, setMenuOpen] = useState(false);
  const watched = isWatched(overlay, episode);
  const details = visibleDetails(overlay, episode);
  const code = episodeCode(episode.season, episode.number);
  const release = releaseLabel(episode.release);
  const status = [release, watched ? "Watched" : null].filter(Boolean).join(" · ");
  const title = details?.title ?? (episode.release.state === "future" ? "Not released yet" : "Title hidden");

  return (
    <li className="sc-episode" data-release={episode.release.state}>
      <button
        type="button"
        className="sc-episode-toggle"
        aria-pressed={watched}
        aria-busy={busy || undefined}
        aria-label={`${code} watched`}
        onClick={() => {
          if (!busy) onToggle();
        }}
      >
        <span aria-hidden="true">{watched ? "✓" : episode.release.state === "unknown" ? "?" : ""}</span>
      </button>
      <Link className="sc-episode-open" href={`/episodes/${episode.id}`} aria-label={`Open ${code}${status ? `, ${status}` : ""}`}>
        <span className="sc-code">{code}</span>
        <span className="sc-episode-title" data-hidden={!details?.title || undefined}>
          {title}
        </span>
        {status && <span className="sc-episode-status">{status}</span>}
      </Link>
      <Menu
        open={menuOpen}
        onOpenChange={setMenuOpen}
        label={`More actions for ${code}`}
        trigger={<span aria-hidden="true">⋯</span>}
        items={[
          ...(onThrough ? [{ id: "through", label: `Mark watched through ${code}…`, onSelect: onThrough }] : []),
          { id: "open", label: "View episode", href: `/episodes/${episode.id}` },
        ]}
      />
    </li>
  );
}
