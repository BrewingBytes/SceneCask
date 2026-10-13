"use client";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { Avatar, Button, EmptyState, ErrorState, Skeleton } from "../../../components/ui/basic";
import { ConfirmDialog } from "../../../components/ui/overlays";
import { endExpiredSession } from "../../auth";
import { initialsOf } from "../../auth/validation";
import type { Failure, Page, Result } from "./api";
import { settingsFailureCopy } from "./copy";
import { SectionStatus, SettingsSection } from "./section";

interface Entry {
  id: string;
  handle: string;
  displayName: string;
  initials?: string;
}

type List<T> =
  | { state: "loading" }
  | { state: "error"; failure: Failure }
  | { state: "ready"; items: T[]; nextCursor: string | null; more: "idle" | "loading" | Failure };

export interface PeopleListProps<T extends Entry> {
  id: string;
  title: string;
  lede: ReactNode;
  load: (cursor?: string) => Promise<Result<Page<T>>>;
  remove: (id: string) => Promise<Result<unknown>>;
  /** Announced while loading, and the empty state. */
  loadingLabel: string;
  empty: { title: string; body: string };
  loadError: string;
  /** Visible button text; its accessible name adds the person's name. */
  actionLabel: string;
  confirm: (person: T) => { title: string; body: ReactNode; label: string };
  done: (person: T) => string;
}

/**
 * A D04 people list (followers, blocks): loading, empty, error with Retry, "Show more" by
 * cursor, and a confirmed removal per row. The list changes only after the API accepts.
 */
export function PeopleList<T extends Entry>(props: PeopleListProps<T>) {
  const { load, remove } = props;
  const [list, setList] = useState<List<T>>({ state: "loading" });
  const [target, setTarget] = useState<T | null>(null);
  const [busy, setBusy] = useState(false);
  const [dialogError, setDialogError] = useState("");
  const [status, setStatus] = useState("");
  const heading = useRef<HTMLElement | null>(null);

  // Each Retry is a new attempt; a superseded or unmounted attempt never settles.
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let live = true;
    void load().then((result) => {
      if (!live) return;
      if (result.ok) setList({ state: "ready", items: result.data.items, nextCursor: result.data.nextCursor, more: "idle" });
      else if (result.failure.kind === "auth") endExpiredSession();
      else setList({ state: "error", failure: result.failure });
    });
    return () => {
      live = false;
    };
  }, [load, attempt]);
  const retry = () => {
    setList({ state: "loading" });
    setAttempt((count) => count + 1);
  };

  async function more() {
    if (list.state !== "ready" || !list.nextCursor || list.more === "loading") return;
    setList({ ...list, more: "loading" });
    const result = await load(list.nextCursor);
    setList((current) => {
      if (current.state !== "ready") return current;
      if (!result.ok) return { ...current, more: result.failure };
      const known = new Set(current.items.map((item) => item.id));
      const items = [...current.items, ...result.data.items.filter((item) => !known.has(item.id))];
      return { state: "ready", items, nextCursor: result.data.nextCursor, more: "idle" };
    });
  }

  async function confirm() {
    if (!target) return;
    setBusy(true);
    setDialogError("");
    const result = await remove(target.id);
    setBusy(false);
    // Already gone (404) is the outcome that was asked for.
    if (result.ok || result.failure.kind === "not_found") {
      setList((current) => (current.state === "ready" ? { ...current, items: current.items.filter((item) => item.id !== target.id) } : current));
      setStatus(props.done(target));
      setTarget(null);
      return;
    }
    if (result.failure.kind === "auth") return endExpiredSession();
    setDialogError(settingsFailureCopy(result.failure));
  }

  const asked = target && props.confirm(target);
  return (
    <SettingsSection id={props.id} title={props.title} lede={props.lede}>
      {list.state === "loading" && (
        <ul className="sc-settings-people" aria-hidden="true">
          {[0, 1].map((index) => (
            <li key={index} className="sc-settings-row">
              <Skeleton decorative width={44} height={44} />
              <Skeleton decorative width="40%" height={16} />
            </li>
          ))}
        </ul>
      )}
      {list.state === "error" && (
        <ErrorState title={props.loadError} onRetry={retry}>
          {settingsFailureCopy(list.failure)}
        </ErrorState>
      )}
      {list.state === "ready" && list.items.length === 0 && <EmptyState title={props.empty.title}>{props.empty.body}</EmptyState>}
      {list.state === "ready" && list.items.length > 0 && (
        <ul className="sc-settings-people" aria-label={props.title}>
          {list.items.map((person) => (
            <li key={person.id} className="sc-settings-row">
              <div className="sc-settings-person">
                {/* The name beside it is the accessible text; the avatar only repeats it. */}
                <span aria-hidden="true">
                  <Avatar initials={person.initials || initialsOf(person.displayName) || "?"} label={person.displayName} />
                </span>
                <span className="sc-settings-row-main">
                  <span className="sc-settings-row-title">{person.displayName}</span>
                  <span className="sc-settings-row-detail">@{person.handle}</span>
                </span>
              </div>
              <div className="sc-settings-row-action">
                <Button
                  variant="secondary"
                  aria-label={`${props.actionLabel} ${person.displayName}`}
                  onClick={(event) => {
                    heading.current = event.currentTarget.closest("section")?.querySelector("h2") ?? null;
                    setDialogError("");
                    setTarget(person);
                  }}
                >
                  {props.actionLabel}
                </Button>
              </div>
            </li>
          ))}
        </ul>
      )}
      {list.state === "ready" && list.nextCursor && (
        <div className="sc-settings-actions">
          <Button variant="secondary" busy={list.more === "loading"} onClick={() => void more()}>
            Show more
          </Button>
        </div>
      )}
      <SectionStatus
        status={list.state === "loading" ? props.loadingLabel : status}
        problem={list.state === "ready" && typeof list.more === "object" && `More people didn’t load. ${settingsFailureCopy(list.more)}`}
      />
      <ConfirmDialog
        open={target !== null}
        onOpenChange={(open) => !open && setTarget(null)}
        title={asked?.title ?? ""}
        confirmLabel={asked?.label ?? ""}
        destructive
        busy={busy}
        onConfirm={() => void confirm()}
        error={dialogError}
        returnFocusRef={heading}
      >
        {asked?.body}
      </ConfirmDialog>
    </SettingsSection>
  );
}
