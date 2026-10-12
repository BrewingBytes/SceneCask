"use client";
import Link from "next/link";
import { useEffect, useRef, useState, useSyncExternalStore, type ReactNode } from "react";
import { EmptyState, ErrorState, Skeleton } from "../../components/ui/basic";
import { ToastViewport, useToastQueue } from "../../components/ui/overlays";
import { CatchupSheet } from "../tracking/catch-up-sheet";
import { trackingFailureCopy } from "../tracking/copy";
import type { TrackingClient } from "../tracking/tracking-client";
import type { Failure } from "./api";
import type { HomeLoad } from "./home-client";
import { todayKicker, upcomingNote } from "./copy";
import { CaughtUpRow, FeaturedShow, UpNextRow } from "./home-sections";
import { useHome } from "./use-home";
import "./home.css";

export interface HomeViewProps {
  /** Beta extension point for the social area; Home is complete without it. */
  social?: ReactNode;
}

const DATE_NOTE = "Release dates without a reliable time zone use midnight UTC, so streaming services may differ.";

const noSubscription = () => () => {};

function LoadFailure({ failure, onRetry }: { failure: Failure; onRetry: () => void }) {
  if (failure.kind === "auth")
    return (
      <EmptyState title="Sign in to see your shows" action={<Link className="sc-button sc-button-primary" href="/auth/signin?returnTo=%2Fhome">Sign in</Link>}>
        Your session has ended. Your library and progress are saved to your account.
      </EmptyState>
    );
  if (failure.kind === "unverified")
    return (
      <EmptyState title="Verify your email to track shows" action={<Link className="sc-button sc-button-primary" href="/auth/verify">Verify email</Link>}>
        Check your inbox for the verification link.
      </EmptyState>
    );
  return (
    <ErrorState title="Your shows didn’t load" onRetry={onRetry}>
      {trackingFailureCopy(failure)}
    </ErrorState>
  );
}

/** The shared catch-up sheet for one show, bound to that show's tracking client. */
function HomeCatchup({ client, open, onOpenChange }: { client: TrackingClient; open: boolean; onOpenChange: (open: boolean) => void }) {
  const snapshot = useSyncExternalStore(client.subscribe, client.getSnapshot, client.getSnapshot);
  if (snapshot.show.status !== "ready") return null;
  return <CatchupSheet open={open} onOpenChange={onOpenChange} mode="edit" show={snapshot.show.show} client={client} snapshot={snapshot} />;
}

/** D02 Home: featured next episode, other Up next shows, caught-up shows and solo empty states. */
export function HomeView({ social }: HomeViewProps) {
  const { toasts, show, update, dismiss } = useToastQueue();
  const [controls] = useState(() => ({ show, update, dismiss }));
  const { client, snapshot } = useHome(controls);
  const { load, busy, upcoming } = snapshot;
  const [sheet, setSheet] = useState<{ client: TrackingClient; open: boolean; session: number } | null>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  // The date is the viewer's own; rendered only in the browser so it never comes from the server.
  const today = useSyncExternalStore(noSubscription, () => todayKicker(new Date()), () => null);

  // A mark can move its show to another section. When the refetched sections remove the focused
  // control, keyboard position moves to the page heading instead of the document start.
  const refocus = useRef<{ settled: HomeLoad | null } | null>(null);
  useEffect(() => {
    const pending = refocus.current;
    if (!pending || busy.size > 0) return;
    const lost = document.activeElement === document.body;
    if (pending.settled === null && !lost) {
      // The mark settled; wait for the refetch it started.
      pending.settled = load;
      return;
    }
    if (pending.settled === load && !lost) return;
    refocus.current = null;
    if (lost) heading.current?.focus();
  }, [load, busy]);
  const markNext = (showId: string) => {
    refocus.current = { settled: null };
    void client.markNext(showId);
  };

  async function openCatchup(showId: string) {
    const tracking = await client.catchupClient(showId);
    if (tracking) setSheet((current) => ({ client: tracking, open: true, session: (current?.session ?? 0) + 1 }));
  }

  const home = load.status === "ready" ? load.home : null;
  const [featured, ...rest] = home?.upNext ?? [];
  const notes = home?.caughtUp.map((item) => upcomingNote(item, upcoming[item.showId] ?? null)) ?? [];

  return (
    <section className="sc-home" aria-labelledby="sc-home-title">
      <div className="sc-home-head">
        <h1 id="sc-home-title" ref={heading} tabIndex={-1}>
          Up next
        </h1>
        {today && <span className="sc-kicker">{today}</span>}
      </div>
      <p className="sc-home-hint" role="status">
        {load.status === "loading" ? "Loading your shows…" : ""}
      </p>
      {load.status === "loading" && (
        <div className="sc-home-loading" aria-hidden="true">
          <Skeleton decorative height={220} />
          <Skeleton decorative width="60%" height={18} />
          <Skeleton decorative width="40%" height={14} />
        </div>
      )}
      {load.status === "error" && <LoadFailure failure={load.failure} onRetry={() => void client.load()} />}
      {home && (
        <>
          {featured && (
            <FeaturedShow
              item={featured}
              busy={busy.has(featured.showId)}
              onMark={() => markNext(featured.showId)}
              onCatchup={() => void openCatchup(featured.showId)}
            />
          )}
          {home.libraryEmpty ? (
            <div className="sc-home-state">
              <h2>Nothing to track yet</h2>
              <p className="sc-reading">Add a show you’re watching and we’ll keep your next episode here.</p>
              <Link className="sc-button sc-button-primary" href="/discover">
                Find a show
              </Link>
            </div>
          ) : (
            !featured &&
            (home.caughtUp.length > 0 ? (
              <div className="sc-home-state sc-home-state-row">
                <div>
                  <h2>✓ You’re caught up</h2>
                  <p className="sc-reading">Every released episode of the shows you’re watching is marked. What’s coming is below.</p>
                </div>
                <Link className="sc-button sc-button-primary" href="/discover">
                  Find something new
                </Link>
              </div>
            ) : (
              <div className="sc-home-state">
                <h2>Choose a show to watch</h2>
                <p className="sc-reading">Shows you set to Watching appear here with their next episode. Pick one from your library.</p>
                <Link className="sc-button sc-button-primary" href="/library">
                  Go to your library
                </Link>
              </div>
            ))
          )}
          {rest.length > 0 && (
            <ul className="sc-home-rows sc-home-up" aria-label="More shows up next">
              {rest.map((item) => (
                <UpNextRow key={item.showId} item={item} busy={busy.has(item.showId)} onMark={() => markNext(item.showId)} />
              ))}
            </ul>
          )}
          {(home.caughtUp.length > 0 || social) && (
            <div className="sc-home-lower">
              {home.caughtUp.length > 0 && (
                <section aria-labelledby="sc-home-caught">
                  <h2 id="sc-home-caught" className="sc-home-section">
                    Waiting on new episodes
                  </h2>
                  <ul className="sc-home-rows">
                    {home.caughtUp.map((item) => (
                      <CaughtUpRow key={item.showId} item={item} episodes={upcoming[item.showId] ?? null} />
                    ))}
                  </ul>
                  {notes.some((note) => note.dated) && <p className="sc-home-note">{DATE_NOTE}</p>}
                </section>
              )}
              {social}
            </div>
          )}
        </>
      )}
      {sheet && (
        <HomeCatchup
          key={sheet.session}
          client={sheet.client}
          open={sheet.open}
          onOpenChange={(open) => setSheet((current) => current && { ...current, open })}
        />
      )}
      <ToastViewport toasts={toasts} onDismiss={dismiss} />
    </section>
  );
}
