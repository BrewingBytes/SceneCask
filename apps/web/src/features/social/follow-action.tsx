"use client";
import { useEffect, useRef, useState } from "react";
import { Button } from "../../components/ui/basic";
import { ConfirmDialog } from "../../components/ui/overlays";
import { endExpiredSession } from "../auth";
import type { Failure, FollowState, Person, Result, SocialApi } from "./api";
import type { Announce } from "./announcer";
import { FailureCopy } from "./announcer";

type Outgoing = Person["relationship"]["outgoing"];

const withOutgoing = (person: Person, outgoing: Outgoing): Person => ({
  ...person,
  relationship: { ...person.relationship, outgoing, mutual: outgoing === "approved" && person.relationship.incoming === "approved" },
});

export interface FollowActionProps {
  api: SocialApi;
  person: Person;
  /** The authoritative person after a write or a re-read. */
  onChange: (person: Person) => void;
  /** The person is gone for the viewer (deleted, or a block either way). */
  onGone?: (person: Person) => void;
  announce: Announce;
}

/**
 * Follow, Request to follow, Cancel request or Unfollow, by the server's outgoing edge. The edge
 * changes only when the API says so: a private account stays Requested until its owner actually
 * approves, and nothing here assumes approval. A lost response re-reads the person before saying
 * what happened.
 */
export function FollowAction({ api, person, onChange, onGone, announce }: FollowActionProps) {
  const { outgoing } = person.relationship;
  const name = person.displayName;
  const [busy, setBusy] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [dialogError, setDialogError] = useState<Failure | null>(null);
  // Whichever button this action shows now: the dialog returns focus there, and a change of
  // state that replaces the focused button moves focus to its successor.
  const trigger = useRef<HTMLElement | null>(null);
  const refocus = useRef(false);
  const root = (node: HTMLDivElement | null) => {
    trigger.current = node?.querySelector("button") ?? null;
  };
  useEffect(() => {
    if (!refocus.current) return;
    refocus.current = false;
    if (document.activeElement === document.body || document.activeElement === null) trigger.current?.focus();
  }, [outgoing]);

  function succeeded(next: Outgoing) {
    refocus.current = true;
    onChange(withOutgoing(person, next));
    if (next === "pending") announce.done(`Request sent to ${name}. You’ll see their library and activity once they approve.`);
    else if (next === "approved") announce.done(`You’re following ${name}.`);
    else if (outgoing === "pending") announce.done(`Your request to follow ${name} was canceled.`);
    else announce.done(`You unfollowed ${name}.`);
  }

  /** The edge after a failed write: re-read once when the write may have committed. */
  async function settleFailure(result: Result<unknown>, wanted: Outgoing[], lead: string) {
    if (result.ok) return;
    const { failure } = result;
    if (failure.kind === "auth") return endExpiredSession();
    if (failure.kind === "not_found") {
      announce.failed(`${name} isn’t available any more.`, failure);
      return onGone?.(person);
    }
    if (failure.kind === "unavailable") {
      const fresh = await api.person(person.id);
      if (fresh.ok) {
        const now = fresh.data.person.relationship.outgoing;
        onChange(fresh.data.person);
        if (wanted.includes(now)) return succeeded(now);
      } else if (fresh.failure.kind === "not_found") {
        announce.failed(`${name} isn’t available any more.`, fresh.failure);
        return onGone?.(person);
      }
    }
    announce.failed(lead, failure);
  }

  async function follow() {
    setBusy(true);
    announce.clear();
    const result = await api.follow(person.id);
    setBusy(false);
    if (result.ok) return succeeded((result.data as { state: FollowState }).state);
    await settleFailure(result, ["pending", "approved"], `Couldn’t follow ${name}.`);
  }

  async function remove(fromDialog: boolean) {
    setBusy(true);
    setDialogError(null);
    if (!fromDialog) announce.clear();
    const result = await api.unfollow(person.id);
    setBusy(false);
    if (result.ok) {
      setConfirming(false);
      return succeeded("none");
    }
    if (fromDialog && result.failure.kind !== "unavailable" && result.failure.kind !== "not_found" && result.failure.kind !== "auth") {
      return setDialogError(result.failure);
    }
    setConfirming(false);
    await settleFailure(result, ["none"], outgoing === "pending" ? `Couldn’t cancel your request.` : `Couldn’t unfollow ${name}.`);
  }

  if (outgoing === "pending")
    return (
      <div ref={root} className="sc-social-follow">
        <Button variant="secondary" busy={busy} aria-label={`Cancel request to follow ${name}`} onClick={() => void remove(false)}>
          Cancel request
        </Button>
      </div>
    );

  if (outgoing === "approved")
    return (
      <div ref={root} className="sc-social-follow">
        <Button
          variant="secondary"
          busy={busy}
          aria-label={`Unfollow ${name}`}
          onClick={() => {
            setDialogError(null);
            setConfirming(true);
          }}
        >
          Unfollow
        </Button>
        <ConfirmDialog
          open={confirming}
          onOpenChange={(open) => !busy && setConfirming(open)}
          title={`Unfollow ${name}?`}
          confirmLabel="Unfollow"
          destructive
          busy={busy}
          onConfirm={() => void remove(true)}
          error={dialogError && <FailureCopy failure={dialogError} />}
          returnFocusRef={trigger}
        >
          {person.visibility === "private" ? (
            <p>
              You’ll lose access to @{person.handle}’s library and activity right away. To follow again, you’ll need to send
              a new request.
            </p>
          ) : (
            <p>You’ll stop seeing @{person.handle}’s activity. You can follow again at any time.</p>
          )}
        </ConfirmDialog>
      </div>
    );

  const label = person.visibility === "private" ? "Request to follow" : "Follow";
  return (
    <div ref={root} className="sc-social-follow">
      <Button busy={busy} aria-label={`${label} ${name}`} onClick={() => void follow()}>
        {label}
      </Button>
    </div>
  );
}
