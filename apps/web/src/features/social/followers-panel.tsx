"use client";
import { useRef, useState } from "react";
import { Button, EmptyState } from "../../components/ui/basic";
import { ConfirmDialog } from "../../components/ui/overlays";
import { endExpiredSession } from "../auth";
import type { Failure, Person, SocialApi } from "./api";
import { FailureCopy, useAnnouncer } from "./announcer";
import { PagedList } from "./paged-list";
import { PersonRow } from "./person-row";
import type { Paged } from "./use-paged";

export interface FollowersPanelProps {
  api: SocialApi;
  list: Paged<Person>;
  retry: () => void;
  more: () => void;
  remove: (person: Person) => void;
}

/** D08 Followers: Remove is confirmed, stating the loss of access and that nobody is notified. */
export function FollowersPanel({ api, list, retry, more, remove }: FollowersPanelProps) {
  const { announce, regions } = useAnnouncer();
  const [target, setTarget] = useState<Person | null>(null);
  const [busy, setBusy] = useState(false);
  const [dialogError, setDialogError] = useState<Failure | null>(null);
  const heading = useRef<HTMLHeadingElement>(null);

  async function confirm() {
    if (!target) return;
    setBusy(true);
    setDialogError(null);
    const result = await api.removeFollower(target.id);
    setBusy(false);
    // Already gone (404) is the outcome that was asked for.
    if (result.ok || result.failure.kind === "not_found") {
      remove(target);
      setTarget(null);
      return announce.done(`${target.displayName} no longer follows you.`);
    }
    if (result.failure.kind === "auth") return endExpiredSession();
    setDialogError(result.failure);
  }

  return (
    <div className="sc-social-panel">
      <h2 ref={heading} tabIndex={-1} className="sc-social-panel-title">
        Followers
      </h2>
      <PagedList
        list={list}
        label="Followers"
        loadingLabel="Loading your followers…"
        errorTitle="Your followers didn’t load"
        empty={
          <EmptyState title="No followers yet">
            People who follow you appear here. Your tracking works the same without them.
          </EmptyState>
        }
        retry={retry}
        more={more}
        itemKey={(person) => person.id}
      >
        {(person) => (
          <PersonRow
            person={person}
            action={
              <Button
                variant="secondary"
                aria-label={`Remove ${person.displayName}`}
                onClick={() => {
                  setDialogError(null);
                  setTarget(person);
                }}
              >
                Remove
              </Button>
            }
          />
        )}
      </PagedList>
      {regions}
      <ConfirmDialog
        open={target !== null}
        onOpenChange={(open) => !open && setTarget(null)}
        title={target ? `Remove ${target.displayName} as a follower?` : ""}
        confirmLabel="Remove follower"
        destructive
        busy={busy}
        onConfirm={() => void confirm()}
        error={dialogError && <FailureCopy failure={dialogError} />}
        returnFocusRef={heading}
      >
        {target && (
          <p>
            @{target.handle} loses access to your library and activity right away while your account is private. They won’t
            be notified, and they can ask to follow you again.
          </p>
        )}
      </ConfirmDialog>
    </div>
  );
}
