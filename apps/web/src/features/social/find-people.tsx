"use client";
import { useCallback, useEffect, useMemo, useState } from "react";
import { EmptyState, TextField } from "../../components/ui/basic";
import type { Person, SocialApi } from "./api";
import { useAnnouncer } from "./announcer";
import { FollowAction } from "./follow-action";
import { PagedList } from "./paged-list";
import { PersonRow } from "./person-row";
import { usePaged } from "./use-paged";

/** C06: q is trimmed and must be 2–100 characters. A leading @ is part of how people type handles. */
export const MIN_QUERY = 2;
export const MAX_QUERY = 100;
export const SEARCH_DEBOUNCE_MS = 300;

export const searchTerm = (query: string) => query.trim().replace(/^@/, "").trim();
const personKey = (person: Person) => person.id;

/** D08 Find people: debounced handle/name search, each result with its follow action. */
export function FindPeople({ api }: { api: SocialApi }) {
  const [query, setQuery] = useState("");
  const [term, setTerm] = useState("");
  const { announce, regions } = useAnnouncer();

  useEffect(() => {
    const timer = setTimeout(() => setTerm(searchTerm(query)), SEARCH_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [query]);

  const length = Array.from(term).length;
  const valid = length >= MIN_QUERY && length <= MAX_QUERY;
  const load = useMemo(() => (valid ? (cursor?: string) => api.search(term, cursor) : null), [api, term, valid]);
  const { list, retry, more, update } = usePaged(load, personKey);

  const replace = useCallback((next: Person) => update((items) => items.map((item) => (item.id === next.id ? next : item))), [update]);
  const drop = useCallback((gone: Person) => update((items) => items.filter((item) => item.id !== gone.id)), [update]);

  const typed = Array.from(searchTerm(query)).length;
  const hint =
    typed > MAX_QUERY
      ? `Use at most ${MAX_QUERY} characters.`
      : typed > 0 && typed < MIN_QUERY
        ? `Type at least ${MIN_QUERY} characters.`
        : "Search by display name or @handle. Email addresses aren’t searchable.";

  return (
    <div className="sc-social-panel">
      <form role="search" onSubmit={(event) => event.preventDefault()}>
        <TextField
          label="Search people"
          hint={hint}
          type="search"
          value={query}
          maxLength={MAX_QUERY + 2}
          autoComplete="off"
          spellCheck={false}
          onChange={(event) => setQuery(event.target.value)}
        />
      </form>
      {list.state === "idle" && typed === 0 && (
        <EmptyState title="Find people you know">
          Following is one-way. Public accounts can be followed right away; private accounts approve each request first.
        </EmptyState>
      )}
      <PagedList
        list={list}
        label="People"
        loadingLabel="Searching people…"
        errorTitle="People search didn’t load"
        empty={<EmptyState title={`No one matches “${term}”`}>Check the spelling, or try their @handle.</EmptyState>}
        retry={retry}
        more={() => void more()}
        itemKey={personKey}
      >
        {(person) => (
          <PersonRow
            person={person}
            action={<FollowAction api={api} person={person} onChange={replace} onGone={drop} announce={announce} />}
          />
        )}
      </PagedList>
      {regions}
    </div>
  );
}
