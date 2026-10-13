"use client";
import Link from "next/link";
import { useCallback, useEffect, useMemo, useRef, useState, type RefObject } from "react";
import { Avatar, Badge, Button, EmptyState, ErrorState, Progress, Skeleton } from "../../components/ui/basic";
import { ConfirmDialog } from "../../components/ui/overlays";
import { endExpiredSession } from "../auth";
import { derivedState, progressText } from "../library/copy";
import { STATUS_LABELS } from "../tracking/copy";
import { createSocialApi, type Failure, type Person, type PersonResult, type SocialApi, type VisibleLibraryItem } from "./api";
import { FailureCopy, useAnnouncer, type Announce } from "./announcer";
import { relationshipText, socialFailureCopy, visibilityLabel } from "./copy";
import { FollowAction } from "./follow-action";
import { PagedList } from "./paged-list";
import { ShowEntry } from "./show-entry";
import { usePaged } from "./use-paged";
import { useViewer } from "./use-viewer";
import "./social.css";

type Phase =
  | { name: "loading" }
  | { name: "error"; failure: Failure }
  | { name: "missing" }
  | { name: "ready"; result: PersonResult }
  | { name: "blocked"; person: Person };

/**
 * D08 /people/[id]: initials, name, handle, visibility and relationship, with follow and block
 * actions. The library renders only when the API says the viewer may see it; otherwise a locked
 * explanation shows no counts, progress or activity.
 */
export function ProfileView({ personId }: { personId: string }) {
  const [api] = useState(createSocialApi);
  const { viewer } = useViewer(api);
  const [phase, setPhase] = useState<Phase>({ name: "loading" });
  const [attempt, setAttempt] = useState(0);
  const { announce, regions } = useAnnouncer();
  const title = useRef<HTMLHeadingElement>(null);

  useEffect(() => {
    let live = true;
    void api.person(personId).then((result) => {
      if (!live) return;
      if (result.ok) return setPhase({ name: "ready", result: result.data });
      if (result.failure.kind === "auth") return endExpiredSession();
      setPhase(result.failure.kind === "not_found" ? { name: "missing" } : { name: "error", failure: result.failure });
    });
    return () => {
      live = false;
    };
  }, [api, personId, attempt]);

  /** A relationship change can grant or revoke library access: re-read what the API allows. */
  const changed = useCallback(
    (person: Person) => {
      setPhase((current) => (current.name === "ready" ? { name: "ready", result: { ...current.result, person } } : current));
      void api.person(personId).then((result) => {
        if (result.ok) setPhase((current) => (current.name === "ready" ? { name: "ready", result: result.data } : current));
      });
    },
    [api, personId],
  );

  const self = viewer.state === "ready" && viewer.user.id === personId;

  return (
    <div className="sc-social sc-reading-column">
      {phase.name === "loading" && (
        <div className="sc-social-loading">
          <Skeleton label="Loading profile…" height={44} width="50%" />
          <Skeleton decorative height={120} />
        </div>
      )}
      {phase.name === "error" && (
        <ErrorState
          title="This profile didn’t load"
          onRetry={() => {
            setPhase({ name: "loading" });
            setAttempt((count) => count + 1);
          }}
        >
          {socialFailureCopy(phase.failure)}
        </ErrorState>
      )}
      {phase.name === "missing" && (
        <>
          <h1 className="sc-social-title">Profile unavailable</h1>
          <EmptyState title="This person isn’t available" action={<Link className="sc-button sc-button-secondary" href="/friends">Find people</Link>}>
            The account may have been deleted, or it can’t be shown to you.
          </EmptyState>
        </>
      )}
      {phase.name === "blocked" && (
        <>
          <h1 ref={title} tabIndex={-1} className="sc-social-title">
            You blocked {phase.person.displayName}
          </h1>
          <p className="sc-social-lede">
            You and @{phase.person.handle} no longer see each other. Follows between you were removed and won’t come back if you
            unblock. <Link href="/settings#sc-settings-blocked">Manage blocked people in Settings</Link>
          </p>
        </>
      )}
      {phase.name === "ready" && (
        <Profile
          api={api}
          result={phase.result}
          self={self}
          titleRef={title}
          announce={announce}
          onChange={changed}
          onGone={() => setPhase({ name: "missing" })}
          onBlocked={(person) => {
            setPhase({ name: "blocked", person });
            announce.done(`You blocked ${person.displayName}.`);
          }}
        />
      )}
      {regions}
    </div>
  );
}

interface ProfileProps {
  api: SocialApi;
  result: PersonResult;
  self: boolean;
  titleRef: RefObject<HTMLHeadingElement | null>;
  announce: Announce;
  onChange: (person: Person) => void;
  onGone: () => void;
  onBlocked: (person: Person) => void;
}

function Profile({ api, result, self, titleRef, announce, onChange, onGone, onBlocked }: ProfileProps) {
  const { person, canViewLibrary } = result;
  const line = relationshipText(person);
  return (
    <>
      <header className="sc-social-profile">
        <span aria-hidden="true">
          <Avatar initials={person.initials || "?"} label={person.displayName} />
        </span>
        <div className="sc-social-profile-main">
          <h1 ref={titleRef} tabIndex={-1} className="sc-social-title">
            {person.displayName}
          </h1>
          <p className="sc-social-person-detail">
            <span className="sc-social-handle">@{person.handle}</span>
            <Badge>{visibilityLabel(person.visibility)}</Badge>
            {self ? <span>This is you</span> : line && <span>{line}</span>}
          </p>
        </div>
      </header>
      {!self && (
        <div className="sc-social-profile-actions">
          <FollowAction api={api} person={person} onChange={onChange} onGone={onGone} announce={announce} />
          <BlockAction api={api} person={person} onBlocked={onBlocked} returnFocusRef={titleRef} />
        </div>
      )}
      {person.relationship.incoming === "pending" && (
        <p className="sc-social-notice">
          {person.displayName} asked to follow you. <Link href="/friends?tab=requests">Answer in Friends</Link>
        </p>
      )}
      <section className="sc-social-section" aria-labelledby="sc-social-library">
        <h2 id="sc-social-library">Library</h2>
        {canViewLibrary ? (
          <VisibleLibrary key={person.relationship.outgoing} api={api} person={person} />
        ) : (
          <Locked person={person} />
        )}
      </section>
    </>
  );
}

/** No counts, progress or activity: only why it's hidden and what would change that. */
function Locked({ person }: { person: Person }) {
  const outgoing = person.relationship.outgoing;
  return (
    <div className="sc-social-locked">
      <p className="sc-social-locked-title">
        @{person.handle}’s library is private
      </p>
      <p>
        Only followers {person.displayName} approves can see their library, progress and activity.{" "}
        {outgoing === "pending"
          ? "Your request is waiting for their approval."
          : person.visibility === "private"
            ? "Request to follow to ask for access."
            : "It isn’t available to you right now."}
      </p>
    </div>
  );
}

function VisibleLibrary({ api, person }: { api: SocialApi; person: Person }) {
  const load = useMemo(() => (cursor?: string) => api.library(person.id, cursor), [api, person.id]);
  const { list, retry, more } = usePaged(load, (item: VisibleLibraryItem) => item.showId);
  // Access can end between the profile read and the library read (unfollow, block, privacy).
  if (list.state === "error" && list.failure.kind === "not_found") return <Locked person={person} />;
  return (
    <PagedList
      list={list}
      label={`${person.displayName}’s library`}
      loadingLabel="Loading library…"
      errorTitle="This library didn’t load"
      empty={<EmptyState title="No saved shows yet">{person.displayName} hasn’t saved any shows.</EmptyState>}
      retry={retry}
      more={() => void more()}
      itemKey={(item) => item.showId}
      className="sc-social-list sc-social-shows"
    >
      {(item) => <LibraryShow item={item} />}
    </PagedList>
  );
}

/** A saved show in someone else's library: codes and counts only, never episode titles. */
function LibraryShow({ item }: { item: VisibleLibraryItem }) {
  const facts = [STATUS_LABELS[item.status], derivedState(item.progress)?.label, item.progress.total > 0 && progressText(item.progress)];
  return (
    <ShowEntry show={{ id: item.showId, title: item.title, year: item.year, posterUrl: item.posterUrl }}>
      <span className="sc-social-person-detail">{facts.filter(Boolean).join(" · ")}</span>
      <Progress label={`${item.title} progress`} value={item.progress.percent} />
    </ShowEntry>
  );
}

/** Block with a confirmation and no Undo (D02): Settings offers Unblock. */
interface BlockActionProps {
  api: SocialApi;
  person: Person;
  onBlocked: (person: Person) => void;
  /** The page heading, which becomes "You blocked …" once the block is accepted. */
  returnFocusRef: RefObject<HTMLHeadingElement | null>;
}

function BlockAction({ api, person, onBlocked, returnFocusRef }: BlockActionProps) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<Failure | null>(null);

  async function block() {
    setBusy(true);
    setError(null);
    const result = await api.block(person.id);
    setBusy(false);
    if (result.ok) {
      setOpen(false);
      return onBlocked(person);
    }
    if (result.failure.kind === "auth") return endExpiredSession();
    setError(result.failure);
  }

  return (
    <>
      <Button
        variant="quiet"
        onClick={() => {
          setError(null);
          setOpen(true);
        }}
      >
        Block
      </Button>
      <ConfirmDialog
        open={open}
        onOpenChange={setOpen}
        title={`Block ${person.displayName}?`}
        confirmLabel="Block"
        destructive
        busy={busy}
        onConfirm={() => void block()}
        error={error && <FailureCopy failure={error} />}
        returnFocusRef={returnFocusRef}
      >
        <p>
          You and @{person.handle} won’t be able to find each other or see each other’s library, activity or discussion
          comments. Follows and requests between you are removed both ways.
        </p>
        <p>They aren’t notified. You can unblock them in Settings, but follows won’t come back.</p>
      </ConfirmDialog>
    </>
  );
}
