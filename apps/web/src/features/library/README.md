# Library (R20, C05)

`LibraryView` renders `/library`: saved shows with derived progress, four manual-status
filters with counts, title search, and per-row status and removal. All calls go through
the generated client (`api.ts`); writes and the single-show recovery read reuse the
tracking adapter.

- **List.** `GET /library?status=&q=&cursor=`. Search is debounced 300ms; a new query
  aborts the earlier request and only the latest settles. `?status=` and `?q=` stay in the
  URL with `replaceState`, so reload and Back/Forward restore them. "Show more shows"
  follows the cursor and moves focus to the first new row.
- **Filters.** All, Plan to watch, Watching, On hold, Dropped, with the server's counts
  across all saved shows. There is no Completed filter: Caught up and Completed are shown
  per row exactly as the backend computed `progress.state`. A canceled or ended show with
  incomplete release info stays Caught up.
- **Rows.** Poster, title and year (opens `/shows/{id}`), derived state, "2 of 6 episodes
  watched · Next S1 E2", out-of-order and release-info notes, and a progress bar. Episode
  codes only, never titles. The row menu sets the manual status or removes the show
  (history and status are kept).
- **Optimistic writes.** Each status or removal owns a patch keyed by action; counts are
  the server's counts adjusted by each loaded row's change. Settling removes only that
  action's patch, so a failure never restores a snapshot over a later edit. A known
  rejection (for example 409) re-reads that one show and accepts it only at an equal or
  newer revision; Retry then starts from that revision with a new key. An unknown outcome
  (network/503) keeps the last known state and Retry reuses the Idempotency-Key and body.
  Success toasts offer Undo; Undo reports changes made afterwards that were kept.
- **States.** Loading skeleton, load error with Retry, signed-out and unverified, empty
  library (Find a show → `/discover`), and no matches (search and filter kept, with
  "Clear search and filter").

Shared with the tracking client (`features/tracking/mutation-support.ts`): per-scope
Idempotency-Keys and the toast Undo flow. The component suites' fake-API plumbing is in
`features/tracking/fixture-api.ts`.

## Checks

The fixture app renders the production route on port 3121; a stateful fake of the C05
library API answers at the `/api/v1` boundary. This is not live integration (R24 owns
that gate).

```sh
yarn playwright test --config apps/web/src/features/library/playwright.config.ts
node apps/web/src/features/library/fixture-server.mjs --build
```

Registering this suite in root scripts and CI is R01-owned. See
[verification evidence](verification.md).
