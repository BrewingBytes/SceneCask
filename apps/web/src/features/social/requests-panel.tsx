"use client";
import { useRef, useState } from "react";
import { Button, EmptyState } from "../../components/ui/basic";
import type { Person, SocialApi } from "./api";
import { useAnnouncer } from "./announcer";
import { PagedList } from "./paged-list";
import { PersonRow } from "./person-row";
import type { Paged } from "./use-paged";

export interface RequestsPanelProps {
  api: SocialApi;
  list: Paged<Person>;
  retry: () => void;
  more: () => void;
  /** Drops a request that was answered or is no longer pending. */
  remove: (person: Person) => void;
  onApproved: () => void;
}

/**
 * D08 Incoming requests. A row leaves only after the API accepts the decision, or answers that it
 * is no longer pending (404); focus then moves to the panel heading.
 */
export function RequestsPanel({ api, list, retry, more, remove, onApproved }: RequestsPanelProps) {
  const { announce, regions } = useAnnouncer();
  const [busy, setBusy] = useState<string | null>(null);
  const heading = useRef<HTMLHeadingElement>(null);

  async function decide(person: Person, decision: "approve" | "decline") {
    setBusy(`${person.id}:${decision}`);
    announce.clear();
    const result = await api.decide(person.id, decision);
    setBusy(null);
    if (result.ok || result.failure.kind === "not_found") {
      remove(person);
      heading.current?.focus();
      if (!result.ok) return announce.done(`The request from ${person.displayName} is no longer pending.`);
      if (decision === "decline") return announce.done(`Declined ${person.displayName}’s request. They aren’t notified.`);
      onApproved();
      return announce.done(`${person.displayName} can now see your library, progress and activity.`);
    }
    announce.failed(`Couldn’t ${decision} ${person.displayName}’s request.`, result.failure);
  }

  return (
    <div className="sc-social-panel">
      <h2 ref={heading} tabIndex={-1} className="sc-social-panel-title">
        Follow requests
      </h2>
      <PagedList
        list={list}
        label="Follow requests"
        loadingLabel="Loading follow requests…"
        errorTitle="Follow requests didn’t load"
        empty={
          <EmptyState title="No pending requests">
            When someone asks to follow your private account, you can approve or decline it here.
          </EmptyState>
        }
        retry={retry}
        more={more}
        itemKey={(person) => person.id}
      >
        {(person) => (
          <PersonRow
            person={person}
            detail="Wants to follow you. Approving lets them see your library, progress and activity."
            action={
              <>
                <Button
                  busy={busy === `${person.id}:approve`}
                  disabled={busy !== null && busy !== `${person.id}:approve`}
                  aria-label={`Approve ${person.displayName}`}
                  onClick={() => void decide(person, "approve")}
                >
                  Approve
                </Button>
                <Button
                  variant="secondary"
                  busy={busy === `${person.id}:decline`}
                  disabled={busy !== null && busy !== `${person.id}:decline`}
                  aria-label={`Decline ${person.displayName}`}
                  onClick={() => void decide(person, "decline")}
                >
                  Decline
                </Button>
              </>
            }
          />
        )}
      </PagedList>
      {regions}
    </div>
  );
}
