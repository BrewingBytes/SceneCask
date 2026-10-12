# Home (R23, C05)

`HomeView` renders `/home` from `GET /home` exactly as the server returns it; `Welcome`
renders `/`. All calls go through the generated client (`api.ts`).

- **Sections.** The first `upNext` item is featured: poster, title, year, watched count,
  progress, the next episode's code ("Title hidden until you watch"), Mark watched, View
  episode and "Watched more? Catch up". The other `upNext` items follow as rows with their own
  Mark watched. `caughtUp` shows are listed under "Waiting on new episodes" with the derived
  state. Order and sections are never recomputed in the browser.
- **Caught-up notes.** For each caught-up show Home reads the show and its latest regular season
  and names the first unwatched episode that is not released: "S2 E3 · Airs Nov 12, 2026
  (estimated)", "· date not announced" or "· Release date unknown". Codes and release labels
  only; a UTC-midnight note appears when a date is shown. Without listings the note falls back
  to the derived state.
- **Mark, catch-up and Undo** use the shared tracking client (`features/tracking`) for that
  show: Idempotency-Keys, unknown-outcome Retry with the same key, success toasts with Undo, and
  the shared catch-up sheet. When a write, Undo or conflict changes a show, Home refetches the
  authoritative sections. If the focused control leaves with its show, focus moves to the "Up
  next" heading.
- **Refetch.** Returning to the tab (focus or visibility) refetches, so a release since the
  last read moves a caught-up show back to Up next. A failed background refetch keeps the last
  sections unless the session ended.
- **States.** Loading skeleton, load error with Retry, signed out, unverified, empty library
  ("Nothing to track yet" → Discover), saved shows but none Watching ("Choose a show to
  watch" → Library), and everything caught up ("Find something new" → Discover).
- **Social.** `HomeView` takes an optional `social` node for the beta area. Home is complete
  without it, and the alpha route passes none.
- **Welcome.** The export's copy and steps with Create account and Sign in, in the account
  screens' brand-only frame. A browser that already has a session goes to `/home`.

## Checks

The fixture app renders the production `/home` and `/` routes on port 3124. A stateful fake of
the C05 Home, show, progress, catch-up and undo API answers at the `/api/v1` boundary. This is
not live integration (R24 owns that).

```sh
yarn playwright test --config apps/web/src/features/home/playwright.config.ts
node apps/web/src/features/home/fixture-server.mjs --build
```

Registering this suite in root scripts and CI is R01-owned. See
[verification evidence](verification.md).
