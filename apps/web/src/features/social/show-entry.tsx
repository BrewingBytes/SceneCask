import Link from "next/link";
import type { ReactNode } from "react";
import { Poster } from "../../components/ui/basic";

export interface ShowEntryProps {
  show: { id: string; title: string; year: number | null; posterUrl: string | null };
  /** What this list says about the show: someone's status and progress, or who saved it. */
  children: ReactNode;
}

/** A show in a social list: poster, title and year linking to the show, then the list's facts. */
export function ShowEntry({ show, children }: ShowEntryProps) {
  return (
    <div className="sc-social-show">
      <Poster src={show.posterUrl} alt="" className="sc-social-poster" />
      <div className="sc-social-show-main">
        <Link href={`/shows/${show.id}`} className="sc-social-show-title">
          {show.title} <span className="sc-social-handle">({show.year ?? "year unknown"})</span>
        </Link>
        {children}
      </div>
    </div>
  );
}
