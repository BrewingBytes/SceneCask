"use client";
import { useEffect, useRef, useState } from "react";
import { Button, Skeleton } from "../../components/ui/basic";
import { Sheet } from "../../components/ui/overlays";
import { outcomeUnknown, type CatchupPreview, type Failure, type Show } from "./api";
import { episodeCode, plural, rangeText, trackingFailureCopy } from "./copy";
import { isWatched, type MutationOutcome, type TrackingClient, type TrackingSnapshot } from "./tracking-client";
import "./catch-up.css";

export interface CatchupSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** "add" also offers saving without progress; "edit" marks through a chosen episode. */
  mode: "add" | "edit";
  show: Show;
  client: TrackingClient;
  snapshot: TrackingSnapshot;
  /** Preselected endpoint, for example from a row's "Mark watched through" action. */
  through?: { id: string; season: number };
}

type Choice = { kind: "none" } | { kind: "through"; id: string } | null;
type PreviewState =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "ready"; preview: CatchupPreview }
  | { status: "error"; failure: Failure };

const NOTE =
  "Catch-up marks released regular episodes only. Specials, unreleased episodes and episodes without a release date are never included; mark those one at a time.";

function exclusions(preview: CatchupPreview) {
  const { alreadyWatched, future, undated, specials } = preview.excluded;
  return [
    { label: "Already watched, left as they are", items: alreadyWatched },
    { label: "Not released yet", items: future },
    { label: "Release date unknown", items: undated },
    { label: "Specials", items: specials },
  ].filter((group) => group.items.length > 0);
}

/**
 * Add/set-progress sheet. Choosing an endpoint requests an explicit server preview; the exact
 * included episodes, counts and exclusions are shown before commit. A stale or expired preview is
 * never submitted silently: a fresh preview replaces it and must be confirmed again.
 */
export function CatchupSheet({ open, onOpenChange, mode, show, client, snapshot, through }: CatchupSheetProps) {
  const regularSeasons = show.seasons.filter((season) => season.number > 0);
  const startSeason = through?.season ?? show.library?.progress.nextEpisode?.season ?? regularSeasons[0]?.number ?? 1;
  const [season, setSeason] = useState(startSeason);
  const [choice, setChoice] = useState<Choice>(through ? { kind: "through", id: through.id } : null);
  const [preview, setPreview] = useState<PreviewState>({ status: "idle" });
  const [stale, setStale] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const request = useRef(0);

  const load = snapshot.seasons[season];
  const episodes = load?.episodes ?? [];

  useEffect(() => {
    if (open) void client.loadSeason(season);
  }, [client, open, season]);

  async function requestPreview(id: string, afterStale = false) {
    const token = ++request.current;
    setPreview({ status: "loading" });
    const result = await client.previewCatchup(id);
    if (token !== request.current) return;
    setPreview(result.ok ? { status: "ready", preview: result.data } : { status: "error", failure: result.failure });
    setStale(afterStale && result.ok);
  }

  // The preselected endpoint is previewed once when the sheet opens with it.
  const initial = useRef(through?.id);
  useEffect(() => {
    if (!open || !initial.current) return;
    const id = initial.current;
    initial.current = undefined;
    void requestPreview(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- runs once per opening.
  }, [open]);

  function choose(next: Choice) {
    setChoice(next);
    setError(null);
    setStale(false);
    if (next?.kind === "through") void requestPreview(next.id);
    else {
      request.current += 1;
      setPreview({ status: "idle" });
    }
  }

  function settle(result: MutationOutcome, retryStale: () => void) {
    if (result.ok) {
      onOpenChange(false);
      return;
    }
    const { kind } = result.failure;
    if (kind === "busy") return;
    if (kind === "conflict" || kind === "expired") {
      retryStale();
      return;
    }
    setError(
      outcomeUnknown(result.failure as Failure)
        ? "Couldn’t confirm the save. Try again; it won’t be applied twice."
        : `Couldn’t save. Nothing was changed. ${trackingFailureCopy(result.failure as Failure)}`,
    );
  }

  async function confirm() {
    if (saving) return;
    setError(null);
    if (choice?.kind === "none") {
      setSaving(true);
      const result = await client.addToLibrary(true);
      setSaving(false);
      settle(result, () => setError("Your library changed somewhere else. Check the show and try again."));
      return;
    }
    if (choice?.kind !== "through" || preview.status !== "ready") return;
    const current = preview.preview;
    // An expired preview is refreshed for reconfirmation instead of being sent.
    if (Date.parse(current.expiresAt) <= Date.now()) {
      void requestPreview(choice.id, true);
      return;
    }
    setSaving(true);
    const result = current.count === 0 && mode === "add" ? await client.addToLibrary(true) : await client.commitCatchup(current);
    setSaving(false);
    settle(result, () => void requestPreview(choice.id, true));
  }

  const ready = preview.status === "ready" ? preview.preview : null;
  const endpointCode = ready ? episodeCode(ready.through.season, ready.through.episode) : "";
  const label =
    choice?.kind === "none"
      ? "Add to library"
      : preview.status === "loading"
        ? "Checking…"
        : ready
          ? ready.count > 0
            ? `Mark ${ready.count} watched`
            : mode === "add"
              ? "Add to library"
              : "Nothing to mark"
          : mode === "add"
            ? "Add to library"
            : "Mark watched";
  const canConfirm = choice?.kind === "none" || (ready !== null && (ready.count > 0 || mode === "add"));

  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      busy={saving}
      kicker={mode === "add" ? "Add to library" : "Mark watched through an episode"}
      title={mode === "add" ? `Where are you in ${show.title}?` : `Catch up on ${show.title}`}
      footer={
        <div className="sc-catchup-actions">
          <Button variant="secondary" onClick={() => onOpenChange(false)} disabled={saving}>
            Cancel
          </Button>
          <Button variant="primary" busy={saving} disabled={!canConfirm && !saving} onClick={() => void confirm()}>
            {saving ? "Saving…" : error ? "Try again" : label}
          </Button>
        </div>
      }
    >
      <div className="sc-catchup">
        {mode === "add" && (
          <button
            type="button"
            className="sc-catchup-option"
            aria-pressed={choice?.kind === "none"}
            onClick={() => choose({ kind: "none" })}
          >
            Haven’t started yet
            <span>Save to Plan to watch without marking episodes.</span>
          </button>
        )}
        <div role="group" aria-labelledby="sc-catchup-pick">
          <p id="sc-catchup-pick" className="sc-catchup-label">
            Last episode you’ve watched
          </p>
          {regularSeasons.length > 1 && (
            <div className="sc-catchup-chips" role="group" aria-label="Season">
              {regularSeasons.map(({ number }) => (
                <button
                  key={number}
                  type="button"
                  className="sc-chip"
                  aria-pressed={number === season}
                  onClick={() => setSeason(number)}
                >
                  Season {number}
                </button>
              ))}
            </div>
          )}
          {load?.status === "error" ? (
            <p className="sc-catchup-inline" role="alert">
              Episodes didn’t load. {trackingFailureCopy(load.failure!)}{" "}
              <Button variant="quiet" onClick={() => void client.loadSeason(season)}>
                Retry
              </Button>
            </p>
          ) : load?.status !== "ready" ? (
            <Skeleton label={`Loading season ${season} episodes`} height={44} />
          ) : (
            <div className="sc-catchup-chips" role="group" aria-label={`Season ${season} episodes`}>
              {episodes.map((episode) => {
                const watched = isWatched(snapshot.overlay, episode);
                const code = episodeCode(episode.season, episode.number);
                const release =
                  episode.release.state === "future" ? ", not released yet" : episode.release.state === "unknown" ? ", release date unknown" : "";
                return (
                  <button
                    key={episode.id}
                    type="button"
                    className="sc-chip sc-chip-episode"
                    data-watched={watched || undefined}
                    aria-pressed={choice?.kind === "through" && choice.id === episode.id}
                    aria-label={`${code}${watched ? ", already watched" : ""}${release}`}
                    onClick={() => choose({ kind: "through", id: episode.id })}
                  >
                    E{episode.number}
                  </button>
                );
              })}
            </div>
          )}
        </div>
        <div className="sc-catchup-summary" aria-live="polite">
          {stale && (
            <p className="sc-catchup-stale" role="alert">
              Episodes changed since you reviewed this. Check the updated list, then confirm again.
            </p>
          )}
          {choice?.kind === "none" ? (
            <p className="sc-catchup-count">Saved as Plan to watch. No episodes are marked.</p>
          ) : preview.status === "loading" ? (
            <p className="sc-catchup-count">Checking which episodes would change…</p>
          ) : preview.status === "error" ? (
            <p className="sc-catchup-inline" role="alert">
              Couldn’t check those episodes. {trackingFailureCopy(preview.failure)}{" "}
              {choice?.kind === "through" && (
                <Button variant="quiet" onClick={() => void requestPreview(choice.id)}>
                  Retry
                </Button>
              )}
            </p>
          ) : ready ? (
            <>
              <p className="sc-catchup-count">
                {ready.count > 0
                  ? `Marks ${plural(ready.count, "episode")} watched: ${rangeText(ready.included)}.`
                  : `Everything released through ${endpointCode} is already marked.`}
              </p>
              {exclusions(ready).length > 0 && (
                <ul className="sc-catchup-excluded" aria-label="Not included">
                  {exclusions(ready).map((group) => (
                    <li key={group.label}>
                      <strong>{group.label}:</strong> {rangeText(group.items)}
                    </li>
                  ))}
                </ul>
              )}
            </>
          ) : (
            <p className="sc-catchup-count">Pick the last episode you’ve watched.</p>
          )}
          <p className="sc-catchup-note">{NOTE}</p>
        </div>
        <p className="sc-catchup-error" role="alert">
          {error}
        </p>
      </div>
    </Sheet>
  );
}
