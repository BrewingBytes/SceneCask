# Discover search (R19, C05)

`DiscoverSearch` renders `/discover`: TV search, disambiguation, open-by-import and
Add to Plan to watch. All calls go through the generated client (`api.ts`).

- **Search.** The trimmed query (2–100 characters) is debounced 300ms. A new term
  aborts the earlier request, and a response settles only if its term is still current,
  so a slow earlier query never overwrites a later one. `?q=` is kept in the URL with
  `replaceState`. Skeleton, results, no-results, failure (Retry keeps the query),
  signed-out and unverified states. `Show more results` appends the next page,
  skips repeated provider IDs and moves focus to the first new result.
- **Disambiguation.** Each row shows title, year (or "year unknown"), genres and
  poster. Accessible names repeat the year and genres.
- **Open.** `POST /shows/import` then navigates to `/shows/{showId}` (application ID).
  Imports are cached per provider ID for the page's lifetime.
- **Add.** Import → `GET /shows/{id}` → `PUT /library/{id}`. A show already saved is
  not saved again. A new entry sends `status: plan_to_watch, expectedRevision: 0`; an
  unsaved existing entry sends its revision and keeps its status and history. A save
  with an unknown outcome (network/503) is retried with the same Idempotency-Key and
  body; a known rejection (for example 409) starts again from fresh library state with
  a new key. One in-flight action per result. The success toast's Undo calls
  `POST /actions/{actionId}/undo`; a no-op save has no Undo.
- Failure copy comes from `copy.ts`, never from server messages or provider payloads.
- `recommendations` is an empty slot for R28's beta social discovery.

Search results carry no library state, so "In library" appears only after an Add (or
an Add that finds the show already saved) on the current page.

## Checks

The fixture app renders the production `/discover` route on port 3119; Playwright
supplies `/api/v1` responses at the network boundary. This is not live integration
(R24 mounts search/import and owns the live gate).

```sh
yarn playwright test --config apps/web/src/features/catalog/search/playwright.config.ts
node apps/web/src/features/catalog/search/fixture-server.mjs --build
```

Registering this suite in root scripts and CI is R01-owned. See
[verification evidence](verification.md).
