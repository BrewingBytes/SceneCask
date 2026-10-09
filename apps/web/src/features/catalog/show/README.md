# Show detail (R21, C05)

`ShowDetail` renders `/shows/[showId]`: poster, info, library status and progress,
season and Specials tabs, episode rows, and the add/catch-up sheet. All reads and writes
go through the shared tracking client in `features/tracking`.

- **Episode rows.** The toggle marks or unmarks only that episode, including future and
  undated ones; their "Airs …"/"Release date unknown" label stays after marking. Titles
  appear only when the backend returned `details`; locked rows say "Title hidden".
  Accessible names use episode codes only. The row menu offers "Mark watched through…"
  (regular episodes) and View episode.
- **Library panel.** Add to library (opens the sheet), Add back (keeps history), status
  menu (Plan to watch/Watching/On hold/Dropped; Caught up/Completed are computed),
  Mark next, Catch up…, Remove from library (history kept) and Erase watch history.
- **Erase history** opens a destructive confirmation before any request. Failures stay
  in the dialog; success offers Undo.
- The open season is kept in `?season=` and pinned, so a progress change never moves it.

## Checks

The fixture app renders the production route on port 3122; a stateful fake of the C05 API
answers at the `/api/v1` boundary. This is not live integration (R24 owns that gate).

```sh
yarn playwright test --config apps/web/src/features/catalog/show/playwright.config.ts
node apps/web/src/features/catalog/show/fixture-server.mjs --build
```

Registering this suite in root scripts and CI is R01-owned. See
[verification evidence](verification.md).
