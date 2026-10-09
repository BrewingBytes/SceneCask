# Tracking client (R21, C05)

`createTrackingClient(api, showId, toasts)` (React: `useTracking`) is the shared client
for show/episode reads and progress, library, catch-up, history and undo writes.

- **Idempotency.** Every write sends an Idempotency-Key. After an unknown outcome
  (network/503) the same body reuses the same key, so Retry is never applied twice; a known
  rejection discards it. `keyedUndo` does the same for Undo.
- **Optimistic patches.** Each action owns a patch (one episode, all episodes for erase,
  status, saved). Settling removes only that action's patch, so concurrent actions roll back
  independently. Patches never contain protected data: a mark does not unlock details;
  details arrive only from a follow-up authorized `GET /episodes/{id}`.
- **Ordering.** Responses apply in server order (tracking revision, then library revision);
  episode rows never move to an older revision. Conflicts and undo refetch the show and
  loaded seasons.
- **Catch-up sheet.** Choosing an endpoint requests an explicit preview showing the
  included range, count and exclusions (already watched, unreleased, undated, specials).
  Commit sends only the preview ID. A 409/410 (or a locally expired preview) fetches a new
  preview, shows "Episodes changed…" and waits for a new confirmation; nothing is resubmitted
  silently. Add mode also offers "Haven’t started yet" (Plan to watch, no episodes).

The HTTP transport (`createApiTransport`) and `keyedUndo` live in
`features/catalog/search/api.ts` and are shared with Discover.
