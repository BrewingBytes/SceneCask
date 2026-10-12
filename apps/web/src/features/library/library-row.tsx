"use client";
import Link from "next/link";
import { useState } from "react";
import { Badge, Poster, Progress } from "../../components/ui/basic";
import { Menu } from "../../components/ui/overlays";
import { STATUS_LABELS, STATUSES } from "../tracking/copy";
import type { LibraryItem, LibraryStatus } from "./api";
import { derivedState, progressText } from "./copy";

export interface LibraryRowProps {
  item: LibraryItem;
  busy: boolean;
  onStatus: (status: LibraryStatus) => void;
  onRemove: () => void;
}

/** Saved show row: opens the show; the menu changes the manual status or removes it. */
export function LibraryRow({ item, busy, onStatus, onRemove }: LibraryRowProps) {
  const [menuOpen, setMenuOpen] = useState(false);
  const year = item.year === null ? "year unknown" : String(item.year);
  const state = derivedState(item.progress);
  const status = STATUS_LABELS[item.status];
  return (
    <li className="sc-library-row" aria-busy={busy || undefined}>
      <Poster src={item.posterUrl} alt="" className="sc-library-poster" />
      <div className="sc-library-main">
        <Link className="sc-library-open" href={`/shows/${item.showId}`}>
          {item.title}{" "}
          <span className="sc-library-year">({year})</span>
        </Link>
        <span className="sc-library-progress">
          {state && <Badge tone={state.tone}>{state.label}</Badge>}
          {item.progress.total > 0 && <span>{progressText(item.progress)}</span>}
        </span>
        <Progress label={`${item.title} progress`} value={item.progress.percent} />
      </div>
      <Menu
        className="sc-library-menu"
        open={menuOpen}
        onOpenChange={setMenuOpen}
        label={`Library actions for ${item.title}`}
        triggerLabel={`${item.title}: ${status}. Change status or remove`}
        triggerVariant="secondary"
        trigger={<span>{status} ▾</span>}
        items={[
          ...STATUSES.map((value) => ({
            id: value,
            label: STATUS_LABELS[value],
            checked: value === item.status,
            disabled: busy,
            onSelect: () => onStatus(value),
          })),
          { id: "remove", label: "Remove from library", tone: "destructive" as const, disabled: busy, onSelect: onRemove },
        ]}
      />
    </li>
  );
}
