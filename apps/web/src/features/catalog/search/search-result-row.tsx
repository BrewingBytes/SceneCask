"use client";
import { Button, Poster } from "../../../components/ui/basic";
import type { SearchShow } from "./api";
import type { RowState } from "./use-library-actions";

export interface SearchResultRowProps {
  show: SearchShow;
  state?: RowState;
  onOpen: () => void;
  onAdd: () => void;
}

/** Year, genres and poster disambiguate same-titled shows; the name repeats them for assistive technology. */
export function SearchResultRow({ show, state = {}, onOpen, onAdd }: SearchResultRowProps) {
  const year = show.year === null ? "year unknown" : String(show.year);
  const genres = show.genres.length > 0 ? show.genres.join(", ") : "Genre unknown";
  const name = `${show.title} (${year})`;
  const busy = state.busy;
  return (
    <li className="sc-search-result" aria-busy={busy ? true : undefined}>
      <Poster src={show.posterUrl} alt="" className="sc-search-poster" />
      <button
        type="button"
        className="sc-search-open"
        onClick={onOpen}
        aria-disabled={busy ? true : undefined}
        aria-label={`Open ${name}, TV, ${genres}`}
        data-search-open=""
      >
        <span className="sc-search-title">
          {show.title} <span className="sc-search-year">({year})</span>
        </span>
        <span className="sc-search-meta">{busy === "open" ? "Opening…" : `TV · ${genres}`}</span>
      </button>
      <Button
        variant={state.inLibrary ? "secondary" : "primary"}
        className="sc-search-add"
        busy={busy === "add"}
        aria-disabled={busy === "open" ? true : undefined}
        onClick={state.inLibrary ? onOpen : onAdd}
        aria-label={state.inLibrary ? `${name} is in your library. Open it` : `Add ${name} to Plan to watch`}
      >
        {state.inLibrary ? "✓ In library" : "Add to library"}
      </Button>
    </li>
  );
}
