"use client";
import Link from "next/link";
import { useEffect, useRef, useState, useSyncExternalStore, type ReactNode, type RefObject } from "react";
import { Badge, Button, EmptyState, ErrorState, Poster, Skeleton } from "../../../components/ui/basic";
import { ConfirmDialog, ToastViewport, useToastQueue } from "../../../components/ui/overlays";
import {
  CatchupSheet,
  createTrackingClient,
  episodeCode,
  isWatched,
  trackingFailureCopy,
  visibleDetails,
  type EpisodeView,
  type ToastControls,
} from "../../tracking";
import { createTrackingApi, type Failure } from "../../tracking/api";
import {
  airText,
  badgeText,
  expiredNotice,
  FUTURE_NOTE,
  HIDDEN_TEXT,
  hiddenNotice,
  revealBody,
  revealedNotice,
  revealTitle,
  SPECIAL_NOTE,
  UNDATED_NOTE,
} from "./copy";
import { useEpisode, type EpisodeSnapshot, type EpisodeStore } from "./use-episode";
import "./episode.css";

export interface EpisodeDetailProps {
  episodeId: string;
  /**
   * Reserved for hosted discussions (beta). Alpha routes pass nothing, so nothing renders and
   * nothing is requested.
   */
  discussions?: ReactNode;
}

const noticeText = (notice: EpisodeSnapshot["notice"], code: string) =>
  notice === "hidden" ? hiddenNotice(code) : notice === "expired" ? expiredNotice(code) : "";

function revealFailureCopy(failure: Failure) {
  if (failure.kind === "not_found") return "This episode is no longer available.";
  if (failure.kind === "rate_limited") return trackingFailureCopy(failure);
  return "Couldn’t reveal details. Nothing was changed. Try again.";
}

/** Product copy for a failed episode read; server messages are never shown. */
function failureView(failure: Failure, episodeId: string) {
  const link = (href: string, label: string, primary = true) => (
    <Link className={`sc-button sc-button-${primary ? "primary" : "secondary"}`} href={href}>
      {label}
    </Link>
  );
  switch (failure.kind) {
    case "auth":
      return {
        title: "Your sign-in session ended",
        body: "Episode details you revealed are hidden again. Sign in to continue; your progress is saved to your account.",
        action: link(`/auth/signin?returnTo=${encodeURIComponent(`/episodes/${episodeId}`)}`, "Sign in"),
      };
    case "unverified":
      return { title: "Verify your email to track shows", body: "Check your inbox for the verification link.", action: link("/auth/verify", "Verify email") };
    case "not_found":
    case "invalid":
      return {
        title: "Episode not found",
        body: "This episode isn’t available. Find the show again from Discover.",
        action: link("/discover", "Search shows", false),
      };
    default:
      return null;
  }
}

function LoadFailure({ failure, episodeId, onRetry }: { failure: Failure; episodeId: string; onRetry: () => void }) {
  const view = failureView(failure, episodeId);
  if (!view)
    return (
      <ErrorState title="This episode didn’t load" onRetry={onRetry}>
        {trackingFailureCopy(failure)}
      </ErrorState>
    );
  return (
    <EmptyState title={view.title} action={view.action}>
      {view.body}
    </EmptyState>
  );
}

interface ScreenProps {
  showId: string;
  episode: EpisodeView;
  notice: EpisodeSnapshot["notice"];
  readFailure: Failure | null;
  store: EpisodeStore;
  toasts: ToastControls;
  /** Set when the viewer pressed Hide again: focus moves to the redacted section. */
  focusHidden: RefObject<boolean>;
  discussions?: ReactNode;
}

/**
 * Header, details and actions for one loaded episode. Remounted (with a fresh tracking client)
 * whenever protected data is evicted, so no episode list fetched under a reveal survives it.
 */
function EpisodeScreen({ showId, episode, notice, readFailure, store, toasts, focusHidden, discussions }: ScreenProps) {
  const [client] = useState(() => createTrackingClient(createTrackingApi(), showId, toasts));
  const tracking = useSyncExternalStore(client.subscribe, client.getSnapshot, client.getSnapshot);
  const [reveal, setReveal] = useState<{ open: boolean; busy: boolean; error: string | null }>({ open: false, busy: false, error: null });
  const [sheet, setSheet] = useState({ open: false, session: 0 });
  const detailsHeading = useRef<HTMLHeadingElement>(null);
  const hiddenSection = useRef<HTMLElement>(null);

  useEffect(() => {
    void client.loadShow();
  }, [client]);

  const show = tracking.show.status === "ready" ? tracking.show.show : null;
  const code = episodeCode(episode.season, episode.number);
  const watched = isWatched(tracking.overlay, episode);
  const details = visibleDetails(tracking.overlay, episode);
  const busy = tracking.busy.has(`episode:${episode.id}`);
  const showHref = `/shows/${showId}?season=${episode.season}`;

  // Browser title uses the show name and episode code, never the protected episode title.
  useEffect(() => {
    document.title = `${show ? `${show.title} ` : ""}${code} · SceneCask`;
  }, [show, code]);

  useEffect(() => {
    if (focusHidden.current) {
      focusHidden.current = false;
      hiddenSection.current?.focus();
    }
  }, [focusHidden]);

  async function toggle(next: boolean) {
    const outcome = await client.setWatched(episode, next);
    if (outcome.ok) store.applyProgress(outcome.data.episodes);
    else if (outcome.failure.kind === "auth") store.endSession();
    else if (outcome.failure.kind === "conflict") void store.load();
  }

  async function confirmReveal() {
    setReveal({ open: true, busy: true, error: null });
    const result = await store.reveal();
    if (result.ok) {
      setReveal({ open: false, busy: false, error: null });
      return;
    }
    if (result.failure.kind === "auth") return;
    if (!result.granted) {
      setReveal({ open: true, busy: false, error: revealFailureCopy(result.failure) });
      return;
    }
    // The reveal exists; only the authorized read failed, so Retry reads again.
    setReveal({ open: false, busy: false, error: null });
    toasts.show({
      id: `episode-read-${episode.id}`,
      tone: "error",
      message: `Details for ${code} didn’t load. ${trackingFailureCopy(result.failure)}`,
      onRetry: () => void store.load(),
    });
  }

  function hide() {
    focusHidden.current = true;
    void store.hide().then((result) => {
      if (result.ok || result.failure.kind === "auth") return;
      toasts.show({
        id: `episode-hide-${episode.id}`,
        tone: "error",
        message: `Couldn’t finish hiding ${code} for this sign-in session. It stays hidden here. ${trackingFailureCopy(result.failure)}`,
        onRetry: hide,
      });
    });
  }

  const status = noticeText(notice, code);
  const revealedOnly = episode.access === "revealed" && details;

  return (
    <>
      <Link className="sc-ep-back" href={showHref}>
        <span aria-hidden="true">←</span> {show ? `Back to ${show.title}` : "Back to show"}
      </Link>
      <div className="sc-ep-head">
        <Poster className="sc-ep-poster" src={show?.posterUrl} alt="" />
        <div className="sc-ep-heading">
          {show ? (
            <Link className="sc-ep-show" href={showHref}>
              {show.title}
            </Link>
          ) : (
            <Skeleton label="Loading show" width="40%" height={20} />
          )}
          <h1 className="sc-code sc-ep-code">{code}</h1>
        </div>
        <Badge tone={watched ? "accent" : "neutral"}>{badgeText(episode, watched)}</Badge>
      </div>

      {episode.season === 0 && <p className="sc-ep-note" role="note">{SPECIAL_NOTE}</p>}
      {episode.release.state === "future" && <p className="sc-ep-note" role="note">{FUTURE_NOTE}</p>}
      {episode.release.state === "unknown" && <p className="sc-ep-note" role="note">{UNDATED_NOTE}</p>}

      {revealedOnly && (
        <div className="sc-ep-revealed">
          <p>{revealedNotice(code)}</p>
          <Button variant="secondary" onClick={hide}>
            Hide again
          </Button>
        </div>
      )}

      {details ? (
        <div className="sc-ep-details">
          <Poster className="sc-ep-still" aspect="still" src={details.stillUrl} alt={details.stillUrl ? `Still from ${code}` : ""} />
          <h2 ref={detailsHeading} tabIndex={-1}>
            {details.title ?? "Untitled episode"}
          </h2>
          <p className="sc-ep-air">{airText(episode)}</p>
          <p className="sc-reading">{details.overview ?? "No summary yet."}</p>
        </div>
      ) : (
        <section className="sc-ep-hidden" ref={hiddenSection} tabIndex={-1} aria-label={`${code} details`}>
          <p className="sc-ep-air-strong">{airText(episode)}</p>
          {status && <p className="sc-ep-status">{status}</p>}
          {watched && readFailure ? (
            <ErrorState title={`Details for ${code} didn’t load`} onRetry={() => void store.load()}>
              {trackingFailureCopy(readFailure)}
            </ErrorState>
          ) : watched ? (
            <Skeleton label={`Loading ${code} details`} width="70%" height={20} />
          ) : (
            <>
              <p>{HIDDEN_TEXT}</p>
              <Button variant="secondary" onClick={() => setReveal({ open: true, busy: false, error: null })}>
                Reveal episode details…
              </Button>
            </>
          )}
        </section>
      )}

      {tracking.show.status === "error" ? (
        <ErrorState title="Watched actions didn’t load" onRetry={() => void client.loadShow()}>
          {trackingFailureCopy(tracking.show.failure)}
        </ErrorState>
      ) : (
        <div className="sc-actions sc-ep-actions">
          {!show ? (
            <Skeleton label="Loading watched actions" width={220} height={48} />
          ) : watched ? (
            <Button variant="secondary" busy={busy} onClick={() => void toggle(false)}>
              Mark unwatched
            </Button>
          ) : (
            <Button variant="primary" busy={busy} onClick={() => void toggle(true)}>
              Mark {code} watched
            </Button>
          )}
          {show && episode.season > 0 && (
            <Button variant="quiet" onClick={() => setSheet((current) => ({ open: true, session: current.session + 1 }))}>
              Mark watched through {code}…
            </Button>
          )}
        </div>
      )}

      {discussions}

      {show && (
        <CatchupSheet
          key={sheet.session}
          open={sheet.open}
          onOpenChange={(open) => setSheet((current) => ({ ...current, open }))}
          mode="edit"
          through={{ id: episode.id, season: episode.season }}
          show={show}
          client={client}
          snapshot={tracking}
        />
      )}
      <ConfirmDialog
        open={reveal.open}
        onOpenChange={(open) => setReveal((current) => ({ ...current, open, error: open ? current.error : null }))}
        title={revealTitle(code)}
        description={revealBody(code, show?.title ?? "this show")}
        confirmLabel={reveal.error ? "Try again" : "Reveal details"}
        busy={reveal.busy}
        error={reveal.error}
        // After a reveal the trigger is gone; focus moves to the revealed details.
        returnFocusRef={detailsHeading}
        onConfirm={() => void confirmReveal()}
      />
    </>
  );
}

/**
 * D02 episode detail: code/show header, protected details only as the API authorizes them,
 * scoped reveal with Hide again, individual watched actions through the shared tracking client,
 * future/undated notices and the explicit catch-up link.
 */
export function EpisodeDetail({ episodeId, discussions }: EpisodeDetailProps) {
  const { toasts, show: showToast, update, dismiss } = useToastQueue();
  const { store, snapshot } = useEpisode(episodeId);
  // Any successful tracking write, including Undo from a toast, rereads this episode: details
  // unlock only from an authorized read, never from the write.
  const [controls] = useState<ToastControls>(() => ({
    show: (toast) => {
      const id = showToast(toast);
      if (toast.tone === "success") void store.load();
      return id;
    },
    update,
    dismiss,
  }));
  const focusHidden = useRef(false);
  const { load } = snapshot;

  useEffect(() => {
    if (load.status !== "ready") document.title = "Episode · SceneCask";
  }, [load.status]);

  return (
    <section className="sc-ep" aria-busy={load.status === "loading" || undefined}>
      {/* Outside the remounting screen so hide/expiry announcements survive eviction. */}
      <p className="sc-sr-only" role="status">
        {load.status === "ready" ? noticeText(snapshot.notice, episodeCode(load.episode.season, load.episode.number)) : ""}
      </p>
      {load.status === "loading" && (
        <div className="sc-ep-head">
          <Skeleton label="Loading episode" width="50%" height={40} />
        </div>
      )}
      {load.status === "error" && <LoadFailure failure={load.failure} episodeId={episodeId} onRetry={() => void store.load()} />}
      {load.status === "ready" && (
        <EpisodeScreen
          key={`${load.showId}:${snapshot.cacheEpoch}`}
          showId={load.showId}
          episode={load.episode}
          notice={snapshot.notice}
          readFailure={snapshot.readFailure}
          store={store}
          toasts={controls}
          focusHidden={focusHidden}
          discussions={discussions}
        />
      )}
      <ToastViewport toasts={toasts} onDismiss={dismiss} />
    </section>
  );
}
