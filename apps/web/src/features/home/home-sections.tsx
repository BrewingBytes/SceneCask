"use client";
import Link from "next/link";
import type { ReactNode } from "react";
import { Badge, Button, Poster, Progress } from "../../components/ui/basic";
import { derivedState } from "../library/copy";
import type { Episode, LibraryItem } from "./api";
import { nextCode, upcomingNote, watchedText } from "./copy";

const yearText = (item: LibraryItem) => (item.year === null ? "Year unknown" : String(item.year));

export interface UpNextProps {
  item: LibraryItem;
  busy: boolean;
  onMark: () => void;
}

/** Featured next episode (D02): code only, the title stays hidden until it is watched. */
export function FeaturedShow({ item, busy, onMark, onCatchup }: UpNextProps & { onCatchup: () => void }) {
  const code = nextCode(item)!;
  return (
    <article className="sc-home-featured" aria-labelledby="sc-home-featured-title" aria-busy={busy || undefined}>
      <div className="sc-home-featured-show">
        <Poster src={item.posterUrl} alt="" className="sc-home-featured-poster" />
        <div className="sc-home-featured-info">
          <h2 id="sc-home-featured-title">
            <Link href={`/shows/${item.showId}`}>{item.title}</Link>
          </h2>
          <p>{yearText(item)}</p>
          <p className="sc-home-featured-progress">{watchedText(item.progress)}</p>
          <Progress label={`${item.title} progress`} value={item.progress.percent} />
        </div>
      </div>
      <div className="sc-home-featured-next">
        <p className="sc-home-label">Next episode</p>
        <p className="sc-home-featured-code">
          <span className="sc-code">{code}</span> <span>Released</span>
        </p>
        <p>Title hidden until you watch</p>
        <Button className="sc-home-featured-mark" busy={busy} onClick={onMark}>
          Mark {code} watched
        </Button>
        <div className="sc-home-featured-links">
          <Link href={`/episodes/${item.progress.nextEpisode!.id}`}>View episode</Link>
          <Button variant="quiet" disabled={busy} onClick={onCatchup}>
            Watched more? Catch up
          </Button>
        </div>
      </div>
    </article>
  );
}

interface ShowRowProps {
  item: LibraryItem;
  meta: ReactNode;
  /** The row's own control or state, above the row-wide link. */
  aside: ReactNode;
  busy?: boolean;
  small?: boolean;
  progress?: boolean;
}

/** A show row: the whole row opens the show; codes and counts only, never episode titles. */
function ShowRow({ item, meta, aside, busy = false, small = false, progress = false }: ShowRowProps) {
  return (
    <li className="sc-home-row" aria-busy={busy || undefined}>
      <Poster src={item.posterUrl} alt="" className={small ? "sc-home-row-poster sc-home-row-poster-small" : "sc-home-row-poster"} />
      <div className="sc-home-row-main">
        <Link className="sc-home-row-open" href={`/shows/${item.showId}`}>
          {item.title}
        </Link>
        <span className="sc-home-row-meta">{meta}</span>
        {progress && <Progress label={`${item.title} progress`} value={item.progress.percent} />}
      </div>
      {aside}
    </li>
  );
}

/** Other shows with a released unwatched episode, in the order Home returned them. */
export function UpNextRow({ item, busy, onMark }: UpNextProps) {
  const code = nextCode(item)!;
  const progress = watchedText(item.progress);
  return (
    <ShowRow
      item={item}
      busy={busy}
      progress
      meta={
        <>
          <span className="sc-code">{code}</span> next{progress && ` · ${progress}`}
        </>
      }
      aside={
        <Button variant="secondary" className="sc-home-row-mark" busy={busy} onClick={onMark} aria-label={`Mark ${code} of ${item.title} watched`}>
          Mark watched
        </Button>
      }
    />
  );
}

/** A watching show with nothing released left to watch, and what it is waiting for. */
export function CaughtUpRow({ item, episodes }: { item: LibraryItem; episodes: readonly Episode[] | null }) {
  const state = derivedState(item.progress);
  return (
    <ShowRow
      item={item}
      small
      meta={upcomingNote(item, episodes).text}
      aside={state && <Badge tone={state.tone}>{state.label}</Badge>}
    />
  );
}
