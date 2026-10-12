# Episode detail (R22, C05/C07)

`EpisodeDetail` renders `/episodes/[episodeId]`: the show/code header, protected details only
as `GET /episodes/{id}` authorizes them, the scoped reveal with Hide again, individual watched
actions, future/undated/special notices and the explicit catch-up link.

- **Protected data.** `use-episode.ts` holds the only copy of episode details, from the last
  authorized read. Hide again, reveal expiry (`expiresAt`), a page restored from the
  back/forward cache and any 401 redact synchronously before a request, discard reads in
  flight, and bump `cacheEpoch`, which remounts the screen with a fresh tracking client so
  episode lists loaded by the catch-up sheet are dropped too. While a Hide is unconfirmed a
  revealed read is redacted on arrival, so a failed Hide never shows details again.
- **Reveal.** A confirmation names the code and says it lasts for this sign-in session and
  marks nothing. Details appear only from the read after `POST /reveals`; if only that read
  fails, a toast offers Retry. No CSS blur: locked responses have no protected fields to render.
- **Watched actions** use the shared tracking client (`features/tracking`): Idempotency-Key,
  optimistic state and Undo. A mark never unlocks details; every successful write, including
  Undo from a toast, rereads the episode. The release label is catalog data and stays after
  marking a future or undated episode.
- **Titles.** `metadata` is generic and the client sets `Show S1 E3 · SceneCask`. Headings,
  accessible names and still alt text use the code, never the protected title. The server
  renders no episode data.
- **Discussions.** `discussions` is a reserved slot for the beta; the alpha route passes
  nothing, so nothing renders or is requested.

## Checks

The fixture app renders the production route on port 3123; a stateful fake of the C05/C07
API (progress, undo, catch-up, reveal grants, sign-out) answers at the `/api/v1` boundary.
This is not live integration (R24 owns that gate).

```sh
yarn playwright test --config apps/web/src/features/catalog/episode/playwright.config.ts
node apps/web/src/features/catalog/episode/fixture-server.mjs --build
```

Registering this suite in root scripts and CI is R01-owned. See
[verification evidence](verification.md).
