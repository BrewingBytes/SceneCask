"use client";
import Link from "next/link";
import { useEffect, useState } from "react";
import { Badge, Button, EmptyState, ErrorState, Poster, Progress, Skeleton, Tabs } from "../../../components/ui/basic";
import { ConfirmDialog, Menu, ToastViewport, useToastQueue } from "../../../components/ui/overlays";
import {
  CatchupSheet,
  episodeCode,
  isWatched,
  STATUS_LABELS,
  STATUSES,
  trackingFailureCopy,
  useTracking,
  type TrackingClient,
  type TrackingSnapshot,
} from "../../tracking";
import type { Failure, LibraryItem, Show } from "../../tracking/api";
import { EpisodeRow } from "./episode-row";
import "./show.css";

export interface ShowDetailProps {
  showId: string;
  /** Season restored from the URL (?season=) on load, reload and Back/Forward. */
  initialSeason?: number;
}

type SheetState = { open: boolean; mode: "add" | "edit"; through?: { id: string; season: number }; session: number };

const SEASON_NOTE = "Release dates come from TV listings. Without a reliable time zone they use midnight UTC, so streaming services may differ.";
const SPECIALS_NOTE = "Specials are tracked separately. They don’t count toward progress, next episode, catch-up or completion.";

function stateText(library: LibraryItem) {
  const { progress } = library;
  switch (progress.state) {
    case "completed":
      return "Completed";
    case "caught_up":
      return "Caught up";
    case "not_yet_available":
      return "Not yet available";
    case "release_unknown":
      return "Release dates unknown";
    default:
      return progress.nextEpisode ? `Next: ${episodeCode(progress.nextEpisode.season, progress.nextEpisode.number)}` : "In progress";
  }
}

function meta(show: Show) {
  const regular = show.seasons.filter((season) => season.number > 0).length;
  return [
    show.year ?? "Year unknown",
    show.genres.length ? show.genres.join(", ") : null,
    regular ? `${regular} season${regular === 1 ? "" : "s"}` : null,
    show.status === "ended" ? "Ended" : show.status === "canceled" ? "Canceled" : null,
  ]
    .filter(Boolean)
    .join(" · ");
}

function hasHistory(snapshot: TrackingSnapshot, show: Show) {
  if (snapshot.overlay.allUnwatched) return false;
  if ((show.library?.progress.watched ?? 0) > 0) return true;
  return Object.values(snapshot.seasons).some((load) => load.episodes.some((episode) => isWatched(snapshot.overlay, episode)));
}

function defaultSeason(show: Show, requested?: number) {
  if (requested !== undefined && show.seasons.some((season) => season.number === requested)) return requested;
  const next = show.library?.progress.nextEpisode?.season;
  if (next !== undefined) return next;
  return show.seasons.find((season) => season.number > 0)?.number ?? show.seasons[0]?.number ?? 1;
}

function LoadFailure({ failure, showId, onRetry }: { failure: Failure; showId: string; onRetry: () => void }) {
  if (failure.kind === "auth")
    return (
      <EmptyState
        title="Sign in to see this show"
        action={
          <Link className="sc-button sc-button-primary" href={`/auth/signin?returnTo=${encodeURIComponent(`/shows/${showId}`)}`}>
            Sign in
          </Link>
        }
      >
        Your session has ended. Your progress is saved to your account.
      </EmptyState>
    );
  if (failure.kind === "unverified")
    return (
      <EmptyState title="Verify your email to track shows" action={<Link className="sc-button sc-button-primary" href="/auth/verify">Verify email</Link>}>
        Check your inbox for the verification link.
      </EmptyState>
    );
  if (failure.kind === "not_found" || failure.kind === "invalid")
    return (
      <EmptyState title="Show not found" action={<Link className="sc-button sc-button-secondary" href="/discover">Search shows</Link>}>
        This show isn’t available. Search for it again from Discover.
      </EmptyState>
    );
  return (
    <ErrorState title="This show didn’t load" onRetry={onRetry}>
      {trackingFailureCopy(failure)}
    </ErrorState>
  );
}

function LibraryPanel({
  show,
  snapshot,
  client,
  onAdd,
  onCatchup,
  onErase,
}: {
  show: Show;
  snapshot: TrackingSnapshot;
  client: TrackingClient;
  onAdd: () => void;
  onCatchup: () => void;
  onErase: () => void;
}) {
  const [statusOpen, setStatusOpen] = useState(false);
  const library = show.library;
  const saved = snapshot.overlay.saved ?? library?.saved ?? false;
  const history = hasHistory(snapshot, show);
  const busy = snapshot.busy.has("library");

  if (!saved || !library)
    return (
      <div className="sc-show-add">
        <Button variant="primary" busy={busy} onClick={history ? () => void client.addToLibrary() : onAdd}>
          {history ? "Add back to library" : "Add to library"}
        </Button>
        {history && library && (
          <p className="sc-show-kept">
            You removed this show earlier. Your history is kept: {library.progress.watched} of {library.progress.total} episodes watched.{" "}
            <button type="button" className="sc-link-destructive" onClick={onErase}>
              Erase history
            </button>
          </p>
        )}
      </div>
    );

  const status = snapshot.overlay.status ?? library.status;
  const next = library.progress.nextEpisode;
  const { progress } = library;
  return (
    <div className="sc-show-library">
      <div className="sc-show-progress-head">
        <span className="sc-show-progress-count">
          {progress.watched} of {progress.total} episodes watched
        </span>
        <span className="sc-show-state">{stateText(library)}</span>
      </div>
      <Progress label="Series progress" value={progress.percent} />
      {progress.outOfOrder && <p className="sc-show-hint">Some episodes are marked out of order. Earlier ones stay unmarked until you mark them.</p>}
      <div className="sc-actions sc-show-actions">
        <Menu
          open={statusOpen}
          onOpenChange={setStatusOpen}
          label="Library status"
          triggerLabel={`Library status: ${STATUS_LABELS[status]}`}
          triggerVariant="secondary"
          trigger={<span>{STATUS_LABELS[status]} ▾</span>}
          align="start"
          items={STATUSES.map((value) => ({
            id: value,
            label: STATUS_LABELS[value],
            checked: value === status,
            disabled: busy,
            onSelect: () => void client.setStatus(value),
          }))}
        />
        {next && (
          <Button variant="primary" busy={snapshot.busy.has(`episode:${next.id}`)} onClick={() => void client.markWatchedById(next.id)}>
            Mark {episodeCode(next.season, next.number)} watched
          </Button>
        )}
        <Button variant="secondary" onClick={onCatchup}>
          Catch up…
        </Button>
      </div>
      <div className="sc-show-manage">
        <Button variant="quiet" busy={busy} onClick={() => void client.removeFromLibrary()}>
          Remove from library
        </Button>
        {history && (
          <Button variant="quiet" className="sc-link-destructive" onClick={onErase}>
            Erase watch history…
          </Button>
        )}
      </div>
    </div>
  );
}

function SeasonPanel({
  season,
  snapshot,
  client,
  onThrough,
}: {
  season: number;
  snapshot: TrackingSnapshot;
  client: TrackingClient;
  onThrough: (id: string) => void;
}) {
  const load = snapshot.seasons[season];
  const name = season === 0 ? "Specials" : `Season ${season}`;
  return (
    <>
      <p className="sc-show-note">{season === 0 ? SPECIALS_NOTE : SEASON_NOTE}</p>
      {!load || load.status === "loading" ? (
        <Skeleton label={`Loading ${name} episodes`} height={64} />
      ) : load.status === "error" ? (
        <ErrorState title={`${name} episodes didn’t load`} onRetry={() => void client.loadSeason(season)}>
          {trackingFailureCopy(load.failure!)}
        </ErrorState>
      ) : load.episodes.length === 0 ? (
        <EmptyState title="No episodes listed yet">Episodes appear here once TV listings include them.</EmptyState>
      ) : (
        <ul className="sc-episodes" aria-label={`${name} episodes`}>
          {load.episodes.map((episode) => (
            <EpisodeRow
              key={episode.id}
              episode={episode}
              overlay={snapshot.overlay}
              busy={snapshot.busy.has(`episode:${episode.id}`)}
              onToggle={() => void client.setWatched(episode, !isWatched(snapshot.overlay, episode))}
              onThrough={season > 0 ? () => onThrough(episode.id) : undefined}
            />
          ))}
        </ul>
      )}
    </>
  );
}

/** D02 show detail: poster/info, library status and progress, seasons/specials and episode rows. */
export function ShowDetail({ showId, initialSeason }: ShowDetailProps) {
  const { toasts, show: showToast, update, dismiss } = useToastQueue();
  const [controls] = useState(() => ({ show: showToast, update, dismiss }));
  const { client, snapshot } = useTracking(showId, controls);
  const [season, setSeason] = useState<number | undefined>(initialSeason);
  const [sheet, setSheet] = useState<SheetState>({ open: false, mode: "edit", session: 0 });
  const [erase, setErase] = useState<{ open: boolean; busy: boolean; error: string | null }>({ open: false, busy: false, error: null });

  const show = snapshot.show.status === "ready" ? snapshot.show.show : null;
  const selected = show ? defaultSeason(show, season) : undefined;
  // Pin the resolved season so a progress change never moves the open tab.
  if (selected !== undefined && selected !== season) setSeason(selected);

  useEffect(() => {
    if (selected === undefined) return;
    void client.loadSeason(selected);
    const url = new URL(window.location.href);
    url.searchParams.set("season", String(selected));
    if (url.href !== window.location.href) window.history.replaceState(window.history.state, "", url);
  }, [client, selected]);

  const openSheet = (mode: "add" | "edit", through?: { id: string; season: number }) =>
    setSheet((current) => ({ open: true, mode, through, session: current.session + 1 }));

  async function confirmErase() {
    setErase({ open: true, busy: true, error: null });
    const result = await client.eraseHistory();
    if (result.ok) {
      setErase({ open: false, busy: false, error: null });
      return;
    }
    if (result.failure.kind === "busy") return;
    const failure = result.failure as Failure;
    setErase({
      open: true,
      busy: false,
      error:
        failure.kind === "conflict"
          ? "Your progress changed since you opened this. Check the latest episodes, then try again."
          : `Couldn’t erase history. Nothing was changed. ${trackingFailureCopy(failure)}`,
    });
  }

  return (
    <section className="sc-show" aria-labelledby={show ? "sc-show-title" : undefined} aria-busy={!show || undefined}>
      {snapshot.show.status === "loading" && (
        <div className="sc-show-hero">
          <div className="sc-show-poster">
            <Skeleton decorative height="100%" />
          </div>
          <div className="sc-show-info">
            <Skeleton label="Loading show" width="60%" height={40} />
            <Skeleton decorative width="40%" height={16} />
            <Skeleton decorative width="90%" height={64} />
          </div>
        </div>
      )}
      {snapshot.show.status === "error" && (
        <LoadFailure failure={snapshot.show.failure} showId={showId} onRetry={() => void client.loadShow()} />
      )}
      {show && selected !== undefined && (
        <>
          <div className="sc-show-hero">
            <Poster className="sc-show-poster" src={show.posterUrl} alt={`${show.title} poster`} />
            <div className="sc-show-info">
              <h1 id="sc-show-title">{show.title}</h1>
              <p className="sc-show-meta">{meta(show)}</p>
              {show.synopsis && <p className="sc-reading sc-show-synopsis">{show.synopsis}</p>}
              {show.metadataStale && (
                <p className="sc-show-hint">
                  <Badge>Listings may be out of date</Badge> We couldn’t refresh this show recently. Your progress is safe.
                </p>
              )}
              <LibraryPanel
                show={show}
                snapshot={snapshot}
                client={client}
                onAdd={() => openSheet("add")}
                onCatchup={() => openSheet("edit")}
                onErase={() => setErase({ open: true, busy: false, error: null })}
              />
            </div>
          </div>
          {(show.releaseInfoIncomplete || show.library?.progress.releaseInfoIncomplete) && (
            <p className="sc-show-incomplete" role="note">
              <strong>Release info incomplete.</strong> Some episodes have no confirmed release date. They aren’t suggested as next or
              included in catch-up, but you can mark them watched yourself.
            </p>
          )}
          {show.seasons.length === 0 ? (
            <EmptyState title="No episodes listed yet">Episodes appear here once TV listings include them.</EmptyState>
          ) : (
            <Tabs
              label="Seasons"
              value={String(selected)}
              onChange={(value) => setSeason(Number(value))}
              tabs={[...show.seasons]
                .sort((a, b) => (a.number === 0 ? 1 : b.number === 0 ? -1 : a.number - b.number))
                .map(({ number }) => ({
                  value: String(number),
                  label: number === 0 ? "Specials" : `Season ${number}`,
                  content: (
                    <SeasonPanel
                      season={number}
                      snapshot={snapshot}
                      client={client}
                      onThrough={(id) => openSheet("edit", { id, season: number })}
                    />
                  ),
                }))}
            />
          )}
          <CatchupSheet
            key={sheet.session}
            open={sheet.open}
            onOpenChange={(open) => setSheet((current) => ({ ...current, open }))}
            mode={sheet.mode}
            through={sheet.through}
            show={show}
            client={client}
            snapshot={snapshot}
          />
          <ConfirmDialog
            open={erase.open}
            onOpenChange={(open) => setErase((current) => ({ ...current, open, error: open ? current.error : null }))}
            title={`Erase watch history for ${show.title}?`}
            description="This unmarks every watched episode, including specials. Removing a show from your library doesn’t do this; erasing is separate. Your library status isn’t changed, and you can undo right after."
            confirmLabel={erase.error ? "Try again" : "Erase history"}
            destructive
            busy={erase.busy}
            error={erase.error}
            onConfirm={() => void confirmErase()}
          />
        </>
      )}
      <ToastViewport toasts={toasts} onDismiss={dismiss} />
    </section>
  );
}
