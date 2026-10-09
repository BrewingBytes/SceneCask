# R21 verification — 2026-10-10

Prerequisites R05 (#59), R17 (#91) and R18 (#90) are merged and are ancestors of this
branch. Changes stay in `features/catalog/show/**`, `features/tracking/**` and
`app/shows/**`, plus a shared-transport extraction in `features/catalog/search/api.ts`
(below).

## Automated evidence

| Command | Result |
| --- | --- |
| `CI=1 yarn playwright test --config apps/web/src/features/catalog/show/playwright.config.ts` | Pass: 15 Chromium tests, 3 consecutive runs |
| Same suite with idempotency-key reuse removed | The concurrent/unknown-outcome test fails |
| Same suite with a stale preview silently resubmitted | The stale catch-up test fails |
| `CI=1 yarn playwright test --config apps/web/src/features/catalog/search/playwright.config.ts` | Pass: 20 tests (Discover after the transport extraction) |
| `node apps/web/src/features/catalog/show/fixture-server.mjs --build` | Pass |
| `yarn lint` | Pass (duplication at baseline 9 clones; web ESLint; Rust fmt/clippy) |
| `yarn typecheck` | Pass |
| `yarn build` | Pass; `/shows/[showId]` is a dynamic route |
| `yarn api:check` | Pass: no drift. No contract changes |
| `yarn test` with local `.env` (PostgreSQL 18) | Pass: 228 Rust tests. No backend changes |
| `yarn test:e2e` with local `.env` | Pass: 6 existing tests. Not R21 integration evidence |

Acceptance criteria:

- **One click changes no earlier episode.** Marking S1 E3 sends exactly one
  `PUT /progress/episodes/{S1E3}` `{watched:true,expectedRevision:0}` with a UUID key and
  CSRF token. S1 E1/E2 stay unpressed and unwritten; Next stays S1 E1 and out-of-order is
  shown. The title unlocks only after `GET /episodes/{id}`; no other row or accessible name
  contains protected text.
- **Future episode.** S1 E6 (future) is enabled; after marking it reads
  "Airs Nov 12, 2026 (estimated) · Watched". The undated S1 E4 can be marked too.
- **Stale catch-up.** Preview through S1 E5 lists S1 E2–E3, E5 plus exclusions (already
  watched, release date unknown, specials). Another device marks S1 E3; confirm returns
  409, a new preview (S1 E2, E5) and an alert appear, and no second commit is sent until
  "Mark 2 watched" is pressed. The second commit uses the new preview ID.
- **Erase history.** The destructive alertdialog appears with no request; Escape cancels
  with no request. Confirm sends `DELETE /shows/{id}/history` `{expectedRevision:3}`;
  status stays On hold; the toast's Undo restores the episodes. A rejected erase keeps the
  dialog open with product copy and changes nothing.
- **Optimistic races.** S1 E1 (503 after 600ms) and S1 E2 marked concurrently: E2 stays
  watched, E1 rolls back alone, a repeat click while busy is ignored, and Retry resends
  the same key and body. A 409 refetches and shows the other device's mark.
- Add sheet "Haven’t started yet" sends `{saved:true,status:"plan_to_watch",expectedRevision:0}`
  and no preview. The row menu opens the sheet previewed through that episode; Escape
  returns focus to the row menu with no writes. Season tabs work by arrow/End keys, the
  season survives reload via `?season=`; Specials have no catch-up action. Status menu by
  keyboard sends `PATCH {status:"on_hold",expectedRevision:1}`. Load failure Retry and
  not-found states use product copy, never server messages.
- **Responsive (320/390/860/1440).** No horizontal overflow, controls ≥44px, axe (WCAG 2
  A/AA, 2.1 AA) clean for the page, catch-up sheet and erase dialog. Reduced motion is
  emulated in every test.

Screenshots in `verification/` (390 and 1440): show, catch-up preview, stale catch-up,
erase confirmation.

## Shared transport

`features/catalog/search/api.ts` now exports `createApiTransport` (the CSRF bootstrap and
failure classification Discover already used) and `keyedUndo`. Discover's behavior is
unchanged and its suite passes. This touches R19's module and needs its owner's sign-off.

## Limitations

- Fixture harness only; R24 owns live integration and network-isolation evidence.
- `/episodes/[episodeId]` (episode links and View episode) belongs to another issue; the
  fixture app stubs it.
- `ToastViewport` is mounted by the page until the integration owner mounts one at the root.
- The page fetches client-side; server-rendered show data with cookie forwarding is R24's.
- Registering this suite in root scripts/CI is R01-owned.
- Manual checks were done through Playwright and screenshot review, not a separate
  hand-run browser session.
